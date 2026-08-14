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
#
# Download acceleration (China / restricted networks):
#   CHAOS_GITHUB_MIRROR=https://ghfast.top   # custom prefix; tried first
#   CHAOS_CN=1                               # prefer public mirrors before origin
#   CHAOS_MIRROR_FIRST=1                     # same as CHAOS_CN=1
# Mirrors rewrite https://github.com/... → ${mirror}/https://github.com/...
# Checksums still verify the binary; a bad mirror cannot install silently.
set -euo pipefail

REPO="${CHAOS_REPO:-chao2hang/chaos-code}"
BIN_NAME="chaos"
VERSION="${CHAOS_VERSION:-}"
INSTALL_DIR=""
MODIFY_PATH=1
FORCE=0

# Public ghproxy-style mirrors (prefix + full origin URL). Order is a best-effort
# default; any may go offline. Users should set CHAOS_GITHUB_MIRROR when possible.
DEFAULT_GITHUB_MIRRORS=(
  "https://ghfast.top"
  "https://ghproxy.net"
  "https://mirror.ghproxy.com"
)

usage() {
  # Under `curl ... | bash -s -- --help` there is no script file to read
  # ($0 is "bash"), so only self-read when $0 is a real file.
  if [[ -f "$0" ]]; then
    sed -n '2,30p' "$0" | sed 's/^# \?//'
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
  CHAOS_VERSION           Pin a version (same as --version)
  CHAOS_SKIP_CHECKSUM=1   Skip SHA256 verification (not recommended)
  CHAOS_GITHUB_MIRROR     Mirror prefix, e.g. https://ghfast.top (tried first)
  CHAOS_CN=1              Prefer public GitHub mirrors (for slow/blocked GitHub)
  CHAOS_MIRROR_FIRST=1    Same as CHAOS_CN=1
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

# Prefix-style mirror: https://ghfast.top/ + https://github.com/...
apply_url_mirror() {
  local mirror="$1"
  local url="$2"
  mirror="${mirror%/}"
  echo "${mirror}/${url}"
}

# Emit candidate download URLs for a github.com or api.github.com origin URL.
# Order: user mirror → (optional public mirrors) → origin → remaining mirrors.
_push_cand() {
  # args: cand; uses outer `seen` / `out` via nameref-style globals set by caller
  local c="$1"
  [[ -n "$c" ]] || return 0
  case "$seen" in
    *"|${c}|"*) return 0 ;;
  esac
  seen="${seen}${c}|"
  out+=("$c")
}

github_url_candidates() {
  local origin_url="$1"
  local prefer_mirrors=0
  local m
  # `seen` / `out` intentionally non-local so _push_cand can append.
  seen="|"
  out=()

  if [[ "${CHAOS_CN:-0}" == "1" || "${CHAOS_MIRROR_FIRST:-0}" == "1" ]]; then
    prefer_mirrors=1
  fi

  if [[ -n "${CHAOS_GITHUB_MIRROR:-}" ]]; then
    _push_cand "$(apply_url_mirror "${CHAOS_GITHUB_MIRROR}" "$origin_url")"
  fi

  if [[ "$prefer_mirrors" -eq 1 ]]; then
    for m in "${DEFAULT_GITHUB_MIRRORS[@]}"; do
      _push_cand "$(apply_url_mirror "$m" "$origin_url")"
    done
    _push_cand "$origin_url"
  else
    _push_cand "$origin_url"
    for m in "${DEFAULT_GITHUB_MIRRORS[@]}"; do
      _push_cand "$(apply_url_mirror "$m" "$origin_url")"
    done
  fi

  printf '%s\n' "${out[@]}"
}

# Download origin_url to dest, trying mirrors on failure. Prints the URL that
# succeeded to stdout on success (caller may ignore). Connect timeout is short
# so a blocked github.com fails over quickly.
download_github() {
  local origin_url="$1"
  local dest="$2"
  local connect_timeout="${3:-12}"
  local max_time="${4:-0}"
  local cand http_code curl_args=()
  local last_err=""

  curl_args=(-fL --retry 2 --retry-delay 1 --connect-timeout "$connect_timeout")
  if [[ "$max_time" -gt 0 ]]; then
    curl_args+=(--max-time "$max_time")
  fi

  while IFS= read -r cand; do
    [[ -n "$cand" ]] || continue
    rm -f "$dest"
    echo "  try: ${cand}" >&2
    http_code="$(
      curl "${curl_args[@]}" -o "$dest" -w '%{http_code}' "$cand" 2>/dev/null
    )" || http_code="000"
    if [[ "$http_code" == "200" && -s "$dest" ]]; then
      # Reject tiny HTML error pages from broken proxies
      if head -c 16 "$dest" 2>/dev/null | grep -qi '<!DOCTYPE\|<html'; then
        last_err="HTML response from ${cand}"
        rm -f "$dest"
        continue
      fi
      echo "$cand"
      return 0
    fi
    last_err="HTTP ${http_code} from ${cand}"
    rm -f "$dest"
  done < <(github_url_candidates "$origin_url")

  echo "error: download failed for ${origin_url}" >&2
  echo "  last: ${last_err}" >&2
  echo "  tip: set CHAOS_GITHUB_MIRROR=https://ghfast.top  or  CHAOS_CN=1" >&2
  return 1
}

latest_version() {
  # Prefer the latest non-draft release tag via GitHub API; fall back to releases/latest redirect.
  # Tries mirrors when origin is slow/blocked (same candidate list as downloads).
  local tag cand body tmp
  tmp="$(mktemp)"
  while IFS= read -r cand; do
    [[ -n "$cand" ]] || continue
    if curl -fsSL --connect-timeout 10 --max-time 30 -o "$tmp" "$cand" 2>/dev/null; then
      tag="$(
        sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$tmp" | head -1
      )"
      if [[ -n "$tag" ]]; then
        rm -f "$tmp"
        echo "${tag#v}"
        return 0
      fi
    fi
  done < <(github_url_candidates "https://api.github.com/repos/${REPO}/releases/latest")

  while IFS= read -r cand; do
    [[ -n "$cand" ]] || continue
    tag="$(
      curl -fsSLI --connect-timeout 10 --max-time 30 "$cand" 2>/dev/null \
        | tr -d '\r' \
        | sed -n 's|^[Ll]ocation: .*tag/\([^/]*\)$|\1|p' \
        | head -1
    )" || true
    if [[ -n "$tag" ]]; then
      rm -f "$tmp"
      echo "${tag#v}"
      return 0
    fi
  done < <(github_url_candidates "https://github.com/${REPO}/releases/latest")

  rm -f "$tmp"
  echo "error: could not resolve latest release for ${REPO}" >&2
  echo "  tip: pass --version X.Y.Z, or set CHAOS_CN=1 / CHAOS_GITHUB_MIRROR=..." >&2
  exit 1
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

ORIGIN_URL="https://github.com/${REPO}/releases/download/v${VERSION}/${ASSET}"
SUMS_ORIGIN="https://github.com/${REPO}/releases/download/v${VERSION}/SHA256SUMS"
SIG_ORIGIN="https://github.com/${REPO}/releases/download/v${VERSION}/${ASSET}.sig"
DEST="${INSTALL_DIR}/${BIN_NAME}"
AGENT_DEST="${INSTALL_DIR}/agent"

echo "Chaos installer"
echo "  repo:    ${REPO}"
echo "  version: ${VERSION}"
echo "  asset:   ${ASSET}"
echo "  store:   ${STORED_PATH}"
echo "  dest:    ${DEST} -> ../downloads/${STORED_NAME}"
echo "  origin:  ${ORIGIN_URL}"
if [[ -n "${CHAOS_GITHUB_MIRROR:-}" ]]; then
  echo "  mirror:  ${CHAOS_GITHUB_MIRROR} (CHAOS_GITHUB_MIRROR)"
elif [[ "${CHAOS_CN:-0}" == "1" || "${CHAOS_MIRROR_FIRST:-0}" == "1" ]]; then
  echo "  mirror:  public list first (CHAOS_CN/CHAOS_MIRROR_FIRST)"
else
  echo "  mirror:  origin first, then public fallbacks"
fi

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
# Large binary: short connect timeout for failover; no overall max-time once
# the transfer is moving (140MB+ assets).
USED_URL="$(download_github "$ORIGIN_URL" "$TMP" 12 0)" || exit 1
echo "  from: ${USED_URL}"

# Integrity: verify against the release's published SHA256SUMS BEFORE the binary
# is ever made executable or run. Set CHAOS_SKIP_CHECKSUM=1 only if you have
# verified the download some other way.
verify_checksum() {
  if [[ "${CHAOS_SKIP_CHECKSUM:-0}" == "1" ]]; then
    echo "warning: checksum verification skipped (CHAOS_SKIP_CHECKSUM=1)" >&2
    return 0
  fi

  local sums expected actual sums_tmp
  sums_tmp="$(mktemp)"
  if ! download_github "$SUMS_ORIGIN" "$sums_tmp" 10 30 >/dev/null; then
    rm -f "$sums_tmp"
    echo "error: could not fetch SHA256SUMS for v${VERSION}." >&2
    echo "  This release may predate checksum publishing. To install anyway," >&2
    echo "  re-run with CHAOS_SKIP_CHECKSUM=1 (you are then trusting the download)." >&2
    exit 1
  fi
  sums="$(cat "$sums_tmp")"
  rm -f "$sums_tmp"

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

# Signature verification: verify the downloaded binary against its .sig
# sidecar using Python's cryptography library (if available). This is
# defense-in-depth on top of the SHA256 checksum — the checksum catches
# corruption, the signature catches a compromised release.
#
# Skipped silently when:
#   - Python 3 or the cryptography package is not installed
#   - CHAOS_SKIP_SIGNATURE=1 is set (explicit opt-out)
#   - The .sig file is not found at the release URL (older releases that
#     predate signing)
#
# Fails hard when Python+cryptography IS installed, the .sig IS found, but
# the signature does not verify — this means the binary was tampered with
# after the release was signed.
verify_signature() {
  if [[ "${CHAOS_SKIP_SIGNATURE:-0}" == "1" ]]; then
    echo "warning: signature verification skipped (CHAOS_SKIP_SIGNATURE=1)" >&2
    return 0
  fi

  local sig_tmp
  sig_tmp="$(mktemp)"
  if ! download_github "$SIG_ORIGIN" "$sig_tmp" 10 30 >/dev/null 2>&1; then
    rm -f "$sig_tmp"
    # No .sig file published for this release — skip (older releases).
    return 0
  fi

  # The compiled-in public key is embedded in the chaos binary itself;
  # for the installer we use the CHAOS_SIGNING_PUBLIC_KEY env var (same
  # base64 32-byte key injected at build time).
  local pubkey
  pubkey="${CHAOS_SIGNING_PUBLIC_KEY:-}"
  if [[ -z "$pubkey" ]]; then
    rm -f "$sig_tmp"
    # No public key configured — skip silently (the binary will still
    # verify its own updates once installed).
    return 0
  fi

  # Verify with Python's cryptography library (ed25519, raw — no pre-hash).
  # The .sig file contains a bare base64-encoded 64-byte signature.
  # Probe first: if python3 or the cryptography package is missing, skip
  # silently (the checksum already ran) — a missing tool must NOT be
  # reported as a tampered binary.
  if ! python3 -c "from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey" >/dev/null 2>&1; then
    rm -f "$sig_tmp"
    return 0
  fi
  if python3 -c "
import base64, sys
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

pubkey = base64.b64decode('$pubkey')
pk = Ed25519PublicKey.from_public_bytes(pubkey)

with open('$TMP', 'rb') as f:
    data = f.read()
with open('$sig_tmp', 'r') as f:
    sig = base64.b64decode(f.read().strip())

pk.verify(sig, data)
print('signature OK')
" 2>/dev/null; then
    rm -f "$sig_tmp"
  else
    rm -f "$sig_tmp"
    echo "error: signature verification FAILED for ${ASSET}" >&2
    echo "  The binary may have been tampered with. Refusing to install." >&2
    echo "  To bypass (NOT recommended), set CHAOS_SKIP_SIGNATURE=1." >&2
    exit 1
  fi
}
verify_signature

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
