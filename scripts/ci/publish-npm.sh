#!/usr/bin/env bash
# Publish platform packages first, then the meta package chaos-code.
# Expects assemble-platform-packages.js to have already written *.br binaries.
#
# Env:
#   NPM_TOKEN / NODE_AUTH_TOKEN  — required unless DRY_RUN=1
#   DRY_RUN=1                    — npm pack --dry-run only
#   PUBLISH_EXISTING_ONLY=1      — skip platforms without binaries; meta publish still requires all six
#   PUBLISH_NPM_ALLOW_PARTIAL=1   — explicitly publish available platform packages only, never the meta package
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
NPM_ROOT="${NPM_ROOT:-$ROOT/crates/codegen/xai-grok-pager/npm}"
DRY_RUN="${DRY_RUN:-0}"
EXISTING_ONLY="${PUBLISH_EXISTING_ONLY:-0}"
NPM_VERIFY_RETRIES="${NPM_VERIFY_RETRIES:-12}"
PUBLISH_PLATFORMS=0
if [[ "${PUBLISH_NPM_ALLOW_PARTIAL:-0}" == "1" ]]; then
  PUBLISH_PLATFORMS=1
fi
if [[ "$DRY_RUN" == "1" && "$PUBLISH_PLATFORMS" != "1" && "$EXISTING_ONLY" != "1" ]]; then
  echo "error: DRY_RUN requires assembled packages; set PUBLISH_EXISTING_ONLY=1 to validate selected artifacts" >&2
  exit 1
fi

if [[ "$DRY_RUN" != "1" && -z "${NODE_AUTH_TOKEN:-}${NPM_TOKEN:-}" ]]; then
  echo "error: set NPM_TOKEN or NODE_AUTH_TOKEN for npm publish" >&2
  exit 1
fi

export NODE_AUTH_TOKEN="${NODE_AUTH_TOKEN:-${NPM_TOKEN:-}}"

has_bin() {
  local dir="$1/bin"
  [[ -d "$dir" ]] || return 1
  # Require a non-empty binary archive, not the tracked .gitkeep placeholder.
  local archive
  for archive in "$dir"/*.br "$dir"/*.exe; do
    [[ -f "$archive" && -s "$archive" ]] && return 0
  done
  return 1
}

publish_one() {
  local dir="$1"
  echo "==> publishing $(basename "$dir")"
  if [[ "$DRY_RUN" == "1" ]]; then
    (cd "$dir" && npm pack --dry-run)
  else
    (cd "$dir" && npm publish --access public --provenance=false)
    local package_name package_version published_version
    package_name="$(node -p 'require(process.argv[1]).name' "$dir/package.json")"
    package_version="$(node -p 'require(process.argv[1]).version' "$dir/package.json")"
    local attempt=0
    while (( attempt < NPM_VERIFY_RETRIES )); do
      published_version="$(npm view "$package_name@$package_version" version --json 2>/dev/null || true)"
      if [[ "$published_version" == "\"$package_version\"" || "$published_version" == "$package_version" ]]; then
        echo "verified $package_name@$package_version on npmjs"
        return 0
      fi
      attempt=$((attempt + 1))
      if (( attempt < NPM_VERIFY_RETRIES )); then sleep 5; fi
    done
    echo "error: registry did not confirm $package_name@$package_version after $((NPM_VERIFY_RETRIES * 5)) seconds (reported: ${published_version:-empty})" >&2
    return 1
  fi
}

PUBLISHED=0
for p in \
  chaos-darwin-arm64 \
  chaos-darwin-x64 \
  chaos-linux-arm64 \
  chaos-linux-x64 \
  chaos-win32-arm64 \
  chaos-win32-x64
do
  pkg="$NPM_ROOT/$p"
  if ! has_bin "$pkg"; then
    if [[ "$EXISTING_ONLY" == "1" ]]; then
      echo "skip $p (no binary archive — not assembled)"
      continue
    fi
    echo "error: $p has no binary archive — run assemble first" >&2
    exit 1
  fi
  publish_one "$pkg"
  PUBLISHED=$((PUBLISHED + 1))
done

if [[ "$PUBLISHED" -eq 0 ]]; then
  echo "error: no platform packages to publish" >&2
  exit 1
fi

# The meta package pins all six platform packages. Publishing it when any
# platform is absent can make clean installs fail due to an unresolvable pin.
EXPECTED_PLATFORMS=6
if [[ "$PUBLISHED" -ne "$EXPECTED_PLATFORMS" ]]; then
  if [[ "$PUBLISH_PLATFORMS" == "1" ]]; then
    echo "warning: partial publication enabled; publishing platform packages only (no meta package)"
    exit 0
  fi
  echo "error: assembled $PUBLISHED of $EXPECTED_PLATFORMS platform packages; refusing to publish the meta package" >&2
  exit 1
fi
publish_one "$NPM_ROOT/chaos"
echo "publish complete ($PUBLISHED platform package(s) + meta)"
