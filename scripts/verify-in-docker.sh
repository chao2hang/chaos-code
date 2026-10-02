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
      sed -n '2,20p' "$0"
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
echo "== preparing cargo cache volumes"
docker volume create chaos-verify-cargo-registry >/dev/null
docker volume create chaos-verify-target >/dev/null

run_args=(
  --rm
  --init
  --workdir /src
  --volume "${repo_root}:/src"
  --volume chaos-verify-cargo-registry:/usr/local/cargo/registry
  --volume chaos-verify-target:/src/target
)

if [ "${MODE}" = "shell" ]; then
  exec docker run -it "${run_args[@]}" "${IMAGE_TAG}" bash
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

echo
if [ -n "${failed}" ]; then
  echo "FAILED gates:${failed}"
  exit 1
fi
echo "all gates passed in ${IMAGE_TAG}"
