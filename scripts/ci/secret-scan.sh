#!/usr/bin/env bash
# Scan files for credential material and local-machine artifacts.
#
# Single source of truth for the patterns: scripts/hooks/pre-commit pipes staged
# paths in, and .github/workflows/ci.yml runs it over the whole tracked tree.
# Duplicating these lists in two places would let them drift, and a security
# check that silently stops matching is worse than none.
#
# Usage:
#   secret-scan.sh                 # scan all tracked files at HEAD
#   printf '%s\n' f1 f2 | secret-scan.sh --stdin
#
# Exit 0 = clean, 1 = findings. Findings print paths and line numbers only,
# never matched text: echoing a secret into CI logs is the spread this prevents.
set -uo pipefail

# --- Path shapes that are never source --------------------------------------
# Literal Win32 paths, temp-file names, env dumps, key/credential files.
PATH_RE='(^|/)C:'
PATH_RE="$PATH_RE|Temp\.tmp"
PATH_RE="$PATH_RE|(^|/)\.tmp[A-Za-z0-9]{6}"
PATH_RE="$PATH_RE|(^|/)[^/]*env\.txt$"
PATH_RE="$PATH_RE|(^|/)\.env($|\.)"
PATH_RE="$PATH_RE|\.(pem|p12|pfx|jks)$"
PATH_RE="$PATH_RE|(^|/)(credentials|auth)\.json$"
PATH_RE="$PATH_RE|(^|/)id_(rsa|ed25519)$"

# --- Credential-shaped content ----------------------------------------------
# Mostly prefix-anchored: a bare KEY=<value> rule fires on half the Rust test
# fixtures here, gets muted, and then protects nothing. The one general rule
# (last) is narrowed to SHOUTING_ENV_VAR= with a long unquoted value, which is
# the shape of a process environment dump and not of Rust source.
#
# POSIX ERE only — this is matched by awk below, which does NOT support \b.
# The leaked dump went undetected in an early version of this script for
# exactly that reason, so use the explicit boundary group instead.
B='(^|[^A-Za-z0-9_-])'
# `"Authorization":"Bearer x"` (JSON, as in the leaked MCP config) as well as
# the bare header form. The JSON encoding defeated the header-only pattern.
CONTENT_RE='(Authorization"?[[:space:]]*:[[:space:]]*"?Bearer[[:space:]]+[A-Za-z0-9._~+/-]{20,})'
CONTENT_RE="$CONTENT_RE|(${B}sk-[A-Za-z0-9_-]{20,})"
CONTENT_RE="$CONTENT_RE|(${B}ctx7sk-[A-Za-z0-9_-]{10,})"
CONTENT_RE="$CONTENT_RE|(${B}gh[pousr]_[A-Za-z0-9]{30,})"
CONTENT_RE="$CONTENT_RE|(${B}xai-[A-Za-z0-9]{20,})"
CONTENT_RE="$CONTENT_RE|(${B}BSA[A-Za-z0-9_-]{20,})"
CONTENT_RE="$CONTENT_RE|(${B}AKIA[0-9A-Z]{16})"
CONTENT_RE="$CONTENT_RE|(-----BEGIN[[:space:]]+[A-Z ]*PRIVATE KEY-----)"
# The value must start with something other than $ or ` so that shell
# assignments like OIDC_TOKEN=$(read_grok_token ...) — references, never
# literals — do not trip the rule.
CONTENT_RE="$CONTENT_RE|([A-Z][A-Z0-9_]*(API_KEY|APIKEY|SECRET|PASSWORD|TOKEN|CREDENTIALS)=[^[:space:]\"'\$\`][^[:space:]\"']{15,})"

# --- Exemptions -------------------------------------------------------------
# The redaction test suites (xai-grok-secrets, xai-grok-telemetry) assert that
# secrets get scrubbed, so they necessarily contain secret-shaped literals. A
# line is exempt if it carries an obvious placeholder marker, or an explicit
# `secret-scan:allow` pragma. Exempting whole files instead would blind the
# scanner to a real key checked in beside a fixture.
ALLOW_RE='CANARY|LEAK[A-Z]*|EXAMPLE|REDACTED|PLACEHOLDER|DUMMY|FAKE|NOTAREAL|secret-scan:allow'

# --staged reads blob content from the index rather than the working tree, so a
# partially staged file (git add -p) is judged by what is actually being
# committed. The hook uses it; CI, which has no index of its own, does not.
mode=tree
case "${1:-}" in
    --staged)
        mode=staged
        mapfile -t files < <(git diff --cached --name-only --diff-filter=ACMR)
        ;;
    --stdin) mapfile -t files ;;
    *) mapfile -t files < <(git ls-files) ;;
esac

read_file() {
    if [ "$mode" = staged ]; then git show ":$1" 2>/dev/null; else cat "$1"; fi
}

fail=0

for path in "${files[@]}"; do
    [ -n "$path" ] || continue
    if printf '%s\n' "$path" | grep -qEi "$PATH_RE"; then
        echo "secret-scan: local-artifact / credential path: $path" >&2
        fail=1
        continue
    fi
    [ "$mode" = staged ] || [ -f "$path" ] || continue

    # Skip binaries. awk has no equivalent of grep -I, and feeding it a 780 KB
    # notice blob or a lockfile is both slow and prone to nonsense matches.
    # A NUL byte in the first 8 KB is the same heuristic git itself uses.
    if read_file "$path" | head -c 8192 | LC_ALL=C grep -q $'\0' 2>/dev/null; then
        continue
    fi

    # Line numbers only; matched text is never printed.
    # awk, not grep -v: the pragma is honoured on the matching line *or* the one
    # above it, so a long fixture line can be annotated without a trailing
    # comment that rustfmt would have to accommodate.
    lines=$(read_file "$path" | awk -v c="$CONTENT_RE" -v a="$ALLOW_RE" '
        { if ($0 ~ c && $0 !~ a && prev !~ a) print NR; prev = $0 }
    ' 2>/dev/null | head -3)
    if [ -n "$lines" ]; then
        echo "secret-scan: credential-shaped content in $path" >&2
        printf '  line %s\n' $lines >&2
        fail=1
    fi
done

if [ "$fail" -ne 0 ]; then
    cat >&2 <<'EOF'

Findings above. If any of this reached a pushed commit, deleting the file is
not remediation: the blob stays fetchable from the remote indefinitely. Revoke
the credential first, then decide about rewriting history.
EOF
    exit 1
fi

echo "secret-scan: clean (${#files[@]} files)"
exit 0
