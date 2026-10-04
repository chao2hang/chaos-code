#!/usr/bin/env bash
# Run the documented Linux gate sequence inside a clean container.
#
# A developer machine is not evidence that the documented steps work: a warm
# target/ directory, a globally installed protoc, a manually installed toolchain
# or an OS trust store that already has the right roots can all hide a step that
# a fresh clone would need. This builds docker/verify.Dockerfile and runs the
# same command list as the `rust` job in .github/workflows/ci.yml there.
#
# Usage:
#   scripts/verify-in-docker.sh              # quick gates (fmt, guards, check, clippy)
#   scripts/verify-in-docker.sh --full       # quick gates plus cargo test --workspace
#   scripts/verify-in-docker.sh --only <label>  # only gates whose label matches (repeatable)
#   scripts/verify-in-docker.sh --shell      # interactive shell in the same image
#
# --only exists because a full sweep costs ~25 minutes here while a change usually needs one
# gate re-checked. Labels come from `scripts/verify-gates.sh --list`; a pattern matches a label
# exactly or as a fragment. A pattern that matches no label is an error rather than a green
# run, and a filtered run prints `K of M gates` in its verdict so it cannot be quoted back as a
# full sweep. cargo test and GUI protocol types are appended by --full and so are named only
# by a run that passes it.
#
# Environment:
#   BASE_IMAGE   base image for docker/verify.Dockerfile (default rust:1-bookworm)
#   IMAGE_TAG    image tag to build and run (default chaos-verify:local)
#
# The source tree is checksummed before the first gate and again after the last
# one, and a mismatch is reported as UNATTRIBUTABLE rather than as a result: the
# container reads the live working tree, so a run that overlapped an edit says
# nothing about any commit. It is said ahead of the gate verdict, because the
# movement is usually the explanation for whatever failed. Re-run it with nothing
# writing to the tree.
#
# Capture evidence with:
#   scripts/verify-in-docker.sh --full 2>&1 | tee verify-in-docker-$(date +%Y%m%d).log
set -euo pipefail

BASE_IMAGE="${BASE_IMAGE:-docker.io/library/debian:bookworm-slim}"
IMAGE_TAG="${IMAGE_TAG:-chaos-verify:local}"
MODE="quick"
ONLY=()

while [ $# -gt 0 ]; do
  case "$1" in
    --full) MODE="full" ;;
    --shell) MODE="shell" ;;
    --only)
      if [ $# -lt 2 ] || [ -z "$1" ]; then
        # An empty pattern matches every label, which would make `--only ""` a full sweep.
        echo "--only needs a gate label (list them with: scripts/verify-gates.sh --list)" >&2
        exit 2
      fi
      shift
      ONLY+=("$1")
      ;;
    -h | --help)
      # The header up to the first non-comment line, so --help cannot drift as lines are
      # added above it.
      awk 'NR > 1 { if ($0 !~ /^#/) exit; print }' "$0"
      exit 0
      ;;
    *)
      echo "unknown argument: $1 (expected --full, --only <label>, --shell or --help)" >&2
      exit 2
      ;;
  esac
  shift
done

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
image_args=(--build-arg "BASE_IMAGE=${BASE_IMAGE}")

echo "== building ${IMAGE_TAG} from ${BASE_IMAGE}"
docker build "${image_args[@]}" -f "${repo_root}/docker/verify.Dockerfile" \
  -t "${IMAGE_TAG}" "${repo_root}"

# Separate named volumes, not a bind mount: cargo writes ~40 GB of build output
# and a bind-mounted target/ owned by root would break the host build afterwards.
# `git` belongs alongside `registry` in that cache: Cargo.lock pins two git
# dependencies (async-openai from our-forks, nucleo), and with only `registry` mounted
# every run re-clones both, which is what made `cargo check` sit for ~9 minutes per
# repository behind a `spurious network error` retry line.
echo "== preparing cargo cache volumes"
docker volume create chaos-verify-cargo-registry >/dev/null
docker volume create chaos-verify-cargo-git >/dev/null
docker volume create chaos-verify-target >/dev/null
# The runtime creates a mount point the image does not already have, and it creates it as
# root inside whatever is mounted at its parent -- so `--volume chaos-verify-target:/src/target`
# alone leaves a root-owned `target/` in the checkout even though nothing is ever written
# through it. One gate run on 2026-10-04 did exactly that. Created here first, the directory
# belongs to whoever ran the gates, and the run then leaves no root-owned path at all.
mkdir -p "${repo_root}/target"

run_args=(
  --rm
  --init
  --workdir /src
  --volume "${repo_root}:/src"
  --volume chaos-verify-cargo-registry:/usr/local/cargo/registry
  --volume chaos-verify-cargo-git:/usr/local/cargo/git
  --volume chaos-verify-target:/src/target
  # The test step in both CI jobs sets this, and this script's whole claim is that it
  # runs "the same command list as the `rust` job". `xai-grok-shell`'s library suite has
  # current-thread actor tests that overflow the harness default stack -- documented in
  # docs/architecture/todo-open-item-classification.md, where it cost CI run 36165469964.
  # Without this the `--full` mode fails on a clean machine for a reason that has nothing
  # to do with the change under test.
  --env RUST_MIN_STACK=16777216
  # The image sets this as well; it is repeated here so an IMAGE_TAG built elsewhere
  # cannot reintroduce the leak. The guards in scripts/ci import each other through the
  # /src bind mount, and a __pycache__ directory the container creates there belongs to
  # root and survives the developer's own `rm -rf`.
  --env PYTHONDONTWRITEBYTECODE=1
  # Git ownership, applied to *every* gate rather than per command line. The repo
  # is bind-mounted from a host uid, so git inside the container refuses to read
  # it ("detected dubious ownership in repository at '/src'") until told otherwise.
  # Gates that need git carry a `bootstrap` prefix, and `docs localization` did not
  # -- so `scripts/l10n-guard.sh` ran its listing against a git that could not open
  # the repository. In the 2026-10-03 `--full` run that gate reported FAIL with
  # nothing printed after "report dir", which is what a broken instrument looks
  # like, not what a verdict looks like. Trust is a property of this container, not
  # of one command in it, so it is set here where no gate can forget it.
  --env GIT_CONFIG_COUNT=1
  --env GIT_CONFIG_KEY_0=safe.directory
  --env GIT_CONFIG_VALUE_0=/src
)

if [ "${MODE}" = "shell" ]; then
  exec docker run -it "${run_args[@]}" "${IMAGE_TAG}" bash
fi

# Checksums of every file the gates can read, taken before and after the run.
#
# This is a bind mount of a live working tree, so a gate can read a file while an
# editor still has it open. That is not hypothetical: a `--full` run on 2026-10-02
# failed its `cargo test` leg on a rustdoc error naming a module that was being
# edited at that moment, every other gate was green, and the failure could not be
# attributed to any commit. Contents are checksummed rather than mtimes because an
# editor can restore an mtime, and untracked files count because a new module is
# exactly the kind of file a run races with.
tree_dir="$(mktemp -d)"
trap 'rm -rf "${tree_dir}"' EXIT
tree_before="${tree_dir}/before.sums"
tree_after="${tree_dir}/after.sums"

fingerprint() { # fingerprint <output-file>
  ( cd "${repo_root}" && git ls-files -co --exclude-standard -z | xargs -0 cksum ) >"$1"
}

sum_of() { cksum <"$1" | cut -d' ' -f1; }

fingerprint "${tree_before}"
echo "== source tree: $(grep -c '' "${tree_before}") files, checksum $(sum_of "${tree_before}")"
# `grep -c` over `wc -l`: `wc` pads its count on some BSDs, which would make a
# clean tree compare unequal to `0` below.
dirty="$(cd "${repo_root}" && git status --porcelain 2>/dev/null | grep -c '' || true)"
if [ -n "${dirty}" ] && [ "${dirty}" != "0" ]; then
  echo "   ${dirty} path(s) differ from HEAD, so this run describes the working tree, not a commit"
fi

# The repo is bind-mounted from a host user, so git inside the container sees a
# foreign owner; tests that shell out to git need it trusted first.
bootstrap='git config --global --add safe.directory /src
  git config --global user.email verify@example.invalid
  git config --global user.name chaos-verify'

# One step per line so a failure names the gate that failed instead of hiding
# inside a chain. `--no-fail-fast` is intentional for the test step only: a
# single flaky test should not hide the rest of the workspace results.
gates=(
  "toolchain matches the pin: rustc -V && cargo -V"
  "pinned protoc launcher: ${bootstrap}; bin/protoc --version"
  "cargo fmt: ${bootstrap}; cargo fmt --all -- --check"
  "ignored-test baseline: ${bootstrap}; python3 scripts/ci/test-ignored-tests.py && python3 scripts/ci/test-ignored-tests-baseline-fixture.py && python3 scripts/ci/test-ignored-tests-baseline.py && python3 scripts/ci/test-ignored-tests-reasons.py && python3 scripts/ci/ignored-tests.py --require-reasons && python3 scripts/ci/ignored-tests.py --check-baseline scripts/ci/ignored-tests-baseline.tsv"
  "brand/protocol guard: python3 scripts/ci/check-brand-protocol.py && python3 scripts/ci/test-brand-protocol.py"
  "protocol mirror coverage: python3 scripts/ci/test-check-protocol-mirror.py && python3 scripts/ci/check-protocol-mirror.py"
  "TODO status doc: python3 scripts/ci/test-classify-open-todos.py && python3 scripts/ci/classify-open-todos.py --check-doc docs/architecture/todo-open-item-classification.md && python3 scripts/ci/classify-open-todos.py --check-export"
  "evidence paths in docs: python3 scripts/ci/test-check-evidence-paths.py && python3 scripts/ci/check-evidence-paths.py"
  "docs path references: python3 scripts/ci/test-check-doc-path-refs.py && python3 scripts/ci/check-doc-path-refs.py"
  "documented commands: python3 scripts/ci/test-check-evidence-commands.py && python3 scripts/ci/check-evidence-commands.py"
  "CI guard wiring: python3 scripts/ci/test-check-guard-wiring.py && python3 scripts/ci/check-guard-wiring.py"
  # The host gate runner reads the array below, so it is checked from inside it: the
  # fixture cases of scripts/verify-gates.sh --self-test include parsing this real file,
  # which fails here if the entry format changes and no local runner notices.
  "host gate runner self-test: bash scripts/verify-gates.sh --self-test"
  # No argument on purpose: the CI step runs it with none, and pointing this one at
  # ci.yml alone left release.yml -- the workflow with the Windows matrix, which is
  # the whole reason the check exists -- unexamined locally.
  "workflow shells: python3 scripts/ci/check-workflow-shells.py"
  "workflow toolchain: python3 scripts/ci/test-check-workflow-toolchain.py && python3 scripts/ci/check-workflow-toolchain.py"
  "workflow yaml: python3 scripts/ci/test-check-workflow-yaml.py && python3 scripts/ci/check-workflow-yaml.py"
  "script portability: python3 scripts/ci/check-script-portability.py && python3 scripts/ci/test-script-portability.py"
  # The complement of portability: a shell script that runs everywhere but stops early. A plain
  # `name="$(pipeline)"` assignment under `set -e` inherits the status, so a `grep` that matched
  # nothing or a `diff` that found a difference ends the script before its own report. Three sites
  # shipped that way on 2026-10-04, all three with the correct exit code and no words. Out of scope
  # are the `set -uo pipefail` scripts, this file's own host runner among them.
  "pipefail report: python3 scripts/ci/test-check-pipefail-report.py && python3 scripts/ci/check-pipefail-report.py"
  # This very entry point runs its gates as root with the working tree bind-mounted, so anything
  # the container writes lands in the checkout owned by root. One gate run left a bytecode cache
  # there on 2026-10-04, a `--full` run left two, and a cache directory the container had to create
  # could not be deleted by the owner of the tree afterwards. The rule judges who mounts the
  # checkout rather than who is named like a container entry point.
  "container hygiene: python3 scripts/ci/test-check-container-hygiene.py && python3 scripts/ci/check-container-hygiene.py"
  # The runtime half of the same rule. The check above reads shell text and cannot see a mount path
  # assembled at runtime; this one walks the tree and asks who owns each path, comparing against the
  # owner of the checkout itself. Running it here, inside the container, means the leak is measured
  # at the moment it exists rather than in a review of the script that caused it.
  "tree ownership: python3 scripts/ci/test-check-tree-ownership.py && python3 scripts/ci/check-tree-ownership.py"
  "panic-site census: python3 scripts/ci/test-panic-site-census.py && python3 scripts/ci/panic-site-census.py --check-baseline scripts/ci/panic-site-baseline.tsv && python3 scripts/ci/panic-site-census.py --check-uncompiled scripts/ci/uncompiled-sources.txt"
  # The census cannot see a Prometheus call whose label count is wrong, because the panic
  # lives in the dependency (`with_label_values` unwraps) and no token at the call site says
  # so. This pairs each label-value call with the labels its metric registration declares.
  "metric labels: python3 scripts/ci/test-check-metric-labels.py && python3 scripts/ci/check-metric-labels.py"
  # tokio spawns the child inside `.output()` itself and leaves it running after the future is
  # dropped, so `timeout(budget, cmd.output())` cancels the waiting and not the work. The clippy
  # spawn ban cannot cover it: `ProcessScope::enroll` needs a `&Child` that call never returns.
  "timeout children: python3 scripts/ci/test-check-timeout-child.py && python3 scripts/ci/check-timeout-child.py"
  "cwd-change census: python3 scripts/ci/test-cwd-change-census.py && python3 scripts/ci/cwd-change-census.py --check-baseline scripts/ci/cwd-change-baseline.tsv"
  "spawn-cwd portability: python3 scripts/ci/test-check-spawn-cwd-portability.py && python3 scripts/ci/check-spawn-cwd-portability.py"
  # A `#[cfg(unix)]` on a test is not a skip: the test stops existing on the other platform, so it
  # appears in no ignored baseline and in no green leg's count. The four budgets are the measured
  # debt of 2026-10-03 and are pinned here as well as in ci.yml; only they can move down.
  "platform-gated tests: python3 scripts/ci/test-platform-gated-tests.py && python3 scripts/ci/platform-gated-tests.py --quiet --check-baseline scripts/ci/platform-gated-tests.tsv --max-unreviewed 1106 --max-blind-windows 74 --max-blind-macos 11 --max-assumption-free 427"
  # The `rustup target add` is load-bearing: the guard fails instead of skipping
  # when a target named in the table is unavailable, and an image without the
  # tier-2 targets installed would otherwise turn it into a Linux-only no-op.
  "load-bearing features: ${bootstrap}; rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin && python3 scripts/ci/test-check-load-bearing-features.py && python3 scripts/ci/check-load-bearing-features.py"
  "version lockstep: python3 scripts/ci/test-check-versions.py && bash scripts/ci/check-versions.sh && python3 scripts/ci/check-version-lockstep.py"
  # The recon record is named by date + upstream tip, so a second same-day run used to
  # overwrite the first; these fixtures drive the shipped script and pin that it cannot.
  "upstream recon: python3 scripts/ci/test-upstream-recon.py"
  "installer guards: python3 scripts/ci/test-installer-asset-names.py && python3 scripts/ci/test-installer-bash-resolution.py && python3 scripts/ci/test-installer-signature-policy.py && python3 scripts/ci/test-installer-download-size.py"
  "npm package guards: node --check crates/codegen/xai-grok-pager/npm/chaos/scripts/assemble-platform-packages.js && node --check crates/codegen/xai-grok-pager/npm/chaos/bin/postinstall.js && node --check crates/codegen/xai-grok-pager/npm/chaos/bin/chaos && bash scripts/ci/test-publish-npm.sh"
  "docs localization: ${bootstrap}; bash scripts/l10n-guard.sh && python3 scripts/check-doc-l10n.py --links && python3 scripts/check-doc-l10n.py --english"
  "localization guard self-tests: python3 scripts/l10n-guard-selftest.py && python3 scripts/check-doc-l10n-selftest.py"
  "secret scan: ${bootstrap}; bash scripts/ci/secret-scan.sh"
  "cargo check: cargo check --workspace --all-targets --locked"
  "cargo clippy: cargo clippy --workspace --all-targets --locked -- -D warnings"
)

if [ "${MODE}" = "full" ]; then
  gates+=("cargo test: ${bootstrap}; cargo test --workspace --locked --no-fail-fast")
  # Deliberately full-only, and deliberately after `cargo test`: the check works by
  # `cargo run --bin chaos-protocol-schema`, so in this mode the binary is already
  # built and the gate costs a comparison. In quick mode it would be a dev build of
  # chaos-engine added to a run that otherwise stops at check/clippy, which is the
  # whole reason quick mode is quick.
  gates+=("GUI protocol types: ${bootstrap}; bash scripts/ci/check-gui-protocol.sh")
fi

# --only filters the array; it never reorders it and never adds to it. The check runs before
# the first gate, so a pattern naming no label is refused instead of coming back as a run that
# selected nothing and printed a verdict about nothing.
total_gates="${#gates[@]}"
if [ "${#ONLY[@]}" -gt 0 ]; then
  for pattern in "${ONLY[@]}"; do
    hits=0
    for gate in "${gates[@]}"; do
      case "${gate%%: *}" in
        "${pattern}" | *"${pattern}"*) hits=$((hits + 1)) ;;
      esac
    done
    if [ "${hits}" -eq 0 ]; then
      echo "--only ${pattern}: no gate label matches it." >&2
      echo '           labels are listed by: scripts/verify-gates.sh --list' >&2
      echo '           (cargo test and GUI protocol types are appended by --full)' >&2
      exit 2
    fi
  done
  selected=()
  for gate in "${gates[@]}"; do
    for pattern in "${ONLY[@]}"; do
      case "${gate%%: *}" in
        "${pattern}" | *"${pattern}"*)
          selected+=("${gate}")
          break
          ;;
      esac
    done
  done
  gates=("${selected[@]}")
  echo "== --only is in effect: ${#gates[@]} of ${total_gates} gates selected, this is not a sweep"
fi

# Preflight: the instrument before the measurements. Several gates read the repo
# through git (`l10n-guard.sh`, `check-doc-l10n.py`, the secret scan, `cargo`
# itself for git dependencies). If the container cannot open the bind-mounted
# repository, no gate verdict below means anything, so stop here rather than
# collect a dozen failures whose only cause is one config line -- and say what to
# fix, because the failure otherwise surfaces as a guard that prints nothing.
echo
echo "== preflight: container can read the repository"
if ! docker run "${run_args[@]}" "${IMAGE_TAG}" \
  bash -c 'set -e; git -C /src rev-parse --short HEAD >/dev/null; git -C /src status --porcelain >/dev/null'; then
  echo "preflight: FAIL -- git inside the container cannot read /src." >&2
  echo "           Check the bind mount of ${repo_root} and the GIT_CONFIG_* safe.directory" >&2
  echo "           entry in this script; a gate that reads git cannot pass without it." >&2
  exit 1
fi
echo "preflight: OK"

failed=""
for gate in "${gates[@]}"; do
  label="${gate%%: *}"
  command_line="${gate#*: }"
  echo
  echo "== ${label}"
  if ! docker run "${run_args[@]}" "${IMAGE_TAG}" bash -c "${command_line}"; then
    failed="${failed} ${label}"
  fi
done

# The verdict is only about the tree the gates actually saw, so this is taken
# before any verdict is printed. Two reasons it comes first: a run that overlapped
# an edit explains its own failures, and a reader who met `FAILED gates` alone would
# blame the code; and the second checksum can itself fail while the tree is being
# rewritten underneath it (a path renamed mid-`cksum`), which must read as
# "unattributable", not as a silent death after the gate summary.
fingerprint_ok=yes
fingerprint "${tree_after}" || fingerprint_ok=no

moved=""
if [ "${fingerprint_ok}" != "yes" ]; then
  moved="(the tree could not be checksummed: a path appeared, disappeared or was renamed mid-run)"
elif ! cmp -s "${tree_before}" "${tree_after}"; then
  # `diff` exits 1 when the two files differ, which is precisely the case this branch exists
  # for, and under `set -euo pipefail` that status propagates out of the assignment and ends
  # the script. Measured on 2026-10-04: a gate that passed, a tree that moved mid-run, exit 1,
  # and no verdict printed at all -- the report this function exists to give was the thing
  # that never ran. The listing is what is wanted here, not the exit status.
  moved="$(diff "${tree_before}" "${tree_after}" | sed -n 's/^[<>] [0-9][0-9]* [0-9][0-9]* //p' | sort -u || true)"
fi

echo
if [ -n "${failed}" ]; then
  echo "FAILED gates:${failed}"
elif [ "${#gates[@]}" -ne "${total_gates}" ]; then
  # The unfiltered wording is reserved for an unfiltered run.
  echo "selected gates passed in ${IMAGE_TAG}"
else
  echo "all gates passed in ${IMAGE_TAG}"
fi
if [ "${#gates[@]}" -ne "${total_gates}" ]; then
  echo "  --only was in effect: ${#gates[@]} of ${total_gates} gates ran, so this is not a full sweep"
fi

if [ -n "${moved}" ]; then
  echo
  echo "UNATTRIBUTABLE: the source tree changed while the gates ran."
  echo "  checksum $(sum_of "${tree_before}") -> $(sum_of "${tree_after}"); these paths differ:"
  echo "${moved}" | sed -n '1,20p' | sed 's/^/    /'
  echo "  nothing in this run can be attributed to a commit; re-run it with the tree at rest."
  exit 1
fi

if [ -n "${failed}" ]; then
  exit 1
fi
