#!/usr/bin/env bash
# scripts/l10n-guard.sh
#
# Detect Chinese (Han) localization regression between two git refs.
# Used by the chaos-upstream-sync skill to catch the historical problem
# of upstream merges silently clobbering Chaos-fork Chinese UI strings.
#
# Usage:
#   l10n-guard.sh --before <ref> --after <ref> [options]
#
# Outputs (in --report dir, default /tmp/l10n-guard-$$):
#   before-files.txt     files with Han chars at --before
#   after-files.txt      files with Han chars at --after
#   regressed.txt        before ∖ after  (Chinese disappeared)
#   shrunk.txt           files where Han count decreased
#   fortress-breach.txt  fortress files missing at --after
#
# Exit: 0 = pass, 1 = fail, 64 = usage error.
#
# Run from repo root. Requires: git, rg (ripgrep with Unicode support).
set -euo pipefail

# ---- Defaults & arg parsing ----------------------------------------------
BEFORE="HEAD"
AFTER="WORKTREE"
REPORT_DIR=""
FORTRESS=()
EXCLUDE=()

# Default fortress: pager paths with high Chinese density (see
# .agents/skills/chaos-upstream-sync/references/chaos-fork-map.md).
DEFAULT_FORTRESS=(
  "crates/codegen/xai-grok-pager/src/slash/commands"
  "crates/codegen/xai-grok-pager/src/views"
  "crates/codegen/xai-grok-pager/src/diagnostics"
  "crates/codegen/xai-grok-pager/src/doctor_cmd"
  "crates/codegen/xai-grok-pager/src/headless.rs"
  "crates/codegen/xai-grok-pager/src/acp"
  "crates/codegen/xai-grok-pager/src/startup.rs"
  "crates/codegen/xai-grok-pager/src/models.rs"
  "crates/codegen/xai-grok-shell/src/agent"
)

usage() {
  cat >&2 <<'USAGE'
l10n-guard.sh — detect Chinese (Han) localization regression

USAGE:
  l10n-guard.sh --before <ref> --after <ref> [options]

OPTIONS:
  --before <ref>       Git ref for baseline (default: HEAD)
  --after  <ref>       Git ref for comparison (default: working tree)
  --fortress <path>    Path prefix of "fortress" files (Chinese mandatory).
                       Repeatable. Default: pager high-risk paths.
  --exclude <path>     Path prefix to skip (e.g. tests/fixtures).
                       Repeatable.
  --report <dir>       Output directory (default: /tmp/l10n-guard-PID)
  -h, --help           Show this help

REPORTS (in --report):
  before-files.txt     Files with Han chars at --before
  after-files.txt      Files with Han chars at --after
  regressed.txt        before ∖ after  (Chinese disappeared — FAIL)
  shrunk.txt           Files where Han count decreased (FAIL)
  fortress-breach.txt  Fortress files missing at --after (FAIL)

EXIT: 0 = pass, 1 = fail, 64 = usage error.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --before)  BEFORE="$2"; shift 2 ;;
    --after)   AFTER="$2";  shift 2 ;;
    --fortress) FORTRESS+=("$2"); shift 2 ;;
    --exclude)  EXCLUDE+=("$2");  shift 2 ;;
    --report)  REPORT_DIR="$2";  shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "l10n-guard: unknown arg: $1" >&2; usage; exit 64 ;;
  esac
done

if [[ ${#FORTRESS[@]} -eq 0 ]]; then
  FORTRESS=("${DEFAULT_FORTRESS[@]}")
fi

# ---- Sanity checks --------------------------------------------------------
if ! command -v rg >/dev/null 2>&1; then
  echo "l10n-guard: rg (ripgrep) not found" >&2
  exit 64
fi
if ! command -v git >/dev/null 2>&1; then
  echo "l10n-guard: git not found" >&2
  exit 64
fi
# Quick rg Unicode support probe
if ! echo "中" | rg -o '[\p{Han}]' >/dev/null 2>&1; then
  echo "l10n-guard: rg lacks Unicode property support (need ripgrep with -PCRE2 or build with unicode)" >&2
  exit 64
fi

# Default report dir
[[ -z "$REPORT_DIR" ]] && REPORT_DIR="/tmp/l10n-guard-$$"
mkdir -p "$REPORT_DIR"
rm -f "$REPORT_DIR"/*.txt "$REPORT_DIR"/*.tsv

# ---- Helper: list all .rs files under crates/ at a ref --------------------
# $1 = ref ("WORKTREE" or a commit-ish)
list_rs_files() {
  local ref="$1"
  if [[ "$ref" == "WORKTREE" ]]; then
    git ls-files -co --exclude-standard -- 'crates/**/*.rs' 2>/dev/null
  else
    git ls-tree -r --name-only "$ref" -- 'crates/' 2>/dev/null \
      | grep -E '\.rs$' || true
  fi
}

# ---- Helper: count Han chars in a single file at a ref --------------------
# $1 = ref, $2 = path
count_han_in() {
  local ref="$1" file="$2"
  if [[ "$ref" == "WORKTREE" ]]; then
    if [[ -f "$file" ]]; then
      rg -o --no-filename '[\p{Han}]' "$file" 2>/dev/null | wc -l
    else
      echo 0
    fi
  else
    git show "${ref}:${file}" 2>/dev/null \
      | rg -o '[\p{Han}]' | wc -l
  fi
}

# ---- Apply exclude filter (path prefix match) ----------------------------
is_excluded() {
  local file="$1"
  for pat in "${EXCLUDE[@]}"; do
    # shellcheck disable=SC2053
    [[ "$file" == ${pat}* ]] && return 0
  done
  return 1
}

# ---- Build manifest: TSV of (count<TAB>path) for files with count > 0 -----
# $1 = ref, $2 = out TSV path
build_manifest() {
  local ref="$1" out="$2"
  : > "$out"
  local files
  mapfile -t files < <(list_rs_files "$ref" | sort -u)
  if [[ ${#files[@]} -eq 0 ]]; then
    return
  fi
  # xargs parallel count; emit "<n>\t<path>" only when n > 0
  printf '%s\n' "${files[@]}" \
    | xargs -I{} -P 4 -n 1 bash -c '
        ref="$1"; file="$2"
        if [[ "$ref" == "WORKTREE" ]]; then
          if [[ ! -f "$file" ]]; then exit 0; fi
          n=$(rg -o --no-filename "[\p{Han}]" "$file" 2>/dev/null | wc -l)
        else
          n=$(git show "${ref}:${file}" 2>/dev/null | rg -o "[\p{Han}]" | wc -l)
        fi
        n=${n:-0}
        if [[ "$n" -gt 0 ]]; then
          printf "%s\t%s\n" "$n" "$file"
        fi
      ' _ "$ref" {} \
    | sort -t$'\t' -k1,1 -n -r >> "$out"
}

# ---- Run -----------------------------------------------------------------
echo "[l10n-guard] before=$BEFORE  after=$AFTER" >&2
echo "[l10n-guard] report dir: $REPORT_DIR" >&2

build_manifest "$BEFORE" "$REPORT_DIR/before.tsv"
build_manifest "$AFTER"  "$REPORT_DIR/after.tsv"

awk -F'\t' '$1 > 0 {print $2}' "$REPORT_DIR/before.tsv" > "$REPORT_DIR/before-files.txt"
awk -F'\t' '$1 > 0 {print $2}' "$REPORT_DIR/after.tsv"  > "$REPORT_DIR/after-files.txt"

sort -u "$REPORT_DIR/before-files.txt" > "$REPORT_DIR/before-files.sorted"
sort -u "$REPORT_DIR/after-files.txt"  > "$REPORT_DIR/after-files.sorted"

# regressed = before ∖ after
comm -23 "$REPORT_DIR/before-files.sorted" "$REPORT_DIR/after-files.sorted" \
  > "$REPORT_DIR/regressed.txt"

# shrunk = file in both, after count < before count
# Read before.tsv and after.tsv as: path -> count
awk -F'\t' '
  FILENAME == ARGV[1] { before[$2] = $1 + 0; next }
  { after[$2] = $1 + 0 }
  END {
    for (f in after) {
      if ((f in before) && after[f] < before[f]) {
        printf "%s\tbefore=%d\tafter=%d\tdelta=%d\n", f, before[f], after[f], before[f] - after[f]
      }
    }
  }
' "$REPORT_DIR/before.tsv" "$REPORT_DIR/after.tsv" \
  | sort > "$REPORT_DIR/shrunk.txt"

# fortress-breach = fortress files at BEFORE missing at AFTER
: > "$REPORT_DIR/fortress-breach.txt"
for pat in "${FORTRESS[@]}"; do
  # find files at BEFORE matching the path prefix
  while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    is_excluded "$f" && continue
    # Check membership in after-files.sorted (exact match)
    if ! grep -Fxq -- "$f" "$REPORT_DIR/after-files.sorted"; then
      echo "$f" >> "$REPORT_DIR/fortress-breach.txt"
    fi
  done < <(awk -F'\t' '$1 > 0 {print $2}' "$REPORT_DIR/before.tsv" \
            | grep -F -- "$pat" || true)
done
sort -u "$REPORT_DIR/fortress-breach.txt" -o "$REPORT_DIR/fortress-breach.txt"

# ---- Report --------------------------------------------------------------
n_before=$(wc -l < "$REPORT_DIR/before-files.txt" | tr -d ' ')
n_after=$(wc -l < "$REPORT_DIR/after-files.txt" | tr -d ' ')
n_regressed=$(wc -l < "$REPORT_DIR/regressed.txt" | tr -d ' ')
n_shrunk=$(wc -l < "$REPORT_DIR/shrunk.txt" | tr -d ' ')
n_fortress=$(wc -l < "$REPORT_DIR/fortress-breach.txt" | tr -d ' ')

echo "" >&2
echo "=== L10n Guard Report ===" >&2
echo "  before Han files:  $n_before" >&2
echo "  after  Han files:  $n_after" >&2
echo "  regressed:         $n_regressed" >&2
echo "  shrunk:            $n_shrunk" >&2
echo "  fortress-breach:   $n_fortress" >&2

failed=0
if [[ "$n_regressed" -gt 0 ]]; then
  echo "" >&2
  echo "FAIL — regressed (Chinese disappeared):" >&2
  cat "$REPORT_DIR/regressed.txt" >&2
  failed=1
fi
if [[ "$n_shrunk" -gt 0 ]]; then
  echo "" >&2
  echo "FAIL — shrunk (Chinese count decreased):" >&2
  cat "$REPORT_DIR/shrunk.txt" >&2
  failed=1
fi
if [[ "$n_fortress" -gt 0 ]]; then
  echo "" >&2
  echo "FAIL — fortress breach (must-restore files):" >&2
  cat "$REPORT_DIR/fortress-breach.txt" >&2
  failed=1
fi

if [[ "$failed" -eq 0 ]]; then
  echo "" >&2
  echo "OK — L10n Guard: PASS" >&2
  exit 0
fi

echo "" >&2
echo "L10n Guard: FAIL — restore Chinese before merging." >&2
exit 1
