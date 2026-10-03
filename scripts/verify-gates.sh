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
#   scripts/verify-gates.sh --list          # print the extracted labels, run nothing
#   scripts/verify-gates.sh --verbose       # stream every gate's output, not just failures
#   scripts/verify-gates.sh --self-test     # verify extraction and the failure path
#
# Capture evidence with:
#   scripts/verify-gates.sh 2>&1 | tee verify-gates-$(date +%Y%m%d).log
set -uo pipefail

script_path="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
gates_source="${VERIFY_GATES_SOURCE:-${repo_root}/scripts/verify-in-docker.sh}"

# Written literally, then used as a prefix pattern: single quotes, so this is the text
# `${bootstrap}; ` and not an expansion. Matching the placeholder is what keeps this runner
# from having to know what the container's bootstrap lines do.
bootstrap_prefix='${bootstrap}; '

mode="run"
include_build="no"
verbose="no"

for arg in "$@"; do
  case "$arg" in
    --with-build) include_build="yes" ;;
    --verbose) verbose="yes" ;;
    --list) mode="list" ;;
    --self-test) mode="self-test" ;;
    -h | --help)
      sed -n '2,37p' "$0"
      exit 0
      ;;
    *)
      echo "unknown argument: $arg (expected --with-build, --list, --verbose, --self-test or --help)" >&2
      exit 2
      ;;
  esac
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

run_gates() {
  local source_file="$1"
  local gate label command_line rc
  local total=0 ran=0 skipped=0 failures=0
  local failed=""
  local out

  if [ ! -f "$source_file" ]; then
    echo "verify-gates: no such gate source: $source_file" >&2
    return 2
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
  if [ "$failures" -ne 0 ]; then
    echo "FAILED gates:${failed}"
    echo "${ran} gate(s) run, ${skipped} skipped, ${failures} failed"
    return 1
  fi
  echo "all gates passed on the host (${ran} run, ${skipped} skipped)"
  return 0
}

# The runner's own fixture suite, and the gate that checks the thing that checks the gates.
# Every case names the failure mode it exists to catch, and --self-test is wired into
# scripts/verify-in-docker.sh's array so the extractor is also held against the real file.
self_test() {
  local work pass=0 fail=0 real_list lines
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
    run_gates "${gates_source}"
    ;;
esac
