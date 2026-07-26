#!/usr/bin/env bash
# Install Chaos CLI from GitHub Releases and put it on PATH.
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
#   --force           Overwrite existing binary
#   -h, --help        Show help
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
  --force           Overwrite existing binary
  -h, --help        Show help

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

detect_asset() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"
  case "$os" in
    linux) os=linux ;;
    darwin) os=darwin ;;
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
    x86_64|amd64) arch=x64 ;;
    aarch64|arm64) arch=arm64 ;;
    *)
      echo "error: unsupported arch: $arch" >&2
      exit 1
      ;;
  esac
  echo "chaos-${os}-${arch}"
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

ASSET="$(detect_asset)"
if [[ -z "$VERSION" ]]; then
  VERSION="$(latest_version)"
fi
VERSION="${VERSION#v}"

if [[ -z "$INSTALL_DIR" ]]; then
  INSTALL_DIR="$(resolve_home)/bin"
fi

URL="https://github.com/${REPO}/releases/download/v${VERSION}/${ASSET}"
DEST="${INSTALL_DIR}/${BIN_NAME}"

echo "Chaos installer"
echo "  repo:    ${REPO}"
echo "  version: ${VERSION}"
echo "  asset:   ${ASSET}"
echo "  dest:    ${DEST}"
echo "  url:     ${URL}"

if [[ -e "$DEST" && "$FORCE" -ne 1 ]]; then
  if [[ -x "$DEST" ]]; then
    cur="$("$DEST" --version 2>/dev/null || true)"
    if [[ "$cur" == *"${VERSION}"* ]]; then
      echo "already installed: $cur"
      if [[ "$MODIFY_PATH" -eq 1 ]]; then
        configure_path "$INSTALL_DIR"
      fi
      echo "done. open a new terminal or: export PATH=\"${INSTALL_DIR}:\$PATH\""
      exit 0
    fi
  fi
  echo "error: ${DEST} exists (use --force to overwrite)" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
TMP="$(mktemp "${TMPDIR:-/tmp}/chaos-install.XXXXXX")"
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

mv -f "$TMP" "$DEST"
chmod +x "$DEST"
trap - EXIT

echo "installed: $DEST"
if "$DEST" --version 2>/dev/null; then
  :
else
  echo "(binary installed; --version unavailable in this environment)"
fi

if [[ "$MODIFY_PATH" -eq 1 ]]; then
  configure_path "$INSTALL_DIR"
fi

echo
echo "OK. Run: chaos --version"
echo "If command not found, open a new terminal or:"
echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
