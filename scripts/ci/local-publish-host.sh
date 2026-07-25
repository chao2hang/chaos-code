#!/usr/bin/env bash
# Publish chaos-code for the *current host platform only* — no GitHub Actions,
# no NPM_TOKEN secret. Uses your interactive `npm login` session.
#
# Prerequisites:
#   npm login          # once
#   release binary at target/release/chaos or target/<triple>/release-dist/chaos
#
# Usage (repo root):
#   ./scripts/ci/local-publish-host.sh           # pack dry-run by default
#   ./scripts/ci/local-publish-host.sh --publish # actually npm publish
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
  Linux-x86_64|Linux-amd64)   PLATFORM=linux-x64;   ENV_KEY=CHAOS_LINUX_X64;   BIN_NAME=chaos ;;
  Linux-aarch64|Linux-arm64)  PLATFORM=linux-arm64; ENV_KEY=CHAOS_LINUX_ARM64; BIN_NAME=chaos ;;
  Darwin-arm64)               PLATFORM=darwin-arm64; ENV_KEY=CHAOS_DARWIN_ARM64; BIN_NAME=chaos ;;
  Darwin-x86_64)              PLATFORM=darwin-x64;  ENV_KEY=CHAOS_DARWIN_X64;  BIN_NAME=chaos ;;
  MINGW*|MSYS*|CYGWIN*)
    echo "on Windows use Git Bash / WSL; set CHAOS_WIN32_* and ONLY_HOST manually" >&2
    exit 1
    ;;
  *)
    echo "unsupported host: $(uname -s)-$(uname -m)" >&2
    exit 1
    ;;
esac

# Prefer release-dist, then release.
BIN=""
for cand in \
  "target/release-dist/$BIN_NAME" \
  "target/release/$BIN_NAME" \
  "target/$(rustc -vV 2>/dev/null | awk '/host:/{print $2}')/release-dist/$BIN_NAME" \
  "target/$(rustc -vV 2>/dev/null | awk '/host:/{print $2}')/release/$BIN_NAME"
do
  if [[ -f "$cand" ]]; then
    BIN="$cand"
    break
  fi
done

if [[ -z "$BIN" ]]; then
  echo "no binary found. Build first:" >&2
  echo "  cargo build -p xai-grok-pager-bin --release" >&2
  echo "  # or: cargo build -p xai-grok-pager-bin --profile release-dist" >&2
  exit 1
fi

if [[ -n "$VERSION" ]]; then
  node scripts/ci/stamp-npm-version.mjs "$VERSION"
fi

echo "host platform: $PLATFORM"
echo "binary:        $BIN ($(du -h "$BIN" | awk '{print $1}'))"
export "$ENV_KEY=$BIN"
export ONLY_HOST=1
node crates/codegen/xai-grok-pager/npm/chaos/scripts/assemble-platform-packages.js

NPM_ROOT="$ROOT/crates/codegen/xai-grok-pager/npm"
PLAT_DIR="$NPM_ROOT/chaos-$PLATFORM"
META_DIR="$NPM_ROOT/chaos"

if [[ "$PUBLISH" -eq 0 ]]; then
  echo ""
  echo "== dry-run (pass --publish to actually release) =="
  (cd "$PLAT_DIR" && npm pack --dry-run)
  (cd "$META_DIR" && npm pack --dry-run)
  echo ""
  echo "When ready:"
  echo "  1. npm whoami          # must be logged in"
  echo "  2. $0 --publish"
  exit 0
fi

if ! npm whoami >/dev/null 2>&1; then
  echo "not logged in to npm. Run: npm login" >&2
  exit 1
fi

echo "==> publish chaos-code-$PLATFORM"
(cd "$PLAT_DIR" && npm publish --access public)

echo "==> publish chaos-code (meta)"
(cd "$META_DIR" && npm publish --access public)

echo ""
echo "done. Users on $PLATFORM can:"
echo "  npm i -g chaos-code"
echo "Other platforms need their platform packages published (CI later, or another host)."
