#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ "${GROK_BLITZ_ITERS:-}" =~ [^0-9] ]]; then
  printf 'GROK_BLITZ_ITERS must contain only decimal digits\n' >&2
  exit 2
fi

cargo test -p xai-grok-update --test test_blitz_cancel blitz_fuzz_stress -- --ignored
