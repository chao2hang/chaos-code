#!/usr/bin/env bash
# MT-1: keep the Cargo package version and the npm package versions from drifting.
#
# Why this exists: `release.yml` resolves the release version from
# `npm/chaos/package.json`, stamps it across the platform packages, and injects
# it into the Rust build as `GROK_VERSION`. Without an injected `GROK_VERSION`
# the Rust side falls back to `CARGO_PKG_VERSION` (see
# `xai-grok-version/src/lib.rs`). So the version a user sees in `chaos --version`
# and the version npm resolved are two independent literals, and nothing compared
# them: a bump applied to only one would ship a binary whose self-reported version
# disagrees with the package that installed it — the same class of defect as the
# "binary calls itself grok" bug, just in a string nobody diffs.
#
# Checks (all must agree):
#   1. `xai-grok-pager-bin` `[package] version`      (what `chaos --version` falls back to)
#   2. `npm/chaos/package.json` version              (what release.yml resolves)
#   3. every `npm/chaos-*/package.json` version      (the per-platform packages)
#   4. `npm/chaos/package.json` optionalDependencies (pins the platform packages by version)
#   5. the set of platform packages matches the sibling directories on disk
set -euo pipefail

cd "$(dirname "$0")/../.."

NPM_DIR="crates/codegen/xai-grok-pager/npm"
CARGO_TOML="crates/codegen/xai-grok-pager-bin/Cargo.toml"

if ! command -v node >/dev/null 2>&1; then
  echo "check-versions: node is required to parse package.json" >&2
  exit 1
fi

# First `version = "…"` inside `[package]`. Restricting to that section keeps a
# `version = "…"` under `[dependencies]` from being mistaken for the package's.
cargo_version="$(awk '
  /^\[/ { section = $0 }
  section == "[package]" && /^version[[:space:]]*=/ {
    sub(/^version[[:space:]]*=[[:space:]]*"/, "");
    sub(/".*$/, "");
    print;
    exit;
  }
' "$CARGO_TOML")"

if [[ -z "$cargo_version" ]]; then
  echo "check-versions: could not read [package] version from $CARGO_TOML" >&2
  exit 1
fi

main_version="$(node -p "require('./$NPM_DIR/chaos/package.json').version")"
expect="$cargo_version"
status=0

report_mismatch() {
  printf 'check-versions: MISMATCH %s\n  expected %s (= %s)\n  actual   %s\n' \
    "$1" "$expect" "$cargo_version" "$2" >&2
  status=1
}

echo "check-versions: Cargo $CARGO_TOML -> $cargo_version"

[[ "$main_version" == "$expect" ]] ||
  report_mismatch "$NPM_DIR/chaos/package.json version" "$main_version"

# Per-platform packages, discovered on disk so a newly added platform cannot
# escape the check by not being listed anywhere.
for pkg in "$NPM_DIR"/chaos-*/package.json; do
  [[ -f "$pkg" ]] || continue
  v="$(node -p "require('./$pkg').version")"
  [[ "$v" == "$expect" ]] || report_mismatch "$pkg version" "$v"
done

# optionalDependencies must pin every platform package at the same version, and
# must not name a package that no longer exists (or omit one that does).
#
# Compare on the packages' `name` fields, not their directory names: the
# directory `chaos-linux-x64` publishes as the package `chaos-code-linux-x64`,
# so the two legitimately differ and comparing them would always fail.
declared="$(node -p "
  Object.entries(require('./$NPM_DIR/chaos/package.json').optionalDependencies || {})
    .map(([k, v]) => k + ' ' + v)
    .sort()
    .join('\n')
")"

while read -r name ver; do
  [[ -n "${name:-}" ]] || continue
  [[ "$ver" == "$expect" ]] ||
    report_mismatch "$NPM_DIR/chaos/package.json optionalDependencies[\"$name\"]" "$ver"
done <<<"$declared"

declared_names="$(grep -v '^$' <<<"$declared" | cut -d' ' -f1 | sort)"
pkg_names="$(node -p "
  require('fs').readdirSync('$NPM_DIR', { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => '$NPM_DIR/' + e.name + '/package.json')
    .filter((p) => require('fs').existsSync(p))
    .map((p) => require('./' + p).name)
    .filter((n) => n && n.startsWith('chaos-code-'))
    .sort()
    .join('\n')
")"

if [[ "$declared_names" != "$pkg_names" ]]; then
  printf 'check-versions: MISMATCH platform package set\n' >&2
  printf '  optionalDependencies names:\n%s\n' "$declared_names" >&2
  printf '  package.json names on disk:\n%s\n' "$pkg_names" >&2
  status=1
fi

if [[ "$status" -ne 0 ]]; then
  echo "check-versions: FAILED" >&2
  exit 1
fi

echo "check-versions: OK — Cargo and all npm packages agree on $expect"
