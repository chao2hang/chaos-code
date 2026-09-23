#!/usr/bin/env bash
# Publish chaos-code packages locally — no GitHub Actions or NPM_TOKEN secret.
# Full publication requires six binaries; partial mode only publishes the host package.
#
# Prerequisites:
#   npm login          # once
#   all six release binaries for full publication, supplied via CHAOS_* variables
#   host release binary only when PUBLISH_NPM_ALLOW_PARTIAL=1
#
# Usage (repo root):
#   ./scripts/ci/local-publish-host.sh           # assemble + pack dry-run
#   ./scripts/ci/local-publish-host.sh --publish # publish all packages after dry-run
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

PUBLISH=0
VERSION=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --publish) PUBLISH=1; shift ;;
    --version) VERSION="${2:?}"; shift 2 ;;
    -h|--help)
      sed -n '2,12p' "$0"
      exit 0
      ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64|Linux-amd64)   PLATFORM=linux-x64 ;;
  Linux-aarch64|Linux-arm64)  PLATFORM=linux-arm64 ;;
  Darwin-arm64)               PLATFORM=darwin-arm64 ;;
  Darwin-x86_64)              PLATFORM=darwin-x64 ;;
  MINGW*|MSYS*|CYGWIN*)       PLATFORM=win32-x64 ;;
  *)
    echo "unsupported host: $(uname -s)-$(uname -m)" >&2
    exit 1
    ;;
esac

# A single-host build is not sufficient: the meta package pins all six platform
# packages, and publishing it without those packages makes installs unreliable.
PLATFORMS=(darwin-arm64 darwin-x64 linux-arm64 linux-x64 win32-arm64 win32-x64)
REQUIRED_BINARIES=("${PLATFORMS[@]}")
if [[ "${PUBLISH_NPM_ALLOW_PARTIAL:-0}" == "1" ]]; then
  export PUBLISH_EXISTING_ONLY=1
  REQUIRED_BINARIES=("$PLATFORM")
  echo "warning: partial publish enabled; only the host platform package will be assembled; the meta package will not be published" >&2
fi
for platform in "${REQUIRED_BINARIES[@]}"; do
  key="CHAOS_${platform^^}"
  key="${key//-/_}"
  if [[ -z "${!key:-}" || ! -f "${!key}" ]]; then
    echo "missing $key: provide the binary before publishing" >&2
    exit 1
  fi
done

if [[ -n "$VERSION" ]]; then
  node scripts/ci/stamp-npm-version.mjs "$VERSION"
fi

echo "host platform: $PLATFORM"
for platform in "${REQUIRED_BINARIES[@]}"; do
  key="CHAOS_${platform^^}"
  key="${key//-/_}"
  printf '%-24s %s (%s)\n' "$key" "${!key}" "$(du -h "${!key}" | awk '{print $1}')"
  export "$key"
done
export ONLY_PLATFORMS="${REQUIRED_BINARIES[*]}"
node crates/codegen/xai-grok-pager/npm/chaos/scripts/assemble-platform-packages.js
if [[ "${PUBLISH_NPM_ALLOW_PARTIAL:-0}" == "1" ]]; then
  export ONLY_PLATFORMS="$PLATFORM"
fi

if [[ "$PUBLISH" -eq 0 ]]; then
  echo ""
  echo "== dry-run (pass --publish to actually release) =="
  DRY_RUN=1 NPM_TOKEN=dummy PUBLISH_EXISTING_ONLY=1 PUBLISH_NPM_ALLOW_PARTIAL="${PUBLISH_NPM_ALLOW_PARTIAL:-0}" bash scripts/ci/publish-npm.sh
  echo ""
  echo "When ready:"
  echo "  1. npm whoami          # must be logged in"
  if [[ "${PUBLISH_NPM_ALLOW_PARTIAL:-0}" == "1" ]]; then
    echo "  2. PUBLISH_NPM_ALLOW_PARTIAL=1 $0 --publish # platform only; no meta package"
  else
    echo "  2. $0 --publish       # all six platforms + meta package"
  fi
  exit 0
fi

if ! npm whoami >/dev/null 2>&1; then
  echo "not logged in to npm. Run: npm login" >&2
  exit 1
fi

echo "==> publish the selected platform packages"
PUBLISH_EXISTING_ONLY=1 PUBLISH_NPM_ALLOW_PARTIAL="${PUBLISH_NPM_ALLOW_PARTIAL:-0}" bash scripts/ci/publish-npm.sh

echo ""
if [[ "${PUBLISH_NPM_ALLOW_PARTIAL:-0}" == "1" ]]; then
  echo "done. Published $PLATFORM only; the meta package was not published."
else
  echo "done. Published all six platform packages and the meta package."
fi
