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
#   disappeared-raw.txt  before ∖ after, before classification
#   moved.txt            of those, files whose Chinese lines survive verbatim
#                        elsewhere at --after (renames / extracted modules)
#   removed-recorded.txt of those, files whose Chinese is really gone, covered
#                        by an entry in --allow-file
#   regressed.txt        unexcused loss (FAIL)
#   stale-allowlist.txt  --allow-file entries whose path is in the working
#                        tree, i.e. a removal that did not happen (FAIL)
#   shrunk.txt           files where Han count decreased
#   fortress-breach.txt  fortress files missing at --after
#
# Exit: 0 = pass, 1 = fail, 64 = usage error.
#
# Run from repo root. Requires: git, rg (ripgrep with Unicode support).
set -euo pipefail

# A guard that dies part-way must say so. `set -e` on its own leaves no trace:
# the 2026-10-03 run of scripts/verify-in-docker.sh recorded this gate as FAIL
# with nothing printed after "report dir" -- the same shape as a run that was
# interrupted, and unlike a verdict no reader can act on. errtrace keeps the
# trap alive inside the functions and subshells below, where the work happens.
# The once-marker is a file rather than a variable because the deaths this guards
# against happen in subshells, where an in-shell flag never comes back.
set -E
_DEATH_MARK="${TMPDIR:-/tmp}/l10n-guard-death-$$"
on_err() {
  local rc=$?
  if [[ -e "$_DEATH_MARK" ]]; then
    return 0
  fi
  : >"$_DEATH_MARK" 2>/dev/null || true
  printf 'l10n-guard: NO VERDICT -- died at line %s (exit %s) while running: %s\n' \
    "${BASH_LINENO[0]:-?}" "$rc" "${BASH_COMMAND:-unknown}" >&2
  printf 'l10n-guard: partial output is in %s\n' "${REPORT_DIR:-<not yet chosen>}" >&2
}
trap on_err ERR
trap 'rm -f "$_DEATH_MARK"' EXIT

# ---- Defaults & arg parsing ----------------------------------------------
SCRIPT_DIR="$(cd -- "$(dirname -- "$0")" && pwd -P)"
BEFORE="HEAD"
AFTER="WORKTREE"
REPORT_DIR=""
ALLOW_FILE="$SCRIPT_DIR/ci/l10n-removed-allowlist.tsv"
USE_ALLOW_LIST=1
MOVE_MIN=90
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
  --allow-file <path>  TSV of adjudicated removals (default: the
                       scripts/ci/l10n-removed-allowlist.tsv next to this
                       script). One "<path>\t<reason>" per line; "#" starts a
                       comment. Excuses a path only if it is *absent* at
                       --after -- see REPORTS below. An entry whose path is in
                       the working tree is itself a failure.
  --no-allow-list      Ignore the allow list entirely.
  --move-min <percent> Minimum share of a disappeared file's unique Chinese
                       lines that must reappear somewhere else at --after for
                       it to count as moved (default: 90; at least 2 of them
                       must match).
  --report <dir>       Output directory (default: /tmp/l10n-guard-PID)
  -h, --help           Show this help

REPORTS (in --report):
  disappeared-raw.txt  Files with Han at --before and none at --after
  moved.txt            ... whose Chinese lines survive verbatim at another
                       path: a rename or an extracted module (OK)
  removed-recorded.txt ... whose removal is recorded in the allow list (OK)
  regressed.txt        Unexcused loss (FAIL). A path that still exists at
                       --after but lost its Chinese is always here -- the
                       allow list cannot excuse it, because that is exactly
                       the upstream-clobber case this guard exists for.
  stale-allowlist.txt  Allow-list entries whose path is in the working tree,
                       i.e. recording a removal that did not happen (FAIL)
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
    --allow-file) ALLOW_FILE="$2"; shift 2 ;;
    --no-allow-list) USE_ALLOW_LIST=0; shift ;;
    --move-min) MOVE_MIN="$2"; shift 2 ;;
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

# An allow-list entry without a reason is an unexplained exemption, which is the
# thing the list is supposed to prevent. Reject the file rather than read it.
if [[ "$USE_ALLOW_LIST" -eq 1 && -n "$ALLOW_FILE" && -f "$ALLOW_FILE" ]]; then
  malformed=$(awk -F'\t' '/^[[:space:]]*#/ {next} NF == 0 {next} NF < 2 || $2 == "" {
                 printf "%s:%d: %s\n", FILENAME, FNR, $0
               }' "$ALLOW_FILE")
  if [[ -n "$malformed" ]]; then
    echo "l10n-guard: $ALLOW_FILE has entries without a reason (need \"<path>\\t<reason>\"):" >&2
    printf '%s\n' "$malformed" >&2
    exit 1
  fi
fi

# ---- Helper: list all .rs files under crates/ at a ref --------------------
# $1 = ref ("WORKTREE" or a commit-ish)
# No `2>/dev/null` here: git's own complaint is the only clue a reader gets when
# a side of the comparison comes back empty for a reason that has nothing to do
# with the change under test (dangling ref, foreign owner, killed helper).
list_rs_files() {
  local ref="$1"
  if [[ "$ref" == "WORKTREE" ]]; then
    git ls-files -co --exclude-standard -- 'crates/**/*.rs'
  else
    git ls-tree -r --name-only "$ref" -- 'crates/' \
      | { grep -E '\.rs$' || true; }
  fi
}

# ---- Apply exclude filter (path prefix match) ----------------------------
is_excluded() {
  local file="$1" pat
  # `${EXCLUDE[@]+...}` because an empty array is an unbound expansion under
  # `set -u` on the bash 3.2 that macOS still ships.
  for pat in ${EXCLUDE[@]+"${EXCLUDE[@]}"}; do
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
  local files=()
  # The listing is captured, not piped through a process substitution, because
  # `< <(...)` throws away the exit status: a killed or misconfigured git then
  # looks like "this side has no files", which makes every disappeared-file
  # comparison vacuously pass. An empty side is a failure, never a clean run.
  local listing
  if ! listing="$(list_rs_files "$ref")"; then
    printf 'l10n-guard: cannot list .rs files at %s -- refusing to compare against a broken side\n' "$ref" >&2
    return 1
  fi
  # `mapfile` is bash 4; macOS still ships 3.2.
  local sorted line
  sorted="$(printf '%s\n' "$listing" | sort -u)"
  while IFS= read -r line; do
    if [[ -n "$line" ]]; then
      files+=("$line")
    fi
  done <<<"$sorted"
  if [ "${#files[@]}" -eq 0 ]; then
    printf 'l10n-guard: no .rs files listed at %s -- an empty side cannot prove anything\n' "$ref" >&2
    return 1
  fi
  # xargs parallel count; emit "<n>\t<path>" only when n > 0
  printf '%s\n' "${files[@]}" \
    | xargs -I{} -P 4 -n 1 bash -c '
        ref="$1"; file="$2"
        if [[ "$ref" == "WORKTREE" ]]; then
          if [[ ! -f "$file" ]]; then exit 0; fi
          chars=$(rg -o --no-filename "[\p{Han}]" "$file" 2>/dev/null); rc=$?
        else
          # A file listed at the ref must be readable at the ref. Counting zero
          # because git failed would drop it from that side silently, and a file
          # missing from one side is exactly the signal this guard reports on.
          if ! chars=$(git show "${ref}:${file}"); then
            printf "l10n-guard: cannot read %s at %s\n" "$file" "$ref" >&2
            exit 9
          fi
          chars=$(printf "%s\n" "$chars" | rg -o "[\p{Han}]"); rc=$?
        fi
        # rg exits 1 for "no match", which is a real answer; anything above 1 is
        # rg itself failing (killed, bad pattern, unreadable file).
        if [[ "$rc" -gt 1 ]]; then
          printf "l10n-guard: cannot count Han in %s (rg exit %s)\n" "$file" "$rc" >&2
          exit 9
        fi
        if [[ -z "$chars" ]]; then
          exit 0
        fi
        n=$(printf "%s\n" "$chars" | grep -c "")
        if [[ "$n" -gt 0 ]]; then
          printf "%s\t%s\n" "$n" "$file"
        fi
      ' _ "$ref" {} \
    | sort -t$'\t' -k1,1 -n -r >> "$out"
}

# ---- Classification helpers ----------------------------------------------
# Does the path exist at a ref?
exists_at() {
  # $1 = ref, $2 = path
  if [[ "$1" == "WORKTREE" ]]; then
    [[ -e "$2" ]]
  else
    git cat-file -e "${1}:${2}" 2>/dev/null
  fi
}

# Unique, whitespace-trimmed lines containing Han, for one path at one ref.
han_lines_at() {
  local ref="$1" file="$2"
  {
    if [[ "$ref" == "WORKTREE" ]]; then
      [[ -f "$file" ]] && cat -- "$file"
    else
      git show "${ref}:${file}" 2>/dev/null
    fi
  } | rg --no-filename '[\p{Han}]' 2>/dev/null \
    | sed 's/^[[:space:]]*//; s/[[:space:]]*$//' | grep . | sort -u || true
}

# Every unique Han line in the whole after tree, deduped. A disappeared file's
# lines are looked up in this pool to tell "moved" from "really gone".
build_han_corpus() {
  local ref="$1" files="$2" out="$3"
  xargs -I{} -P 4 -n 1 bash -c '
      ref="$1"; file="$2"
      if [[ "$ref" == "WORKTREE" ]]; then
        if [[ ! -f "$file" ]]; then exit 0; fi
        rg --no-filename "[\p{Han}]" "$file" 2>/dev/null || true
      else
        git show "${ref}:${file}" 2>/dev/null | rg --no-filename "[\p{Han}]" || true
      fi
    ' _ "$ref" {} \
    < "$files" \
    | sed 's/^[[:space:]]*//; s/[[:space:]]*$//' | grep . | sort -u > "$out"
}

# Paths recorded in the allow list.
allow_list_paths() {
  [[ "$USE_ALLOW_LIST" -eq 1 && -n "$ALLOW_FILE" && -f "$ALLOW_FILE" ]] || return 0
  awk -F'\t' '!/^[[:space:]]*#/ && NF >= 2 && $2 != "" {print $1}' "$ALLOW_FILE"
}

# Reason recorded for one path; non-zero exit when the path is not recorded.
allow_reason() {
  local path="$1"
  [[ "$USE_ALLOW_LIST" -eq 1 && -n "$ALLOW_FILE" && -f "$ALLOW_FILE" ]] || return 1
  awk -F'\t' -v p="$path" '
    !/^[[:space:]]*#/ && $1 == p && $2 != "" { reason = $2; hit = 1 }
    END { if (!hit) exit 1; print reason }
  ' "$ALLOW_FILE"
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

# disappeared = before ∖ after
comm -23 "$REPORT_DIR/before-files.sorted" "$REPORT_DIR/after-files.sorted" \
  > "$REPORT_DIR/disappeared-raw.txt"

# ---- Adjudicate the disappeared files ------------------------------------
# "Chinese disappeared from this path" has three causes and only one of them is
# the regression this guard was written for, so each is classified separately:
#   moved    -- the path is gone but its Chinese lines are there verbatim at
#               another path: a rename, or a module extracted into its own
#               crate. Nothing lost its translation; the file changed address.
#   recorded -- the path is gone and the Chinese really is gone, and someone
#               wrote down why in the allow list (dead code retired, test-only
#               fixture data that never rendered).
#   regressed -- everything else. A path that still exists at --after but lost
#               its Chinese always lands here whether or not it is allow-listed:
#               the list is consulted only for absent paths, because an
#               in-place strip is precisely the upstream-clobber case.
build_han_corpus "$AFTER" "$REPORT_DIR/after-files.sorted" \
  "$REPORT_DIR/after-han-lines.txt"

: > "$REPORT_DIR/moved.txt"
: > "$REPORT_DIR/removed-recorded.txt"
: > "$REPORT_DIR/regressed.txt"
: > "$REPORT_DIR/excused.txt"
tmp_lines="$REPORT_DIR/.han-lines.$$"
while IFS= read -r f; do
  [[ -z "$f" ]] && continue
  if exists_at "$AFTER" "$f"; then
    printf '%s\t(path exists at %s with no Chinese in it)\n' "$f" "$AFTER" \
      >> "$REPORT_DIR/regressed.txt"
    continue
  fi
  han_lines_at "$BEFORE" "$f" > "$tmp_lines"
  total=$(wc -l < "$tmp_lines" | tr -d ' ')
  hits=$(grep -c -Fx -f "$tmp_lines" "$REPORT_DIR/after-han-lines.txt" 2>/dev/null || true)
  hits=${hits:-0}
  if [ "$hits" -ge 2 ] && [ $((hits * 100)) -ge $((total * MOVE_MIN)) ]; then
    printf '%s\t(%s of %s Chinese lines reappear at another path)\n' \
      "$f" "$hits" "$total" >> "$REPORT_DIR/moved.txt"
    printf '%s\n' "$f" >> "$REPORT_DIR/excused.txt"
    continue
  fi
  if reason=$(allow_reason "$f"); then
    printf '%s\t(%s)\n' "$f" "$reason" >> "$REPORT_DIR/removed-recorded.txt"
    printf '%s\n' "$f" >> "$REPORT_DIR/excused.txt"
    continue
  fi
  printf '%s\t(%s of %s Chinese lines reappear elsewhere; no allow-list entry)\n' \
    "$f" "$hits" "$total" >> "$REPORT_DIR/regressed.txt"
done < "$REPORT_DIR/disappeared-raw.txt"
rm -f "$tmp_lines"
sort -u "$REPORT_DIR/excused.txt" -o "$REPORT_DIR/excused.txt"

# An allow-list entry whose path is sitting in the working tree records a removal
# that has not happened. Checked against the working tree rather than against
# --after on purpose: judging it by the compared refs would make `--before HEAD
# --after HEAD` call every entry stale (both sides still have the file), and
# would miss the case that matters -- somebody restored the file later, outside
# the range being compared. Either way the entry has to go, or the list quietly
# turns into a permanent exemption nobody re-read.
: > "$REPORT_DIR/stale-allowlist.txt"
if [[ "$USE_ALLOW_LIST" -eq 1 && -n "$ALLOW_FILE" && -f "$ALLOW_FILE" ]]; then
  while IFS= read -r p; do
    [[ -z "$p" ]] && continue
    if [[ -e "$p" ]]; then
      printf '%s\t(path is in the working tree; the removal it records did not happen -- delete this entry)\n' "$p" \
        >> "$REPORT_DIR/stale-allowlist.txt"
    fi
  done < <(allow_list_paths)
fi

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

# fortress-breach = fortress files at BEFORE missing at AFTER. A file the
# classification already excused (moved, or a recorded removal) is not a
# breach -- otherwise a rename inside a fortress path, or retiring dead code in
# one, could never pass.
: > "$REPORT_DIR/fortress-breach.txt"
for pat in "${FORTRESS[@]}"; do
  # find files at BEFORE matching the path prefix
  while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    is_excluded "$f" && continue
    grep -Fxq -- "$f" "$REPORT_DIR/excused.txt" && continue
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
n_moved=$(wc -l < "$REPORT_DIR/moved.txt" | tr -d ' ')
n_recorded=$(wc -l < "$REPORT_DIR/removed-recorded.txt" | tr -d ' ')
n_stale=$(wc -l < "$REPORT_DIR/stale-allowlist.txt" | tr -d ' ')
n_regressed=$(wc -l < "$REPORT_DIR/regressed.txt" | tr -d ' ')
n_shrunk=$(wc -l < "$REPORT_DIR/shrunk.txt" | tr -d ' ')
n_fortress=$(wc -l < "$REPORT_DIR/fortress-breach.txt" | tr -d ' ')

echo "" >&2
echo "=== L10n Guard Report ===" >&2
echo "  before Han files:  $n_before" >&2
echo "  after  Han files:  $n_after" >&2
echo "  moved:             $n_moved" >&2
echo "  removed-recorded:  $n_recorded" >&2
echo "  regressed:         $n_regressed" >&2
echo "  stale-allowlist:   $n_stale" >&2
echo "  shrunk:            $n_shrunk" >&2
echo "  fortress-breach:   $n_fortress" >&2

failed=0
if [[ "$n_moved" -gt 0 ]]; then
  echo "" >&2
  echo "OK — moved (Chinese followed the file to a new path):" >&2
  cat "$REPORT_DIR/moved.txt" >&2
fi
if [[ "$n_recorded" -gt 0 ]]; then
  echo "" >&2
  echo "OK — removed as recorded in $(basename "$ALLOW_FILE"):" >&2
  cat "$REPORT_DIR/removed-recorded.txt" >&2
fi
if [[ "$n_regressed" -gt 0 ]]; then
  echo "" >&2
  echo "FAIL — regressed (Chinese disappeared, unexcused):" >&2
  cat "$REPORT_DIR/regressed.txt" >&2
  failed=1
fi
if [[ "$n_stale" -gt 0 ]]; then
  echo "" >&2
  echo "FAIL — stale allow-list entries:" >&2
  cat "$REPORT_DIR/stale-allowlist.txt" >&2
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
