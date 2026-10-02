#!/usr/bin/env bash
# Platform regression entry point: runs the crate tests whose behaviour is
# selected by `cfg(unix)` / `cfg(windows)` / `cfg(target_os = ...)`, so a
# macOS or Windows machine can verify its own code paths with one command.
#
# Usage:
#   scripts/test-platform.sh                 # default target-OS crate set
#   scripts/test-platform.sh xai-tty-utils   # only the named crates
#
# Capture evidence with:
#   scripts/test-platform.sh 2>&1 | tee platform-test-$(uname -s).log
set -euo pipefail

# Crates carrying target-OS code: TTY/stderr handle handling, the sandbox's
# seccomp/namespace vs Job Object paths, PTY teardown, the updater's per-OS
# installer hint and process spawning, and the composition-root binary.
default_crates=(
  xai-tty-utils
  xai-grok-sandbox
  xai-grok-shell-terminal
  xai-grok-update
  xai-grok-tools
  xai-grok-pager-bin
)

crates=("${@:-}")
if [ "${#crates[@]}" -eq 0 ] || [ -z "${crates[0]:-}" ]; then
  crates=("${default_crates[@]}")
fi

# Large actor tests exceed the default 2 MiB test-thread stack; CI uses 16 MiB.
export RUST_MIN_STACK="${RUST_MIN_STACK:-16777216}"

echo "== platform report =="
echo "os           : $(uname -srm 2>/dev/null || echo unknown)"
echo "hostname     : $(hostname 2>/dev/null || echo unknown)"
echo "rustc        : $(rustc -Vv 2>/dev/null | tr '\n' ' ' || echo missing)"
echo "cargo        : $(cargo -V 2>/dev/null || echo missing)"
echo "logical cpus : $(getconf _NPROCESSORS_ONLN 2>/dev/null || echo unknown)"
echo "RUST_MIN_STACK: $RUST_MIN_STACK"
echo "git commit   : $(git rev-parse HEAD 2>/dev/null || echo unknown)"
# A dirty tree means the log does not describe the pushed commit, so say so.
if [ -n "$(git status --porcelain 2>/dev/null | head -1)" ]; then
  echo 'working tree : dirty (log does not describe a pushed commit)'
fi
echo "crates       : ${crates[*]}"
echo

select_args=()
for crate in "${crates[@]}"; do
  select_args+=(-p "$crate")
done

echo "== cargo test =="
cargo test --locked --no-fail-fast "${select_args[@]}"
