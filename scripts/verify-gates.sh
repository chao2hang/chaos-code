#!/usr/bin/env bash
# Run the gate list from scripts/verify-in-docker.sh on this machine, without Docker.
#
# scripts/verify-in-docker.sh is the instrument that proves the documented steps work from
# a cold start, and it is the right thing to run before shipping. It is the wrong thing to
# reach for after a ten-line edit: it builds an image and then runs `cargo check` and
# `cargo clippy` over the whole workspace inside it, so the cheap guards a change actually
# touched answer an hour later. The workaround had been a throwaway loop over
# `scripts/ci/*.py` written once per session, which is why the gate tables in evidence logs
# could not quote a command a reader can re-run.
#
# The list is not copied here, because a copy is a second source of truth that eventually
# stops agreeing with the first. It is read out of the `gates=()` array of
# scripts/verify-in-docker.sh, so a gate added there shows up here on the next run with the
# same label and the same command line. Two transformations, both deliberate, nothing else:
#
#   * the `${bootstrap}` prefix (git safe.directory plus an identity) is dropped. The
#     container needs it because the tree is bind-mounted from a foreign uid; a test runner
#     rewriting a contributor's global git config is not acceptable.
#   * the build gates (`cargo check`, `cargo clippy`, `cargo test`, `GUI protocol types`)
#     are skipped unless --with-build, because rebuilding the workspace is the part this
#     runner exists to avoid.
#
# Environment:
#   VERIFY_GATES_SOURCE  file to read the gates array from (default
#                        scripts/verify-in-docker.sh; --self-test points it at fixtures)
#
# Usage:
#   scripts/verify-gates.sh                 # cheap gates, in array order
#   scripts/verify-gates.sh --with-build    # plus cargo check/clippy/test and GUI types
#   scripts/verify-gates.sh --only <label>  # only the gates whose label matches (repeatable)
#   scripts/verify-gates.sh --list          # print the extracted labels, run nothing
#   scripts/verify-gates.sh --verbose       # stream every gate's output, not just failures
#   scripts/verify-gates.sh --self-test     # verify extraction and the failure path
#
# --only filters by label, exactly or as a fragment, and filters only: the build gates stay
# skipped under it unless --with-build is also given, because a fragment that happens to match
# `cargo test` should not silently turn a fast loop into a workspace rebuild. Its summary says
# `K of T selected by --only` instead of reading like a sweep.
#
# Capture evidence with:
#   scripts/verify-gates.sh 2>&1 | tee verify-gates-$(date +%Y%m%d).log
set -uo pipefail

script_path="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
gates_source="${VERIFY_GATES_SOURCE:-${repo_root}/scripts/verify-in-docker.sh}"

# The container exports this for every gate it runs (`--env RUST_MIN_STACK` in
# scripts/verify-in-docker.sh), and the workspace test leg needs it: `xai-grok-shell`'s
# current-thread actor tests overflow the harness default stack, which is the reason that
# --env line exists. Mirroring only the command lines and not the environment they run in
# made this runner's `--with-build` test leg fail on a stack overflow no other runner sees:
# the same tree, 2026-10-04, exits 0 with the value set (386 result blocks, 31,594 tests) and
# dies on `-p xai-grok-shell --lib` without it. An already-set value wins, as it does in the
# container.
RUST_MIN_STACK_DEFAULT=16777216
export RUST_MIN_STACK="${RUST_MIN_STACK:-${RUST_MIN_STACK_DEFAULT}}"

# Written literally, then used as a prefix pattern: single quotes, so this is the text
# `${bootstrap}; ` and not an expansion. Matching the placeholder is what keeps this runner
# from having to know what the container's bootstrap lines do.
bootstrap_prefix='${bootstrap}; '

mode="run"
include_build="no"
verbose="no"
# One pattern per line; empty means no filter. Kept as a string rather than an array because
# the matcher reads it with `read -r` and the file is also sourced by the self-test fixtures.
only=""
only_count=0

while [ $# -gt 0 ]; do
  case "$1" in
    --with-build) include_build="yes" ;;
    --verbose) verbose="yes" ;;
    --list) mode="list" ;;
    --self-test) mode="self-test" ;;
    --only)
      # The value is `$2`, not `$1`: `$1` is the literal `--only`. An empty pattern is refused
      # rather than parsed, because `matches_only` skips empty lines and so `--only ""` would
      # select nothing -- in `--list` mode that is an exit 0 with no output at all.
      if [ $# -lt 2 ] || [ -z "$2" ]; then
        echo "--only needs a gate label (list them with: scripts/verify-gates.sh --list)" >&2
        exit 2
      fi
      shift
      only="${only}${1}"$'\n'
      only_count=$((only_count + 1))
      ;;
    -h | --help)
      # The header up to the first non-comment line, so `--help` cannot drift out of sync
      # with the number of lines added above it.
      awk 'NR > 1 { if ($0 !~ /^#/) exit; print }' "$0"
      exit 0
      ;;
    *)
      echo "unknown argument: $1 (expected --with-build, --only <label>, --list, --verbose, --self-test or --help)" >&2
      exit 2
      ;;
  esac
  shift
done

# One gate per output line: every `"label: command"` entry inside `gates=( ... )`, plus the
# `gates+=( ... )` lines the full mode appends. Comment lines inside the array and the
# closing `)` are not gates. The `["]` classes are there because the awk program is
# single-quoted and the quote characters have to reach awk as data.
extract_gates() {
  awk '
    /^[[:space:]]*gates=\(/           { inblock = 1; next }
    /^[[:space:]]*gates\+=\(["]/      { line = $0
                                   sub(/^[[:space:]]*gates\+=\(["]/, "", line)
                                   sub(/["][)]$/, "", line)
                                   print line; next }
    inblock && /^[[:space:]]*#/  { next }
    inblock && /^[[:space:]]*\)/ { inblock = 0; next }
    inblock && /^[[:space:]]*["]/ { line = $0
                                   sub(/^[[:space:]]*["]/, "", line)
                                   sub(/["][[:space:]]*$/, "", line)
                                   print line }
  ' "$1"
}

# The gates this runner exists to keep off a fast loop, matched on the exact label. A
# renamed build gate therefore shows up as a cheap gate and runs, rather than being skipped
# by a fuzzy match nobody notices.
is_build_gate() {
  case "$1" in
    "cargo check" | "cargo clippy" | "cargo test" | "GUI protocol types") return 0 ;;
    *) return 1 ;;
  esac
}

# Is this label selected by any --only pattern? Exact label or fragment, with the pattern
# quoted so a bracket in it stays a bracket. Selection never adds a gate: a gate the array
# does not contain cannot be run by naming it.
matches_only() {
  local label="$1" pattern
  while IFS= read -r pattern; do
    [ -n "$pattern" ] || continue
    case "$label" in
      "$pattern" | *"$pattern"*) return 0 ;;
    esac
  done <<< "${only}"
  return 1
}

run_gates() {
  local source_file="$1"
  local gate label command_line rc pattern labels
  local total=0 ran=0 skipped=0 failures=0 filtered=0
  local failed=""
  local out

  if [ ! -f "$source_file" ]; then
    echo "verify-gates: no such gate source: $source_file" >&2
    return 2
  fi

  # A --only pattern that names nothing is a typo, and the run would otherwise print a summary
  # about zero gates and exit 0. Every pattern is therefore checked against the labels before
  # anything runs, so a mistyped filter costs no build time and cannot look like a pass.
  if [ "${only_count}" -gt 0 ]; then
    labels="$(extract_gates "${source_file}" | sed 's/: .*//')"
    while IFS= read -r pattern; do
      [ -n "$pattern" ] || continue
      if ! printf '%s\n' "${labels}" | grep -qF -- "${pattern}"; then
        echo "verify-gates: --only ${pattern} matches no gate label in ${source_file}" >&2
        echo '              labels are listed by: scripts/verify-gates.sh --list' >&2
        return 2
      fi
    done <<< "${only}"
  fi

  while IFS= read -r gate; do
    [ -n "$gate" ] || continue
    total=$((total + 1))

    label="${gate%%: *}"
    command_line="${gate#*: }"
    if [ "$label" = "$gate" ]; then
      echo "verify-gates: array entry has no 'label: command' shape: ${gate}" >&2
      echo "              the format of ${source_file} changed; fix this runner, do not skip the gate" >&2
      return 2
    fi
    command_line="${command_line#"$bootstrap_prefix"}"

    if [ "${only_count}" -gt 0 ] && ! matches_only "${label}"; then
      filtered=$((filtered + 1))
      continue
    fi

    if [ "$mode" = "list" ]; then
      printf '%s\n' "$label"
      continue
    fi

    if [ "$include_build" = "no" ] && is_build_gate "$label"; then
      skipped=$((skipped + 1))
      printf 'SKIP  %s (build gate; --with-build runs it)\n' "$label"
      continue
    fi

    out="$(bash -c "$command_line" 2>&1)"
    rc=$?
    ran=$((ran + 1))
    if [ "$verbose" = "yes" ] && [ -n "$out" ]; then
      printf '%s\n' "$out" | sed 's/^/    | /'
    fi
    if [ "$rc" -eq 0 ]; then
      printf 'PASS  %s\n' "$label"
    else
      printf 'FAIL  %s (exit %s)\n' "$label" "$rc"
      # The reason is at the end: a guard prints its findings and then its summary. Forty
      # lines covers both, and keeps one red gate from burying the others under a cargo-sized
      # dump.
      printf '%s\n' "$out" | tail -n 40 | sed 's/^/    /'
      failed="${failed} ${label}"
      failures=$((failures + 1))
    fi
  done < <(extract_gates "$source_file")

  if [ "$mode" = "list" ]; then
    return 0
  fi
  if [ "$total" -eq 0 ]; then
    echo "verify-gates: extracted 0 gates from ${source_file}" >&2
    echo '              a source is a `gates=(` line followed by one quoted entry per line' >&2
    return 2
  fi

  echo
  # A filtered run has to say it is filtered: a summary indistinguishable from a full sweep is
  # how a partial pass gets quoted back as "all gates passed".
  local scope=""
  if [ "${only_count}" -gt 0 ]; then
    scope=", $((ran + skipped)) of ${total} selected by --only"
  fi
  if [ "$failures" -eq 0 ] && [ "$ran" -eq 0 ] && [ "${only_count}" -gt 0 ]; then
    # Selecting a gate is not the same as measuring it. A filter whose every match is a skipped
    # build gate ran nothing, and exit 0 would read as a clean sweep.
    echo "nothing ran: every selected gate was skipped ($((ran + skipped)) of ${total} gates selected by --only)" >&2
    echo '              the build gates (cargo check/clippy/test, GUI protocol types) need --with-build' >&2
    return 1
  fi
  if [ "$failures" -ne 0 ]; then
    echo "FAILED gates:${failed}"
    echo "${ran} gate(s) run, ${skipped} skipped, ${failures} failed${scope}"
    return 1
  fi
  echo "all gates passed on the host (${ran} run, ${skipped} skipped${scope})"
  return 0
}

# The runner's own fixture suite, and the gate that checks the thing that checks the gates.
# Every case names the failure mode it exists to catch, and --self-test is wired into
# scripts/verify-in-docker.sh's array so the extractor is also held against the real file.
self_test() {
  local work pass=0 fail=0 real_list lines container_stack
  LAST_OUT=""
  LAST_RC=0

  work="$(mktemp -d)"
  cd "$work" || return 2

  # The fixture mirrors the real file's placeholder. `bootstrap` is exported so that a
  # surviving prefix expands, inside the gate's own bash, to a command that aborts it: with
  # the strip in place the placeholder never reaches a shell, while without it the gate runs
  # `exit 44`. An unset variable would not do, because bash expands `${bootstrap}; true` to
  # an empty command plus `true` and reports success (measured: exit 0).
  bootstrap='exit 44'
  export bootstrap

  cat >all-pass.sh <<'FIXTURE'
gates=(
  # a comment inside the array is not a gate
  "always passes: true"
  "bootstrap prefix is dropped: ${bootstrap}; true"
)
  gates+=("appended entry is picked up: true")
FIXTURE

  cat >one-fails.sh <<'FIXTURE'
gates=(
  "a gate that fails: echo the-output-has-to-be-shown >&2; exit 3"
)
FIXTURE

  cat >no-array.sh <<'FIXTURE'
# a gate source with no gates array at all
mode="quick"
FIXTURE

  cat >broken-entry.sh <<'FIXTURE'
gates=(
  "an entry with no command"
)
FIXTURE

  cat >build-gate.sh <<'FIXTURE'
gates=(
  "cargo check: exit 9"
  "cheap neighbour: true"
)
FIXTURE

  # Two labels sharing a fragment with a failing gate in between: a filter that matched
  # everything, or matched the pair but kept walking, ends up running the failure.
  cat >pair.sh <<'FIXTURE'
gates=(
  "docs guard first: true"
  "unrelated: exit 7"
  "docs guard second: true"
)
FIXTURE

  # The environment is part of the mirror, not an extra on top of it. This fixture gate reads
  # the value back, and EXPECTED_STACK comes from the container's own --env line, so the two
  # copies cannot drift apart without one of these two cases going red.
  cat >env-mirror.sh <<'FIXTURE'
gates=(
  "a gate sees the mirrored stack size: test \"${RUST_MIN_STACK:-}\" = \"${EXPECTED_STACK}\""
)
FIXTURE

  run_case() {
    local name="$1" expected="$2" fixture="$3"
    shift 3
    LAST_OUT="$(VERIFY_GATES_SOURCE="${work}/${fixture}" bash "${script_path}" "$@" 2>&1)"
    LAST_RC=$?
    if [ "${LAST_RC}" -eq "${expected}" ]; then
      printf 'ok    %s\n' "${name}"
      pass=$((pass + 1))
    else
      printf 'not ok %s (expected exit %s, got %s)\n' "${name}" "${expected}" "${LAST_RC}"
      printf '%s\n' "${LAST_OUT}" | tail -n 10 | sed 's/^/      /'
      fail=$((fail + 1))
    fi
  }

  expect_line() {
    local name="$1" pattern="$2"
    if printf '%s\n' "${LAST_OUT}" | grep -q -- "${pattern}"; then
      printf 'ok    %s\n' "${name}"
      pass=$((pass + 1))
    else
      printf 'not ok %s (no line matching: %s)\n' "${name}" "${pattern}"
      fail=$((fail + 1))
    fi
  }

  # The other half of a filter: a test that only greps for the gate that ran cannot tell a
  # filter from a full sweep, so every selection case also has to assert the absent line.
  reject_line() {
    local name="$1" pattern="$2"
    if printf '%s\n' "${LAST_OUT}" | grep -q -- "${pattern}"; then
      printf 'not ok %s (unexpected line matching: %s)\n' "${name}" "${pattern}"
      printf '%s\n' "${LAST_OUT}" | tail -n 10 | sed 's/^/      /'
      fail=$((fail + 1))
    else
      printf 'ok    %s\n' "${name}"
      pass=$((pass + 1))
    fi
  }

  run_case "a passing set exits 0" 0 all-pass.sh
  expect_line "the appended gates+=(...) entry is extracted" "^PASS  appended entry is picked up"
  expect_line "the bootstrap placeholder is stripped, not executed" "^PASS  bootstrap prefix is dropped"
  expect_line "a comment inside the array is not counted as a gate" "(3 run, 0 skipped)"

  run_case "a failing gate exits 1" 1 one-fails.sh
  expect_line "the failing gate is named" "^FAIL  a gate that fails"
  expect_line "the failing gate's own output is shown" "the-output-has-to-be-shown"
  expect_line "the summary repeats the failed gate" "^FAILED gates: a gate that fails"

  run_case "a source with no gates array is an error" 2 no-array.sh
  expect_line "and it says what it looked for" "extracted 0 gates"

  run_case "an entry the extractor cannot split is an error" 2 broken-entry.sh
  expect_line "naming the entry rather than skipping it" "no 'label: command' shape"

  run_case "a build gate is skipped by default" 0 build-gate.sh
  expect_line "and says that it skipped" "^SKIP  cargo check"
  run_case "the same gate runs under --with-build" 1 build-gate.sh --with-build

  # --only. The absent lines matter as much as the present ones: a filter that silently ran
  # everything would satisfy any grep for the gate it was supposed to select.
  run_case "--only runs just the gate it names" 0 all-pass.sh --only "appended entry is picked up"
  expect_line "the named gate ran" "^PASS  appended entry is picked up"
  reject_line "and the gates it did not name did not run" "^PASS  always passes"
  expect_line "the summary says the run was filtered" "1 of 3 selected by --only"
  reject_line "so it does not read as an unfiltered sweep" "all gates passed on the host (1 run, 0 skipped)"

  run_case "a fragment selects every label containing it" 0 pair.sh --only "docs guard"
  expect_line "the first of the pair ran" "^PASS  docs guard first"
  expect_line "and the second of the pair" "^PASS  docs guard second"
  reject_line "the gate sitting between them did not run" "unrelated"
  expect_line "which is why this run exits 0 rather than 1" "2 of 3 selected by --only"

  run_case "a pattern naming no gate is an error" 2 all-pass.sh --only "no such gate anywhere"
  expect_line "reported by the pattern, before any gate ran" "matches no gate label"
  reject_line "and nothing ran on the way to the error" "^PASS  always passes"

  run_case "a repeated --only unions its patterns" 0 all-pass.sh \
    --only "always passes" --only "appended entry is picked up"
  expect_line "both gates ran" "2 of 3 selected by --only"

  run_case "--only does not unlock a build gate by itself" 1 build-gate.sh --only "cargo check"
  expect_line "the gate is selected and then skipped" "^SKIP  cargo check"
  expect_line "and the run reports that it measured nothing" "every selected gate was skipped"
  run_case "with --with-build the same filter runs it" 1 build-gate.sh --with-build --only "cargo check"
  expect_line "the build gate ran and failed" "^FAIL  cargo check"
  reject_line "and its cheap neighbour stayed filtered out" "cheap neighbour"

  run_case "--list honours the filter" 0 all-pass.sh --list --only "appended"
  expect_line "printing just the selected label" "^appended entry is picked up$"
  reject_line "and not the others" "^always passes$"

  run_case "--only without a value is refused" 2 all-pass.sh --only

  # An empty string survives `[ $# -lt 2 ]`, and `matches_only` skips empty lines, so an
  # unparsed empty pattern selected nothing: `--list` came back exit 0 with no output at all.
  run_case "an empty --only pattern is refused, not parsed" 2 all-pass.sh --only ""
  expect_line "naming the flag instead of selecting nothing" "needs a gate label"
  reject_line "and no gate ran on the way to the error" "^PASS  always passes"

  container_stack="$(sed -n 's/.*--env RUST_MIN_STACK=\([0-9][0-9]*\).*/\1/p' \
    "${repo_root}/scripts/verify-in-docker.sh" | head -n 1)"
  if [ -n "${container_stack}" ]; then
    printf 'ok    the container still sets a stack size to mirror (%s)\n' "${container_stack}"
    pass=$((pass + 1))
  else
    printf 'not ok no RUST_MIN_STACK --env line to mirror in scripts/verify-in-docker.sh\n'
    fail=$((fail + 1))
  fi

  EXPECTED_STACK="${container_stack}"
  export EXPECTED_STACK
  run_case "a gate sees the stack size this runner exports" 0 env-mirror.sh
  # An operator who sets it deliberately is not overwritten by the default.
  LAST_OUT="$(RUST_MIN_STACK=4242 EXPECTED_STACK=4242 \
    VERIFY_GATES_SOURCE="${work}/env-mirror.sh" bash "${script_path}" 2>&1)"
  LAST_RC=$?
  if [ "${LAST_RC}" -eq 0 ]; then
    printf 'ok    an explicit RUST_MIN_STACK wins over the default\n'
    pass=$((pass + 1))
  else
    printf 'not ok an explicit RUST_MIN_STACK was overwritten (exit %s)\n' "${LAST_RC}"
    printf '%s\n' "${LAST_OUT}" | tail -n 6 | sed 's/^/      /'
    fail=$((fail + 1))
  fi

  if bash -n "${repo_root}/scripts/verify-in-docker.sh" 2>/dev/null; then
    printf 'ok    the gate source is itself valid bash\n'
    pass=$((pass + 1))
  else
    printf 'not ok the gate source does not parse: scripts/verify-in-docker.sh\n'
    fail=$((fail + 1))
  fi

  real_list="$(VERIFY_GATES_SOURCE="${repo_root}/scripts/verify-in-docker.sh" \
    bash "${script_path}" --list 2>&1)"
  lines="$(printf '%s\n' "${real_list}" | grep -c .)"
  if [ "${lines}" -ge 20 ]; then
    printf 'ok    the real entry file still parses (%s labels extracted)\n' "${lines}"
    pass=$((pass + 1))
  else
    printf 'not ok the real entry file parses (only %s labels extracted)\n' "${lines}"
    printf '%s\n' "${real_list}" | tail -n 10 | sed 's/^/      /'
    fail=$((fail + 1))
  fi
  LAST_OUT="${real_list}"
  expect_line "and a known cheap gate is among them" "^secret scan$"
  LAST_OUT="${real_list}"
  expect_line "and the array's own header gate is among them" "^cargo fmt$"

  echo
  rm -rf "${work}"
  if [ "${fail}" -ne 0 ]; then
    echo "verify-gates self-test: ${pass} passed, ${fail} FAILED"
    return 1
  fi
  echo "verify-gates self-test: ${pass}/${pass} cases pass"
  return 0
}

case "${mode}" in
  list)
    cd "${repo_root}" || exit 2
    run_gates "${gates_source}"
    ;;
  self-test)
    self_test
    ;;
  *)
    cd "${repo_root}" || exit 2
    echo "== gates extracted from ${gates_source}"
    echo "   host run: the \${bootstrap} prefix is dropped, build gates are $([ "${include_build}" = yes ] && echo included || echo skipped)"
    echo "   env: RUST_MIN_STACK=${RUST_MIN_STACK:-unset} (mirrors the container's --env; set it to override)"
    if [ "${only_count}" -gt 0 ]; then
      echo "   --only is in effect (${only_count} pattern(s)); this is a filtered run, not a sweep"
    fi
    run_gates "${gates_source}"
    ;;
esac
