#!/usr/bin/env bash
# Verify a published release with the code the auto-updater actually ships.
#
# The updater checks an artifact against a `.sig` sidecar before it activates the
# build. Everything in the repository that tests that check does so with a keypair
# generated inside the test, because the *trusted* key is fixed at compile time by
# `option_env!("CHAOS_SIGNING_PUBLIC_KEY")` and a test build only ever carries the
# all-zero placeholder. So the suite could prove "a wrong signature is refused" but
# never the half that matters to a user about to upgrade: does a real release, signed
# by the real key, actually pass the real verifier?
#
# That half is checkable without any private key. The trusted key is a repository
# *variable* (`gh variable list` prints it in clear text -- it is public by design),
# and every release publishes `chaos-<platform>`, `chaos-<platform>.sig` and
# `SHA256SUMS`. So this script fetches those three, recomputes the digest, and runs
# `xai_grok_update::signature::verify_file` -- the shipped function, not a
# reimplementation -- over the downloaded bytes.
#
# Three things have to hold, and the script fails if any of them does not:
#
#   1. the artifact's sha256 matches the published SHA256SUMS
#   2. the sidecar verifies under the configured key, and a one-byte corruption of
#      the same artifact is refused -- without this control, "verified" would also
#      be printed by a verifier that accepts anything
#   3. a build that was given no key refuses instead of accepting
#
# Check 3 is also the only behavioural test that the `rerun-if-env-changed`
# directive in `build.rs` works. The key is baked in at compile time and Cargo
# tracks no environment variables by default, so dropping the variable and building
# again has to relink the crate; if it does not, the binary still carries the key
# from the previous build and reports the key as configured.
#
# Usage:
#   scripts/verify-release-signature.sh                      # latest release, host platform
#   scripts/verify-release-signature.sh --tag v0.4.2
#   scripts/verify-release-signature.sh --asset chaos-linux-x64
#   scripts/verify-release-signature.sh --all          # every artifact in SHA256SUMS
#   scripts/verify-release-signature.sh --keep
#
# Needs: curl, cargo, and `gh` unless --public-key and --tag are both given.
# Downloads one release artifact (a hundred-ish MB) into a temp dir.

set -eu

failures=""
checks=0

say() { printf '   %s\n' "$*"; }
header() { printf '\n== %s\n' "$*"; }
failure() {
  failures="${failures}
  - $1"
  printf '   FAILED %s\n' "$1" >&2
  return 0
}
ok() { say "ok  $1"; }

# ---------------------------------------------------------------- arguments

repo="${CHAOS_REPO:-chao2hang/chaos-code}"
tag=""
asset=""
public_key="${CHAOS_SIGNING_PUBLIC_KEY:-}"
all_assets=0
keep=0

while [ $# -gt 0 ]; do
  case "$1" in
    --tag) tag="${2:-}"; shift 2 ;;
    --asset) asset="${2:-}"; shift 2 ;;
    --all) all_assets=1; shift ;;
    --repo) repo="${2:-}"; shift 2 ;;
    --public-key) public_key="${2:-}"; shift 2 ;;
    --keep) keep=1; shift ;;
    -h|--help)
      sed -n '1,12p' "$0"
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      exit 2
      ;;
  esac
done

for tool in curl cargo; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "$tool is required" >&2
    exit 2
  fi
done

# The repo root, resolved the way BSD `readlink -f` cannot help with.
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)

workdir=$(mktemp -d)
cleanup() {
  if [ "$keep" = "1" ]; then
    say "artifacts kept in ${workdir}"
  else
    rm -rf "$workdir"
  fi
}
trap cleanup EXIT

# `sha256sum` on Linux, `shasum -a 256` on macOS. Same digest, same output shape.
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "no sha256sum/shasum available" >&2
    return 1
  fi
}

# ------------------------------------------------------- what to download

# `gh_release_asset_name()` in the updater maps (os, arch) to the asset stem; this
# is the same table, kept here so the script does not have to run the very binary
# whose release it is checking.
uname_s=$(uname -s)
uname_m=$(uname -m)
case "$uname_s" in
  Linux) os_part=linux ;;
  Darwin) os_part=darwin ;;
  MINGW*|MSYS*|CYGWIN*) os_part=win32 ;;
  *) os_part="$uname_s" ;;
esac
case "$uname_m" in
  x86_64|amd64) arch_part=x64 ;;
  arm64|aarch64) arch_part=arm64 ;;
  *) arch_part="$uname_m" ;;
esac
if [ -z "$asset" ]; then
  asset="chaos-${os_part}-${arch_part}"
  case "$os_part" in win32) asset="${asset}.exe" ;; esac
fi

if [ -z "$tag" ] || [ -z "$public_key" ]; then
  if ! command -v gh >/dev/null 2>&1; then
    echo "gh is required to resolve the latest tag or the public key; pass --tag and --public-key instead" >&2
    exit 2
  fi
fi
if [ -z "$tag" ]; then
  tag=$(gh release view --repo "$repo" --json tagName --jq .tagName)
fi
if [ -z "$public_key" ]; then
  # A repository variable, not a secret: it is the key every installed binary
  # already trusts, so reading it proves nothing about the private half.
  public_key=$(gh variable get CHAOS_SIGNING_PUBLIC_KEY --repo "$repo")
fi

download_base="https://github.com/${repo}/releases/download"
header "release under test"
say "repo:    ${repo}"
say "tag:     ${tag}"
say "asset:   ${asset}"
say "workdir: ${workdir}"

for name in SHA256SUMS "${asset}" "${asset}.sig"; do
  if ! curl -fsSL -o "${workdir}/${name}" "${download_base}/${tag}/${name}"; then
    echo "could not download ${name} from ${tag}" >&2
    exit 1
  fi
done

# ---------------------------------------------------------- 1. sha256

header "published digest"
want=$(awk -v a="$asset" '$2 == a {print $1}' "${workdir}/SHA256SUMS")
if [ -z "$want" ]; then
  failure "SHA256SUMS has no line for ${asset}"
else
  got=$(sha256_of "${workdir}/${asset}")
  checks=$((checks + 1))
  if [ "$got" = "$want" ]; then
    ok "the artifact matches the published SHA256SUMS (${got})"
  else
    failure "digest mismatch: SHA256SUMS says ${want}, the bytes are ${got}"
  fi
fi

checks=$((checks + 1))
if curl -fsSL "https://api.github.com/repos/${repo}/releases/tags/${tag}" \
     -o "${workdir}/release.json" 2>/dev/null \
  && python3 - "${workdir}/release.json" >"${workdir}/assets.txt" <<'PY'
import json, sys
with open(sys.argv[1]) as handle:
    release = json.load(handle)
for asset in release.get("assets", []):
    print(asset["name"])
PY
then
  missing=""
  unsigned=0
  while read -r name; do
    case "$name" in
      SHA256SUMS|*.sig|"${tag}"*) continue ;;
    esac
    if ! grep -qx -- "${name}.sig" "${workdir}/assets.txt"; then
      missing="${missing} ${name}"
      unsigned=$((unsigned + 1))
    fi
  done <"${workdir}/assets.txt"
  if [ "$unsigned" = "0" ]; then
    ok "every published artifact in ${tag} has a .sig sidecar"
  else
    failure "${unsigned} artifact(s) with no sidecar:${missing}"
  fi
else
  failure "could not list the release's assets to look for sidecars"
fi

# -------------------------------------------------- 2. the shipped verifier

example_bin="${repo_root}/target/debug/examples/verify_release_artifact"

header "build the updater example with the release key"
if (cd "$repo_root" && CHAOS_SIGNING_PUBLIC_KEY="$public_key" \
      cargo build --offline --locked --example verify_release_artifact -p xai-grok-update \
      >"${workdir}/build-with-key.log" 2>&1); then
  checks=$((checks + 1))
  ok "the example built against the configured key"
else
  echo "build failed; see the output below" >&2
  sed 's/^/     | /' "${workdir}/build-with-key.log" >&2
  exit 1
fi

header "verify the real signature"
if [ "$all_assets" = "1" ]; then
  # Every artifact the release published a digest for, not just this machine's.
  assets=$(awk '{print $2}' "${workdir}/SHA256SUMS")
else
  assets="$asset"
fi

for current in $assets; do
  if [ "$current" != "$asset" ]; then
    if ! curl -fsSL -o "${workdir}/${current}" "${download_base}/${tag}/${current}"; then
      failure "could not download ${current}"
      continue
    fi
    if ! curl -fsSL -o "${workdir}/${current}.sig" "${download_base}/${tag}/${current}.sig"; then
      failure "could not download ${current}.sig"
      continue
    fi
    want=$(awk -v a="$current" '$2 == a {print $1}' "${workdir}/SHA256SUMS")
    got=$(sha256_of "${workdir}/${current}")
    checks=$((checks + 1))
    if [ "$got" = "$want" ]; then
      ok "${current} matches its published digest"
    else
      failure "${current}: SHA256SUMS says ${want}, the bytes are ${got}"
    fi
  fi

  set +e
  "${example_bin}" "${workdir}/${current}" "${workdir}/${current}.sig" >"${workdir}/verify.log" 2>&1
  verify_status=$?
  set -e
  checks=$((checks + 1))
  if [ "$verify_status" != "0" ]; then
    failure "${current}: verification exited ${verify_status}: $(cat "${workdir}/verify.log")"
    continue
  fi
  if grep -q '^public_key=configured$' "${workdir}/verify.log" && grep -q '^verify\[artifact\]=ok$' "${workdir}/verify.log"; then
    ok "xai_grok_update::signature::verify_file accepted the real sidecar for ${current}"
  else
    failure "${current}: unexpected verdict: $(cat "${workdir}/verify.log")"
  fi
done

checks=$((checks + 1))
set +e
"${example_bin}" "${workdir}/${asset}" "${workdir}/${asset}.sig" --tamper \
  >"${workdir}/tamper.log" 2>&1
tamper_status=$?
set -e
if grep -q '^verify\[tampered\]=ok$' "${workdir}/tamper.log"; then
  failure "a one-byte-corrupted artifact was accepted -- the check above proves nothing"
elif [ "$tamper_status" = "0" ] \
  && grep -q '^verify\[artifact\]=ok$' "${workdir}/tamper.log" \
  && grep -q '^verify\[tampered\]=refused' "${workdir}/tamper.log"; then
  ok "the same verifier refuses the same artifact with one byte flipped"
else
  failure "unexpected tamper outcome (exit ${tamper_status}): $(cat "${workdir}/tamper.log")"
fi

# ------------------------------------------- 3. an unkeyed build refuses

header "a build with no key"
# Cargo relinks only what it knows depends on an env var. `build.rs` names this one,
# so dropping it recompiles the crate; if that ever stops happening the binary keeps
# the previous key and this check is the thing that notices.
if (cd "$repo_root" && env -u CHAOS_SIGNING_PUBLIC_KEY \
      cargo build --offline --locked --example verify_release_artifact -p xai-grok-update \
      >"${workdir}/build-no-key.log" 2>&1); then
  set +e
  "${example_bin}" "${workdir}/${asset}" "${workdir}/${asset}.sig" >"${workdir}/no-key.log" 2>&1
  status=$?
  set -e
  checks=$((checks + 1))
  if [ "$status" = "2" ] && grep -q '^public_key=absent$' "${workdir}/no-key.log"; then
    ok "without a compiled-in key the build refuses instead of trusting the download"
  else
    failure "an unkeyed build did not refuse (exit ${status}): $(cat "${workdir}/no-key.log")"
  fi
else
  failure "the example would not build without the key"
fi

# ------------------------------------------------------------- verdict

printf '\n'
if [ -n "$failures" ]; then
  printf 'FAILED checks:%s\n' "$failures" >&2
  printf '%d check(s) run, at least one failed\n' "$checks" >&2
  exit 1
fi
printf 'all %d check(s) passed\n' "$checks"
