#!/usr/bin/env bash
# Install Chaos CLI from GitHub Releases and put it on PATH.
#
# Layout matches `chaos update` (auto_update::install_gh_release):
#   ~/.chaos/downloads/chaos-<ver>-<platform>   # versioned binary
#   ~/.chaos/bin/chaos  -> ../downloads/...     # relative symlink
#   ~/.chaos/bin/agent  -> ../downloads/...     # alias
#   ~/.chaos/downloads/chaos-latest -> versioned name
#
# One-liner (latest):
#   curl -fsSL https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.sh | bash
#
# Pin a version:
#   curl -fsSL .../install.sh | bash -s -- --version 0.2.113
#   CHAOS_VERSION=0.2.113 bash install.sh
#
# Options:
#   --version X.Y.Z   Release version without leading v (default: latest)
#   --dir DIR         Install directory (default: ~/.chaos/bin or $CHAOS_HOME/bin)
#   --no-path         Do not modify shell rc / PATH
#   --force           Re-download even if the target version is already installed
#   -h, --help        Show help
#
# Existing installs are upgraded in place when the resolved version differs.
# Same-version installs are skipped (use --force to re-fetch).
set -euo pipefail

REPO="${CHAOS_REPO:-chao2hang/chaos-code}"
BIN_NAME="chaos"
VERSION="${CHAOS_VERSION:-}"
INSTALL_DIR=""
MODIFY_PATH=1
FORCE=0

usage() {
  # Under `curl ... | bash -s -- --help` there is no script file to read
  # ($0 is "bash"), so only self-read when $0 is a real file.
  if [[ -f "$0" ]]; then
    sed -n '2,18p' "$0" | sed 's/^# \?//'
  else
    cat <<'EOF'
Install Chaos CLI from GitHub Releases and put it on PATH.

Options:
  --version X.Y.Z   Release version without leading v (default: latest)
  --dir DIR         Install directory (default: ~/.chaos/bin or $CHAOS_HOME/bin)
  --no-path         Do not modify shell rc / PATH
  --force           Re-download even if already on the target version
  -h, --help        Show help

Existing installs are upgraded in place when the version differs.

Environment:
  CHAOS_VERSION         Pin a version (same as --version)
  CHAOS_SKIP_CHECKSUM=1 Skip SHA256 verification (not recommended)
EOF
  fi
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="${2:-}"
      shift 2
      ;;
    --version=*)
      VERSION="${1#*=}"
      shift
      ;;
    --dir)
      INSTALL_DIR="${2:-}"
      shift 2
      ;;
    --dir=*)
      INSTALL_DIR="${1#*=}"
      shift
      ;;
    --no-path)
      MODIFY_PATH=0
      shift
      ;;
    --force)
      FORCE=1
      shift
      ;;
    -h|--help)
      usage
      ;;
    *)
      echo "unknown option: $1" >&2
      exit 2
      ;;
  esac
done

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "error: required command not found: $1" >&2
    exit 1
  }
}

need_cmd curl
need_cmd uname
need_cmd mktemp

# SHA256 helper: coreutils on Linux, `shasum` on macOS. Emits the bare hex digest.
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    return 1
  fi
}

# Prefer CHAOS_HOME / existing layout, same dual-read idea as the npm postinstall.
resolve_home() {
  if [[ -n "${CHAOS_HOME:-}" ]]; then
    echo "$CHAOS_HOME"
    return
  fi
  if [[ -n "${GROK_HOME:-}" ]]; then
    echo "$GROK_HOME"
    return
  fi
  local home chaos grok
  home="${HOME:-}"
  [[ -n "$home" ]] || home="$(eval echo ~)"
  chaos="$home/.chaos"
  grok="$home/.grok"
  if [[ -d "$chaos" ]]; then
    echo "$chaos"
  elif [[ -d "$grok" ]]; then
    echo "$grok"
  else
    echo "$chaos"
  fi
}

# Populate OS_KEY / ARCH_KEY / PLATFORM / ASSET for this host.
# ASSET matches GitHub release names (chaos-linux-x64).
# PLATFORM matches auto-update storage names (linux-x86_64).
detect_platform() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"
  case "$os" in
    linux) OS_KEY=linux ;;
    darwin) OS_KEY=darwin ;;
    mingw*|msys*|cygwin*)
      echo "error: use PowerShell scripts/install.ps1 on Windows" >&2
      exit 1
      ;;
    *)
      echo "error: unsupported OS: $os" >&2
      exit 1
      ;;
  esac
  case "$arch" in
    x86_64|amd64)
      ARCH_KEY=x64
      ARCH_STORAGE=x86_64
      ;;
    aarch64|arm64)
      ARCH_KEY=arm64
      ARCH_STORAGE=aarch64
      ;;
    *)
      echo "error: unsupported arch: $arch" >&2
      exit 1
      ;;
  esac
  # auto-update stores downloads as chaos-<ver>-macos-aarch64 on Darwin.
  if [[ "$OS_KEY" == "darwin" ]]; then
    PLATFORM="macos-${ARCH_STORAGE}"
  else
    PLATFORM="${OS_KEY}-${ARCH_STORAGE}"
  fi
  ASSET="chaos-${OS_KEY}-${ARCH_KEY}"
}

# Atomic-ish symlink replace: write temp link then mv over DEST.
# Works when DEST is missing, a symlink, or a regular file (install.sh
# historically left a bare binary; chaos update expects a symlink).
atomic_symlink() {
  local target="$1"
  local dest="$2"
  local tmp
  tmp="$(mktemp "${dest}.XXXXXX.tmp-link")"
  rm -f "$tmp"
  ln -s "$target" "$tmp"
  mv -f "$tmp" "$dest"
}

latest_version() {
  # Prefer the latest non-draft release tag via GitHub API; fall back to releases/latest redirect.
  local tag
  tag="$(
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
      | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
      | head -1
  )" || true
  if [[ -z "$tag" ]]; then
    tag="$(
      curl -fsSLI "https://github.com/${REPO}/releases/latest" \
        | tr -d '\r' \
        | sed -n 's|^[Ll]ocation: .*tag/\([^/]*\)$|\1|p' \
        | head -1
    )" || true
  fi
  if [[ -z "$tag" ]]; then
    echo "error: could not resolve latest release for ${REPO}" >&2
    exit 1
  fi
  echo "${tag#v}"
}

append_path_line() {
  local rc="$1"
  local line="$2"
  local marker="# chaos-code PATH"
  mkdir -p "$(dirname "$rc")"
  touch "$rc"
  if grep -Fqs "$marker" "$rc" 2>/dev/null || grep -Fqs "$line" "$rc" 2>/dev/null; then
    echo "PATH already configured in $rc"
    return
  fi
  {
    echo ""
    echo "$marker"
    echo "$line"
  } >>"$rc"
  echo "appended PATH export to $rc"
}

configure_path() {
  local dir="$1"
  local export_line="export PATH=\"${dir}:\$PATH\""
  local shell_name rc
  shell_name="$(basename "${SHELL:-bash}")"

  case "$shell_name" in
    zsh)
      rc="${ZDOTDIR:-$HOME}/.zshrc"
      ;;
    bash)
      if [[ -f "$HOME/.bashrc" ]]; then
        rc="$HOME/.bashrc"
      else
        rc="$HOME/.bash_profile"
      fi
      ;;
    fish)
      rc="${XDG_CONFIG_HOME:-$HOME/.config}/fish/config.fish"
      export_line="fish_add_path ${dir}"
      ;;
    *)
      rc="$HOME/.profile"
      ;;
  esac

  append_path_line "$rc" "$export_line"

  # Ensure current process can run chaos immediately when possible.
  case ":${PATH}:" in
    *":${dir}:"*) ;;
    *) export PATH="${dir}:$PATH" ;;
  esac
}

detect_platform
if [[ -z "$VERSION" ]]; then
  VERSION="$(latest_version)"
fi
VERSION="${VERSION#v}"

CHAOS_HOME="$(resolve_home)"
if [[ -z "$INSTALL_DIR" ]]; then
  INSTALL_DIR="${CHAOS_HOME}/bin"
fi
DOWNLOAD_DIR="${CHAOS_HOME}/downloads"
STORED_NAME="chaos-${VERSION}-${PLATFORM}"
STORED_PATH="${DOWNLOAD_DIR}/${STORED_NAME}"

URL="https://github.com/${REPO}/releases/download/v${VERSION}/${ASSET}"
DEST="${INSTALL_DIR}/${BIN_NAME}"
AGENT_DEST="${INSTALL_DIR}/agent"

echo "Chaos installer"
echo "  repo:    ${REPO}"
echo "  version: ${VERSION}"
echo "  asset:   ${ASSET}"
echo "  store:   ${STORED_PATH}"
echo "  dest:    ${DEST} -> ../downloads/${STORED_NAME}"
echo "  url:     ${URL}"

# Default is upgrade-in-place. --force re-downloads even when the target
# version is already installed (useful after a corrupt download).
if [[ -e "$DEST" && "$FORCE" -ne 1 ]]; then
  if [[ -x "$DEST" ]]; then
    cur="$("$DEST" --version 2>/dev/null || true)"
    if [[ -n "$cur" && "$cur" == *"${VERSION}"* ]]; then
      echo "already installed: $cur"
      if [[ "$MODIFY_PATH" -eq 1 ]]; then
        configure_path "$INSTALL_DIR"
      fi
      echo "done. open a new terminal, or in this session:"
      echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
      echo "  chaos --version"
      exit 0
    fi
    if [[ -n "$cur" ]]; then
      echo "upgrading existing install: ${cur} -> ${VERSION}"
    else
      echo "replacing existing binary at ${DEST}"
    fi
  else
    echo "replacing existing path at ${DEST}"
  fi
fi

mkdir -p "$INSTALL_DIR" "$DOWNLOAD_DIR"
TMP="$(mktemp "${DOWNLOAD_DIR}/${STORED_NAME}.XXXXXX.tmp")"
cleanup() { rm -f "$TMP"; }
trap cleanup EXIT

echo "downloading..."
HTTP_CODE="$(
  curl -fL --retry 3 --retry-delay 1 \
    -o "$TMP" \
    -w '%{http_code}' \
    "$URL"
)"
if [[ "$HTTP_CODE" != "200" ]]; then
  echo "error: download failed (HTTP ${HTTP_CODE}): ${URL}" >&2
  echo "  check releases: https://github.com/${REPO}/releases" >&2
  exit 1
fi

# Basic sanity: not an HTML error page
if head -c 16 "$TMP" | grep -qi '<!DOCTYPE\|<html'; then
  echo "error: download looks like HTML, not a binary: ${URL}" >&2
  exit 1
fi

# Integrity: verify against the release's published SHA256SUMS BEFORE the binary
# is ever made executable or run. Set CHAOS_SKIP_CHECKSUM=1 only if you have
# verified the download some other way.
verify_checksum() {
  if [[ "${CHAOS_SKIP_CHECKSUM:-0}" == "1" ]]; then
    echo "warning: checksum verification skipped (CHAOS_SKIP_CHECKSUM=1)" >&2
    return 0
  fi

  local sums expected actual
  sums="$(curl -fsSL "https://github.com/${REPO}/releases/download/v${VERSION}/SHA256SUMS" 2>/dev/null)" || true
  if [[ -z "$sums" ]]; then
    echo "error: could not fetch SHA256SUMS for v${VERSION}." >&2
    echo "  This release may predate checksum publishing. To install anyway," >&2
    echo "  re-run with CHAOS_SKIP_CHECKSUM=1 (you are then trusting the download)." >&2
    exit 1
  fi

  expected="$(printf '%s\n' "$sums" | awk -v f="$ASSET" '$2 == f || $2 == "*" f {print $1; exit}')"
  if [[ -z "$expected" ]]; then
    echo "error: SHA256SUMS has no entry for ${ASSET}" >&2
    exit 1
  fi

  actual="$(sha256_of "$TMP")" || {
    echo "error: no sha256sum/shasum available to verify the download" >&2
    echo "  install coreutils, or re-run with CHAOS_SKIP_CHECKSUM=1" >&2
    exit 1
  }

  if [[ "$actual" != "$expected" ]]; then
    echo "error: checksum mismatch for ${ASSET}" >&2
    echo "  expected: ${expected}" >&2
    echo "  actual:   ${actual}" >&2
    echo "  Refusing to install. This download may be corrupt or tampered with." >&2
    exit 1
  fi
  echo "checksum OK (${actual})"
}
verify_checksum

chmod +x "$TMP"
# Smoke before replace
if ! "$TMP" --version >/dev/null 2>&1; then
  # Some builds may need a tty for full UI; --version should still work.
  if ! "$TMP" --help >/dev/null 2>&1; then
    echo "warning: binary did not respond to --version/--help; installing anyway" >&2
  fi
fi

# Publish into downloads/ under the versioned name auto-update expects, then
# point bin/{chaos,agent} at it via relative symlinks. Relative targets survive
# Docker bind-mounts that remap $HOME (same rationale as chaos update).
mv -f "$TMP" "$STORED_PATH"
chmod +x "$STORED_PATH"
trap - EXIT

REL_TARGET="../downloads/${STORED_NAME}"
atomic_symlink "$REL_TARGET" "$DEST"
atomic_symlink "$REL_TARGET" "$AGENT_DEST"
atomic_symlink "${STORED_NAME}" "${DOWNLOAD_DIR}/chaos-latest"

echo "installed: $DEST -> ${REL_TARGET}"
if "$DEST" --version 2>/dev/null; then
  :
else
  echo "(binary installed; --version unavailable in this environment)"
fi

if [[ "$MODIFY_PATH" -eq 1 ]]; then
  configure_path "$INSTALL_DIR"
fi

echo
echo "OK. Verify (this session may need PATH first — curl|bash does not update the parent shell):"
echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
echo "  chaos --version"
echo "Or use the full path: ${DEST} --version"
echo "New terminals pick up PATH from your shell rc automatically."
