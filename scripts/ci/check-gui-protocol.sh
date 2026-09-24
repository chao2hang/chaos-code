#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
EXPECTED="$ROOT/apps/chaos-ui/src/generated/protocol.ts"
ACTUAL="$(mktemp)"
trap 'rm -f "$ACTUAL"' EXIT
cargo run --quiet --locked -p chaos-engine --bin chaos-protocol-schema > "$ACTUAL"
if ! cmp -s "$EXPECTED" "$ACTUAL"; then
  echo "GUI protocol TypeScript is stale. Regenerate with:" >&2
  echo "  cargo run -p chaos-engine --bin chaos-protocol-schema > apps/chaos-ui/src/generated/protocol.ts" >&2
  diff -u "$EXPECTED" "$ACTUAL" || true
  exit 1
fi
printf 'GUI protocol types are up to date\n'
