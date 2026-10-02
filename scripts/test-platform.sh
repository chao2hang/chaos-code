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

# The search tools shell out to ripgrep. Release builds embed it; debug builds —
# what `cargo test` produces — use the host's, and RG_BIN_PATH is how you point
# at one. Rather than have every grep/glob test die at spawn on a machine whose
# PATH has no rg, provision the same pinned release the release build embeds.
rg_ver="15.0.0"
if [ -n "${RG_BIN_PATH:-}" ]; then
  echo "ripgrep      : \$RG_BIN_PATH ($RG_BIN_PATH)"
elif command -v rg >/dev/null 2>&1; then
  RG_BIN_PATH="$(command -v rg)"
  export RG_BIN_PATH
  echo "ripgrep      : $RG_BIN_PATH"
else
  case "$(uname -s)/$(uname -m)" in
    Darwin/arm64) rg_triple="aarch64-apple-darwin" ;;
    Darwin/x86_64) rg_triple="x86_64-apple-darwin" ;;
    Linux/x86_64) rg_triple="x86_64-unknown-linux-musl" ;;
    Linux/aarch64) rg_triple="aarch64-unknown-linux-gnu" ;;
    *) rg_triple='' ;;
  esac
  if [ -z "$rg_triple" ]; then
    echo "ripgrep      : none found and no release for $(uname -s)/$(uname -m); install ripgrep or set RG_BIN_PATH" >&2
    exit 1
  fi
  tools_dir="${GROK_PLATFORM_TOOLS_DIR:-.platform-tools}"
  mkdir -p "$tools_dir"
  if [ ! -x "$tools_dir/rg" ]; then
    echo "== provisioning ripgrep $rg_ver ($rg_triple) =="
    curl -fsSL \
      "https://github.com/BurntSushi/ripgrep/releases/download/$rg_ver/ripgrep-$rg_ver-$rg_triple.tar.gz" \
      -o "$tools_dir/ripgrep.tar.gz"
    tar -xzf "$tools_dir/ripgrep.tar.gz" -C "$tools_dir" --strip-components=1 \
      "ripgrep-$rg_ver-$rg_triple/rg"
    rm -f "$tools_dir/ripgrep.tar.gz"
  fi
  RG_BIN_PATH="$(pwd)/$tools_dir/rg"
  export RG_BIN_PATH
  echo "ripgrep      : $RG_BIN_PATH (downloaded $rg_ver)"
fi
echo

echo "== cargo test =="
cargo test --locked --no-fail-fast "${select_args[@]}"
