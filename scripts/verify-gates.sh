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
#   VERIFY_GATES_CHANGED newline-separated path list overriding what this runner believes
#                        is changed (see the NOT COVERED rule below; --self-test sets it so
#                        its fixtures do not inherit the real checkout's diff)
#   VERIFY_GATES_LOG_DIR directory for every gate's full output (same as --log-dir; the
#                        default is a timestamped directory under target/verify-gates)
#
# A failing gate prints its own last 40 lines, which is a reading aid and deliberately not
# the record: cargo names the failing target at the very end of a test leg, after the output
# of the target that failed. Measured on 2026-10-05, where the only lines the sweep kept were
# `error: 1 target failed:` / `-p chaos-engine --lib` plus 38 lines of an unrelated crate's
# doctests, and the name of the failing test -- which cargo prints ~1,000 lines earlier -- was
# gone. So every gate's complete output is also written to a file, named in the failure block
# and in the summary, and the failing test names and targets are lifted out of the full text
# and printed regardless of where they sit in it.
#
# Usage:
#   scripts/verify-gates.sh                 # cheap gates, in array order
#   scripts/verify-gates.sh --with-build    # plus cargo check/clippy/test and GUI types
#   scripts/verify-gates.sh --only <label>  # only the gates whose label matches (repeatable)
#   scripts/verify-gates.sh --list          # print the extracted labels, run nothing
#   scripts/verify-gates.sh --verbose       # stream every gate's output, not just failures
#   scripts/verify-gates.sh --log-dir DIR   # where to keep every gate's full output
#   scripts/verify-gates.sh --self-test     # verify extraction and the failure path
#   scripts/verify-gates.sh --allow-unbuilt-changes
#                                           # skip the build gates even with Rust changed
#
# --only filters by label, exactly or as a fragment, and filters only: the build gates stay
# skipped under it unless --with-build is also given, because a fragment that happens to match
# `cargo test` should not silently turn a fast loop into a workspace rebuild. Its summary says
# `K of T selected by --only` instead of reading like a sweep.
#
# Skipping the build gates is the point of this runner, but the skip is not allowed to come back
# as a pass. Four of the gates (cargo check/clippy/test, GUI protocol types) are what CI runs on
# every push, and on 2026-10-04 a batch whose only Rust change was a new test file swept green
# here -- `all gates passed on the host (35 run, 4 skipped)` -- and CI's clippy leg failed on it
# within the hour. So when the build gates were skipped and the diff holds a file one of them
# measures (`*.rs`, `*.toml`, `*.lock`, the generated protocol types), the run ends `NOT COVERED`
# with exit 1 and names the files, rather than printing a verdict about a tree it never compiled.
# `--allow-unbuilt-changes` is there for the run where that is the intended answer.
#
# Capture evidence with:
#   scripts/verify-gates.sh 2>&1 | tee verify-gates-$(date +%Y%m%d).log
# and keep the per-gate files the run names in its summary: the tee'd log holds only the tails.
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
allow_unbuilt="no"
# One pattern per line; empty means no filter. Kept as a string rather than an array because
# the matcher reads it with `read -r` and the file is also sourced by the self-test fixtures.
only=""
only_count=0
# Where every gate's complete output is kept. Empty means "decide on the first write", so a
# `--list` run or a self-test that never needs the files creates no directory.
log_dir_opt="${VERIFY_GATES_LOG_DIR:-}"

while [ $# -gt 0 ]; do
  case "$1" in
    --with-build) include_build="yes" ;;
    --allow-unbuilt-changes) allow_unbuilt="yes" ;;
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
    --log-dir)
      # Same shape as --only: an empty value would mean "the default directory" while looking
      # like a caller's path, and a caller who passed a path expects their path.
      if [ $# -lt 2 ] || [ -z "$2" ]; then
        echo "--log-dir needs a directory (the default is under target/verify-gates)" >&2
        exit 2
      fi
      shift
      log_dir_opt="$1"
      ;;
    -h | --help)
      # The header up to the first non-comment line, so `--help` cannot drift out of sync
      # with the number of lines added above it.
      awk 'NR > 1 { if ($0 !~ /^#/) exit; print }' "$0"
      exit 0
      ;;
    *)
      echo "unknown argument: $1 (expected --with-build, --allow-unbuilt-changes, --only <label>, --log-dir <dir>, --list, --verbose, --self-test or --help)" >&2
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

# Of the skipped build gates' four, which changed files could change the verdict? `cargo
# check`, `cargo clippy` and `cargo test` read every Rust source and every manifest in the
# workspace; `GUI protocol types` compares the schema binary's output against the one
# generated TypeScript file. Nothing else a diff can hold moves one of those four, so a
# docs-only sweep keeps the fast loop this runner exists to be.
is_build_relevant_path() {
  case "$1" in
    *.rs | *.toml | *.lock) return 0 ;;
    apps/chaos-ui/src/generated/protocol.ts) return 0 ;;
    *) return 1 ;;
  esac
}

# What CI is going to compile: everything differing from HEAD (staged and unstaged) plus
# every commit ahead of the branch this one pushes to. The second half is what catches the
# flow where the commit happened first and the sweep second. Outside a git checkout, or
# with no upstream configured, there is nothing to claim and the rule stays quiet rather
# than inventing a diff; VERIFY_GATES_CHANGED replaces the whole list.
changed_paths() {
  if [ -n "${VERIFY_GATES_CHANGED+x}" ]; then
    printf '%s\n' "${VERIFY_GATES_CHANGED}"
    return 0
  fi
  git rev-parse --git-dir >/dev/null 2>&1 || return 0
  git diff --name-only HEAD 2>/dev/null
  local upstream
  upstream="$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null)" || upstream=""
  if [ -n "${upstream}" ]; then
    git diff --name-only "${upstream}..HEAD" 2>/dev/null
  fi
  return 0
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

# Where a gate's complete output goes. The directory is made once, by a called statement
# rather than a command substitution, because a substitution runs in a subshell and the value
# would never reach the summary line. Decided on the first failure, so `--list` and a green
# sweep leave no directory behind; a tree where it cannot be made still gets the named failures
# below, just with no file to point at, because losing the log must not lose the verdict.
gate_log_dir=""
log_dir_ready="no"
log_dir_unusable="no"
ensure_log_dir() {
  local candidate
  [ "${log_dir_ready}" = "yes" ] && return 0
  [ "${log_dir_unusable}" = "yes" ] && return 1
  candidate="${log_dir_opt:-${repo_root}/target/verify-gates/$(date -u +%Y%m%dT%H%M%SZ)-$$}"
  if ! mkdir -p "${candidate}" 2>/dev/null; then
    log_dir_unusable="yes"
    return 1
  fi
  gate_log_dir="${candidate}"
  log_dir_ready="yes"
}

# Pure, once ensure_log_dir has succeeded. Labels hold spaces and the odd path; file names do not.
gate_log_path() {
  printf '%s/%s\n' "${gate_log_dir}" "$(printf '%s' "$1" | tr -cs 'A-Za-z0-9._-' '-')"
}

# Cargo prints the failing test names in the middle of a test leg and the failing target flags
# at the very end, and the tail this runner shows holds at most one of the two. Both are
# therefore lifted out of the complete output rather than out of what fits on screen, which is
# why they read the gate's output on stdin instead of a file -- the log is a convenience, and a
# tree that cannot write one still gets the names. The test names are matched as bare
# identifiers (or, for doc-tests, as cargo's own `path - item (line N)` shape) so a test's own
# indented stdout is not read as one, and each is tagged with the `Running`/`Doc-tests` header
# cargo printed above it, which is what makes a multi-target `--no-fail-fast` leg readable as
# "this target's this test went red" rather than as a pile.
failing_targets() {
  awk '
    /^error: [0-9]+ targets? failed:/        { wanted = 1; next }
    wanted && /^[[:space:]]+`[^`]+`$/ {
      gsub(/^[[:space:]]+/, "")
      gsub(/^`|`$/, "")
      print; next
    }
    wanted && /^[[:space:]]*$/               { next }
                                           { wanted = 0 }
  ' | awk '!seen[$0]++'
}

failing_pairs() {
  awk '
    /^ *(Running|Doc-tests) / { label = $0
                                sub(/^[[:space:]]+/, "", label)
                                sub(/[[:space:]]+$/, "", label)
                                next }
    /^failures:[[:space:]]*$/                       { wanted = 1; next }
    wanted && /^[[:space:]]{4}[A-Za-z_][A-Za-z0-9_:]*$/ {
      name = $0
      sub(/^[[:space:]]+/, "", name)
      print (label == "" ? "(no target header)" : label) "\t" name
      next
    }
    # A doc-test is not named by an identifier: cargo prints `path - item (line N)`, so that
    # shape needs its own anchor rather than a looser character class that indented prose
    # could also satisfy.
    wanted && /^[[:space:]]{4}[A-Za-z0-9_][A-Za-z0-9_.\/-]*( - [^(]+)? \(line [0-9]+\)$/ {
      name = $0
      sub(/^[[:space:]]+/, "", name)
      print (label == "" ? "(no target header)" : label) "\t" name
      next
    }
    wanted && /^[[:space:]]*$/                      { next }
                                                  { wanted = 0 }
  ' | awk '!seen[$0]++'
}

report_named_failures() {
  local text targets pairs total shown
  text="$(cat)"
  targets="$(printf '%s\n' "${text}" | failing_targets)"
  if [ -n "${targets}" ]; then
    printf '    target(s) cargo blames, as it says them:\n'
    printf '%s\n' "${targets}" | sed 's/^/      /'
  fi
  pairs="$(printf '%s\n' "${text}" | failing_pairs)"
  if [ -n "${pairs}" ]; then
    total="$(printf '%s\n' "${pairs}" | wc -l | tr -d ' ')"
    printf '    red, %s of them, one line each, tagged with the target cargo was running:\n' \
      "${total}"
    shown="$(printf '%s\n' "${pairs}" | head -n 40)"
    printf '%s\n' "${shown}" | awk -F'\t' '{ printf "      [%s] %s\n", $1, $2 }'
    if [ "${total}" -gt 40 ]; then
      printf '      ... %s more, in the full output\n' "$((total - 40))"
    fi
  fi
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
      # dump. It is not the record -- the whole output goes to a file, named below.
      local gate_log="" gate_lines=""
      if ensure_log_dir; then
        gate_log="$(gate_log_path "$(printf '%02d' "${ran}")-${label}")"
        if printf '%s\n' "${out}" >"${gate_log}" 2>/dev/null; then
          gate_lines="$(wc -l <"${gate_log}" | tr -d ' ')"
        else
          gate_log=""
        fi
      fi
      printf '%s\n' "$out" | tail -n 40 | sed 's/^/    /'
      printf '%s\n' "${out}" | report_named_failures
      if [ -n "${gate_log}" ]; then
        printf '    full output of this gate: %s (%s lines)\n' "${gate_log}" "${gate_lines}"
      fi
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
  # Named here as well as in each failure block, because the line a reader keeps is the summary.
  if [ -n "${gate_log_dir}" ]; then
    printf 'full gate output: %s\n' "${gate_log_dir}"
  fi
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
  # The skipped build gates and the diff are compared only on a run that otherwise looks green,
  # because that is the shape that gets quoted back as a verdict: 2026-10-04's red batch was
  # pushed behind `all gates passed on the host (35 run, 4 skipped)`. A skip that leaves
  # changes unmeasured is its own failure, not a footnote under a pass.
  local unbuilt="" path unbuilt_count=0
  if [ "${skipped}" -gt 0 ]; then
    while IFS= read -r path; do
      [ -n "$path" ] || continue
      is_build_relevant_path "$path" || continue
      unbuilt="${unbuilt}${path}"$'\n'
      unbuilt_count=$((unbuilt_count + 1))
    done < <(changed_paths | sort -u)
  fi
  if [ "${unbuilt_count}" -gt 0 ] && [ "${allow_unbuilt}" = "no" ]; then
    echo "NOT COVERED  ${skipped} build gate(s) skipped while ${unbuilt_count} changed file(s) are theirs to measure:" >&2
    printf '%s' "${unbuilt}" | head -n 5 | sed 's/^/              /' >&2
    if [ "${unbuilt_count}" -gt 5 ]; then
      echo "              ... and $((unbuilt_count - 5)) more" >&2
    fi
    echo '              run: scripts/verify-gates.sh --with-build' >&2
    echo '                     (or --allow-unbuilt-changes when those files are not meant to be compiled here)' >&2
    return 1
  fi
  if [ "$failures" -ne 0 ]; then
    echo "FAILED gates:${failed}"
    echo "${ran} gate(s) run, ${skipped} skipped, ${failures} failed${scope}"
    return 1
  fi
  if [ "${unbuilt_count}" -gt 0 ]; then
    # The operator said the skip is the intended answer. The pass line still has to carry what
    # was left unmeasured, or the log reads as a sweep to whoever meets it next.
    echo "  --allow-unbuilt-changes: ${unbuilt_count} changed file(s) the skipped build gates would have measured"
  fi
  echo "all gates passed on the host (${ran} run, ${skipped} skipped${scope})"
  return 0
}

# The runner's own fixture suite, and the gate that checks the thing that checks the gates.
# Every case names the failure mode it exists to catch, and --self-test is wired into
# scripts/verify-in-docker.sh's array so the extractor is also held against the real file.
self_test() {
  local work pass=0 fail=0 real_list lines container_stack
  local saved_log default_log
  LAST_OUT=""
  LAST_RC=0

  work="$(mktemp -d)"
  cd "$work" || return 2

  # The fixtures below are three lines of fake gates run against this real checkout, so the
  # diff the NOT COVERED rule reads must come from the fixtures, not from whatever Rust work
  # the tree happens to hold. Set-but-empty means "nothing changed"; the cases that want a
  # diff set it themselves.
  VERIFY_GATES_CHANGED=""
  export VERIFY_GATES_CHANGED
  # Every case is its own process, so without this each failing fixture would create its own
  # timestamped directory under the real target/. The default path is covered separately, by a
  # case that clears this and then checks where the file actually landed.
  VERIFY_GATES_LOG_DIR="${work}/gate-logs"
  export VERIFY_GATES_LOG_DIR

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

  # The shape that made this reporting necessary: cargo's own names for what failed, split
  # between the middle of the output (test names) and the very end (target flags), with enough
  # filler in front that neither can reach a printed tail. Reproduced from a real `cargo test`
  # leg on 2026-10-05 whose failing test name existed nowhere in the captured sweep log. It
  # carries two targets rather than the one the real leg had, so that which target a name is
  # attributed to is something a case can be wrong about, instead of a label that happens to be
  # right only because there was a single candidate.
  cat >cargo-shaped-output.sh <<'SH'
#!/bin/sh
printf '     Running unittests src/lib.rs (target/debug/deps/chaos_engine-80b8ed7d12d00ecb)\n'
i=0
while [ "$i" -lt 300 ]; do
  printf 'test t%s ... ok\n' "$i"
  i=$((i + 1))
done
printf '\nfailures:\n\n---- tests::buried_by_the_tail stdout ----\n\n'
printf "thread 'tests::buried_by_the_tail' panicked at src/lib.rs:4091:41:\nnote: run with RUST_BACKTRACE=1\n\n"
printf 'failures:\n    tests::buried_by_the_tail\n    a_module::another_one\n\n'
printf 'test result: FAILED. 299 passed; 2 failed; 0 ignored; 0 measured\n'
printf '\nerror: test failed, to rerun pass `-p chaos-engine --lib`\n'
printf '     Running unittests src/lib.rs (target/debug/deps/xai_tty_utils-654c93f9b2abfae2)\n'
printf 'test u0 ... ok\n'
printf '\nfailures:\n\n---- a_later_target_own_test stdout ----\n\n'
printf "thread 'a_later_target_own_test' panicked at src/lib.rs:1:1\n\n"
printf 'failures:\n    a_later_target_own_test\n\n'
printf 'test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured\n'
printf '\nerror: test failed, to rerun pass `-p xai-tty-utils --lib`\n'
printf '   Doc-tests chaos_engine\n\nrunning 1 test\ntest src/lib.rs - add (line 3) ... FAILED\n'
printf '\nfailures:\n\n---- src/lib.rs - add (line 3) stdout ----\n'
printf 'Test executable failed (exit status: 101).\n\n'
printf 'failures:\n    src/lib.rs - add (line 3)\n\n'
printf 'test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured\n'
printf '\nerror: doctest failed, to rerun pass `-p chaos-engine --doc`\n'
printf '\nerror: 3 targets failed:\n    `-p chaos-engine --lib`\n'
printf '    `-p xai-tty-utils --lib`\n    `-p chaos-engine --doc`\n'
exit 101
SH
  chmod +x cargo-shaped-output.sh

  # The runner runs its gates from the repository root, so the fixture reaches the helper by an
  # absolute path handed over in the environment -- the same channel the stack-size case above
  # uses, rather than a path substituted into the fixture at write time. Unquoted on purpose:
  # the fixture heredoc is quoted, so a backslash-escaped quote here would reach the gate's
  # shell as a literal `"` inside the file name it is asked to open.
  FIXTURE_SH="${work}/cargo-shaped-output.sh"
  export FIXTURE_SH

  cat >cargo-shaped.sh <<'FIXTURE'
gates=(
  "a gate shaped like cargo's test leg: sh ${FIXTURE_SH}"
)
FIXTURE

  # A guard that prints a line reading `failures:` and then indented prose must not have test
  # names invented for it. Without this fixture the identifier rule in `failing_tests` could be
  # loosened to any indented word and every case above would stay green.
  cat >indented-prose.sh <<'FIXTURE'
gates=(
  "a guard whose findings are indented: printf 'findings:\nfailures:\n    called `Result::unwrap()` on an Err value\n    note: run with RUST_BACKTRACE=1\n'; exit 5"
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

  # The skip has to cost something when there is a diff for the skipped gates to measure.
  # 2026-10-04: batch 11's only Rust change was a new test file, the sweep here said
  # `all gates passed on the host (35 run, 4 skipped)`, and CI's `cargo clippy --all-targets`
  # leg failed on the pushed commit. A green line about a tree this runner never compiled is
  # the failure mode, so the green line is what is being taken away here.
  VERIFY_GATES_CHANGED="crates/a.rs
crates/b.rs"
  run_case "a skipped build gate with changed Rust is not a pass" 1 build-gate.sh
  expect_line "it says which gates went unmeasured and why" "^NOT COVERED  1 build gate.s. skipped while 2 changed file.s."
  expect_line "and it names the files rather than the count alone" "^              crates/a.rs$"
  expect_line "pointing at the flag that would have covered them" "verify-gates.sh --with-build"
  run_case "unless told the files are deliberately unbuilt" 0 build-gate.sh --allow-unbuilt-changes
  expect_line "where the pass line still says what went unmeasured" \
    "--allow-unbuilt-changes: 2 changed file.s. the skipped build gates would have measured"
  run_case "and --with-build settles it by running the gate" 1 build-gate.sh --with-build
  reject_line "where the verdict is the gate's own failure, not the skip" "^NOT COVERED"

  VERIFY_GATES_CHANGED="docs/readme.md"
  run_case "a change no build gate reads keeps the sweep cheap and green" 0 build-gate.sh
  reject_line "so the fast loop is not held hostage by an unrelated diff" "^NOT COVERED"

  VERIFY_GATES_CHANGED="apps/chaos-ui/src/generated/protocol.ts"
  run_case "the generated protocol types count as the GUI gate's" 1 build-gate.sh

  # A filtered run already refuses to read as a sweep (`K of T selected by --only`), so the
  # unbuilt rule is not layered on top of it; pinned here because both shapes are quoted.
  VERIFY_GATES_CHANGED="crates/a.rs"
  run_case "a filtered run is judged by its own disclaimer, not this rule" 0 build-gate.sh \
    --only "cheap neighbour"
  reject_line "which did not skip a build gate, so it says nothing about Rust" "^NOT COVERED"
  VERIFY_GATES_CHANGED=""

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

  run_case "--log-dir without a value is refused, not defaulted" 2 all-pass.sh --log-dir
  expect_line "naming the flag instead of silently picking target/" "needs a directory"

  # The flag last on the command line is caught by the "no second word" half of the guard; an
  # empty value is caught only by the other half, so it needs its own case or that half is dead
  # weight that no mutation can be shown to need.
  run_case "an empty --log-dir value is refused, not parsed" 2 all-pass.sh --log-dir ""
  expect_line "naming the flag instead of taking the empty string as a path" "needs a directory"
  reject_line "and no gate ran on the way to the error" "^PASS  always passes"

  # The reason this runner was changed: cargo puts the failing test names thousands of lines
  # above the `error: N target failed:` line, so the printed tail can only ever carry one of
  # them. Both halves have to be lifted out of the whole output, and the whole output has to
  # survive somewhere a reader can open.
  run_case "a failure whose names are buried above the tail still names them" 1 cargo-shaped.sh
  expect_line "the first target cargo blames" "^      -p chaos-engine --lib$"
  expect_line "and the second target it blames" "^      -p xai-tty-utils --lib$"
  expect_line "the test buried under 300 lines of filler" \
    "^      \[Running unittests src/lib.rs (target/debug/deps/chaos_engine-80b8ed7d12d00ecb)\] tests::buried_by_the_tail$"
  expect_line "and the second name from that same block" \
    "^      \[Running unittests src/lib.rs (target/debug/deps/chaos_engine-80b8ed7d12d00ecb)\] a_module::another_one$"
  expect_line "the later target's test tagged with the later target" \
    "^      \[Running unittests src/lib.rs (target/debug/deps/xai_tty_utils-654c93f9b2abfae2)\] a_later_target_own_test$"
  reject_line "rather than folded onto whichever target came first" \
    "chaos_engine-80b8ed7d12d00ecb\] a_later_target_own_test"
  expect_line "the third target cargo blames" "^      -p chaos-engine --doc$"
  expect_line "a doc-test named the way cargo names it, not dropped" \
    "^      \[Doc-tests chaos_engine\] src/lib.rs - add (line 3)$"
  expect_line "counted, not just trailed" "red, 4 of them, one line each"
  expect_line "pointing at the file holding the whole output" "^    full output of this gate: "
  expect_line "and the summary repeating that directory" "^full gate output: ${work}/gate-logs$"

  # A pointer to a file that turns out to hold another tail is the same loss with extra steps,
  # so the persisted file is opened and a line from its middle is looked for.
  saved_log="$(printf '%s\n' "${LAST_OUT}" \
    | sed -n 's#^    full output of this gate: \([^ ]*\) .*#\1#p' | head -n 1)"
  if [ -n "${saved_log}" ] && [ -f "${saved_log}" ] \
    && grep -q '^test t17 \.\.\. ok$' "${saved_log}"; then
    printf 'ok    the persisted file holds the middle of the output, not another tail\n'
    pass=$((pass + 1))
  else
    printf 'not ok the persisted gate log is missing or truncated (%s)\n' "${saved_log}"
    fail=$((fail + 1))
  fi

  # The names come out of the output's own structure, not out of "some indented line": a guard
  # that prints `failures:` and then indented prose gets no fabricated test list, which is what
  # keeps the identifier rule from being loosened into a line-matcher without anything going red.
  run_case "indented prose under a failures line is not read as a test name" 1 indented-prose.sh
  reject_line "no test names invented for a guard" "one line each, tagged with the target"
  reject_line "and no target names invented either" "cargo blames"
  expect_line "while the finding itself is still printed" "called .Result::unwrap."

  # The same failure with a caller-chosen directory has to land there, or a sweep whose log a
  # reader is asked to keep would keep the runner's own throwaway path instead.
  run_case "--log-dir puts the file where the caller said" 1 cargo-shaped.sh \
    --log-dir "${work}/kept"
  expect_line "naming that path in the failure block" "^    full output of this gate: ${work}/kept/"
  expect_line "and in the summary" "^full gate output: ${work}/kept$"

  # Nothing was asked for when the gate passed, so nothing is claimed. A runner that printed a
  # file pointer for gates it never wrote would train readers to ignore the line.
  run_case "a passing run writes no failure pointer" 0 all-pass.sh
  reject_line "because there was no failure to point at" "full output of this gate"
  reject_line "and invents no names for a gate that had none" "one line each, tagged with the target"

  # The names come out of the output, not out of a guess about what a red gate must have run: a
  # guard that prints prose gets no fabricated test list.
  run_case "a failing guard with no cargo shape gets no invented names" 1 one-fails.sh
  reject_line "no target list" "cargo blames"
  reject_line "no test list" "one line each, tagged with the target"
  expect_line "while still keeping the full output" "^    full output of this gate: "

  # The default directory is part of the contract, since that is what a plain sweep gets.
  LAST_OUT="$(VERIFY_GATES_SOURCE="${work}/cargo-shaped.sh" VERIFY_GATES_LOG_DIR="" \
    bash "${script_path}" 2>&1)"
  default_log="$(printf '%s\n' "${LAST_OUT}" | sed -n 's#^full gate output: \(.*\)$#\1#p' \
    | head -n 1)"
  case "${default_log}" in
    "${repo_root}/target/verify-gates/"*)
      printf 'ok    with no --log-dir the output goes under target/verify-gates\n'
      pass=$((pass + 1))
      ;;
    *)
      printf 'not ok the default gate log went somewhere else (%s)\n' "${default_log}"
      fail=$((fail + 1))
      ;;
  esac
  if [ -n "${default_log}" ]; then
    rm -rf "${default_log}"
  fi

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
    if [ "${include_build}" = "no" ]; then
      echo '   a skipped build gate plus a changed Rust file is a NOT COVERED failure, not a pass'
    fi
    echo "   env: RUST_MIN_STACK=${RUST_MIN_STACK:-unset} (mirrors the container's --env; set it to override)"
    if [ "${only_count}" -gt 0 ]; then
      echo "   --only is in effect (${only_count} pattern(s)); this is a filtered run, not a sweep"
    fi
    run_gates "${gates_source}"
    ;;
esac
