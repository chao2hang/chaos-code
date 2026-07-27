#!/usr/bin/env bash
# Point this clone's hooks at scripts/hooks/ so the pre-commit secret scan runs.
#
# core.hooksPath is used instead of copying into .git/hooks: hooks stay version
# controlled, and an update to scripts/hooks/ reaches everyone who has run this
# once, rather than silently going stale in each clone.
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

chmod +x scripts/hooks/* 2>/dev/null || true
git config core.hooksPath scripts/hooks

echo "hooks installed: core.hooksPath -> scripts/hooks"
echo "verify with: git config --get core.hooksPath"
