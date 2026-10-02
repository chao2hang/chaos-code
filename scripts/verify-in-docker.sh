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
#   scripts/verify-in-docker.sh --shell      # interactive shell in the same image
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

for arg in "$@"; do
  case "$arg" in
    --full) MODE="full" ;;
    --shell) MODE="shell" ;;
    -h | --help)
      sed -n '2,25p' "$0"
      exit 0
      ;;
    *)
      echo "unknown argument: $arg (expected --full, --shell or --help)" >&2
      exit 2
      ;;
  esac
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
  "brand/protocol guard: python3 scripts/ci/check-brand-protocol.py"
  "workflow shells: python3 scripts/ci/check-workflow-shells.py .github/workflows/ci.yml"
  "script portability: python3 scripts/ci/check-script-portability.py"
  "docs localization: bash scripts/l10n-guard.sh && python3 scripts/check-doc-l10n.py --links && python3 scripts/check-doc-l10n.py --english"
  "localization guard self-tests: python3 scripts/l10n-guard-selftest.py && python3 scripts/check-doc-l10n-selftest.py"
  "secret scan: ${bootstrap}; bash scripts/ci/secret-scan.sh"
  "cargo check: cargo check --workspace --all-targets --locked"
  "cargo clippy: cargo clippy --workspace --all-targets --locked -- -D warnings"
)

if [ "${MODE}" = "full" ]; then
  gates+=("cargo test: ${bootstrap}; cargo test --workspace --locked --no-fail-fast")
fi

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
  moved="$(diff "${tree_before}" "${tree_after}" | sed -n 's/^[<>] [0-9][0-9]* [0-9][0-9]* //p' | sort -u)"
fi

echo
if [ -n "${failed}" ]; then
  echo "FAILED gates:${failed}"
else
  echo "all gates passed in ${IMAGE_TAG}"
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
