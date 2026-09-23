#!/usr/bin/env bash
# List Rust ignored test attributes with crate, source location, reason, and function.
set -euo pipefail
exec python3 "$(dirname "$0")/ignored-tests.py" "$@"
