#!/usr/bin/env bash
# Publish platform packages first, then the meta package chaos-code.
# Expects assemble-platform-packages.js to have already written *.br binaries.
#
# Env:
#   NPM_TOKEN / NODE_AUTH_TOKEN  — required unless DRY_RUN=1
#   DRY_RUN=1                    — npm pack --dry-run only
#   PUBLISH_EXISTING_ONLY=1      — skip platforms with empty bin/ (partial CI matrix)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
NPM_ROOT="$ROOT/crates/codegen/xai-grok-pager/npm"
DRY_RUN="${DRY_RUN:-0}"
EXISTING_ONLY="${PUBLISH_EXISTING_ONLY:-0}"

if [[ "$DRY_RUN" != "1" && -z "${NODE_AUTH_TOKEN:-}${NPM_TOKEN:-}" ]]; then
  echo "error: set NPM_TOKEN or NODE_AUTH_TOKEN for npm publish" >&2
  exit 1
fi

export NODE_AUTH_TOKEN="${NODE_AUTH_TOKEN:-${NPM_TOKEN:-}}"

has_bin() {
  local dir="$1/bin"
  [[ -d "$dir" ]] || return 1
  # any non-hidden file under bin/
  compgen -G "${dir}/*" > /dev/null
}

publish_one() {
  local dir="$1"
  echo "==> publishing $(basename "$dir")"
  if [[ "$DRY_RUN" == "1" ]]; then
    (cd "$dir" && npm pack --dry-run)
  else
    (cd "$dir" && npm publish --access public --provenance=false)
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
      echo "skip $p (no bin/ — not assembled)"
      continue
    fi
    echo "error: $p has no bin/ contents — run assemble first" >&2
    exit 1
  fi
  publish_one "$pkg"
  PUBLISHED=$((PUBLISHED + 1))
done

if [[ "$PUBLISHED" -eq 0 ]]; then
  echo "error: no platform packages to publish" >&2
  exit 1
fi

# Meta package optionalDependencies list all six; publishing is fine even if
# some platforms were not built this run — users on missing platforms simply
# won't get a binary until that platform package is published later.
publish_one "$NPM_ROOT/chaos"
echo "publish complete ($PUBLISHED platform package(s) + meta)"
