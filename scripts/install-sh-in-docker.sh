#!/usr/bin/env bash
# Run the documented installer in a machine that has never seen this repository,
# against the real published release.
#
# The release path has three halves and this is the last one to get a runnable check.
# `release.yml` builds and signs six artifacts; the same workflow publishes npm
# packages; and `scripts/install.sh` is what README tells a user to pipe into bash.
# Until now the installer had only been exercised by
# `scripts/ci/test-installer-signature-policy.py`, which reads the script as text and
# asserts that certain error strings exist in it. Text assertions cannot tell you that
# the script downloads 150 MB and only then discovers it has no signing key to verify
# with, which is exactly what it did: signature verification is fail-closed and the key
# came from `CHAOS_SIGNING_PUBLIC_KEY`, an environment variable no README example sets.
# So `curl -fsSL .../install.sh | bash` -- the command on the front page -- could not
# complete a fresh install on any machine. All three installers now ship the public key
# as a built-in default (see DEFAULT_SIGNING_PUBLIC_KEY); the environment variable
# still overrides it for anyone signing their own releases.
#
# What this script asks, in a stock Debian container with no cargo, no repo and no key
# passed in by the caller:
#
#   - does the installer run to completion using only its own built-in key
#   - did the checksum and signature checks actually report OK (not silently skipped)
#   - is what landed on disk the release artifact, reachable through relative symlinks
#     that survive a bind-mount remapping $HOME
#   - does the installed binary run
#   - is a second run a no-op instead of a second download
#   - does a key that is present-but-blank still fail closed, and before downloading
#   - does a *different* valid key refuse the same artifact, leaving the install alone
#
# The last two are the controls: without them the passing signature line could just as
# well mean "verification was skipped".
#
# Usage:
#   scripts/install-sh-in-docker.sh                      # real release feed
#   scripts/install-sh-in-docker.sh --version 0.4.2      # pin it
#   scripts/install-sh-in-docker.sh --skip-wrong-key     # save one 150 MB download
#   scripts/install-sh-in-docker.sh --keep
#
# Needs: docker, network access to github.com (releases + the raw script), and `gh`
# only for the optional cross-check of the built-in key against the repository
# variable (skipped with a note when gh is absent or unauthenticated).

set -eu

image="debian:bookworm-slim"
repo="${CHAOS_REPO:-chao2hang/chaos-code}"
version=""
container_name="chaos-install-sh-$$"
keep=0
skip_wrong_key=0
script_src="$(dirname "$0")/install.sh"

checks=0
failures=""
notes=""

say() { printf '   %s\n' "$*"; }
header() { printf '\n== %s\n' "$*"; }
ok() { say "ok  $1"; }
bump() { checks=$((checks + 1)); }
note() { notes="${notes}
  - $1"; say "note  $1"; }
failure() {
  failures="${failures}
  - $1"
  printf '   FAILED %s\n' "$1" >&2
  return 0
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version) version="${2:-}"; shift 2 ;;
    --image) image="${2:-}"; shift 2 ;;
    --repo) repo="${2:-}"; shift 2 ;;
    --skip-wrong-key) skip_wrong_key=1; shift ;;
    --keep) keep=1; shift ;;
    -h|--help) sed -n '1,12p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required: the point is a machine with no prior install." >&2
  exit 2
fi
if [ ! -f "$script_src" ]; then
  echo "cannot find ${script_src}" >&2
  exit 2
fi

# The key the installers embed. Read out of the script rather than hard-coded here, so
# the script under test and the check cannot drift apart into two different claims.
embedded_key=$(sed -n "s/^DEFAULT_SIGNING_PUBLIC_KEY='\([^']*\)'.*/\1/p" "$script_src")
if [ -z "$embedded_key" ]; then
  echo "no DEFAULT_SIGNING_PUBLIC_KEY in ${script_src}: nothing to test." >&2
  exit 2
fi

# A second, syntactically valid ed25519 key that has never signed anything of ours.
# `wrong` here means "not ours", not "malformed" -- a malformed key would let a bug in
# the base64 decoder look like a security check.
wrong_key="$(python3 -c 'import base64,hashlib;print(base64.b64encode(hashlib.sha256(b"not the chaos release key").digest()).decode())')"

if [ "$image" = "debian:bookworm-slim" ] && ! docker image inspect "$image" >/dev/null 2>&1; then
  echo "pulling ${image}" >&2
  docker pull "$image" >&2
fi

cleanup() {
  if [ "$keep" = "1" ]; then
    say "container ${container_name} left running: docker rm -f ${container_name}"
  else
    docker rm -f "$container_name" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

docker run -d --name "$container_name" --entrypoint sleep "$image" 3600 >/dev/null
in_container() { docker exec "$container_name" "$@"; }
# docker exec only forwards stdin with -i; without it the installer would arrive empty.
in_container_write() { docker exec -i "$container_name" sh -c "cat > $1"; }

header "the machine doing the installing"
say "image:        ${image} (stock Debian; no cargo, no repo, no prior install)"
say "installer:    ${script_src} (working tree)"
say "repo:         ${repo}"
say "built-in key: ${embedded_key}"

bump
if in_container uname -a >/dev/null 2>&1; then
  ok "container is up: $(in_container uname -srm)"
else
  failure "container did not come up"
  echo "cannot continue" >&2
  exit 1
fi

# The installer needs curl and, for signatures, python3 + cryptography. Installing them
# from distro packages is itself part of the claim: a user following the README is
# expected to be able to satisfy the prerequisites with what their distro ships.
bump
if in_container sh -c "apt-get update -qq >/dev/null 2>&1 \
  && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq curl ca-certificates python3 python3-cryptography >/dev/null 2>&1"; then
  ok "prerequisites install from distro packages: curl, python3, python3-cryptography"
else
  failure "could not install curl/python3/python3-cryptography from the distro"
  echo "cannot continue" >&2
  exit 1
fi
say "python3 $(in_container python3 --version 2>&1 | cut -d' ' -f2), cryptography $(in_container python3 -c 'import cryptography;print(cryptography.__version__)')"

# Clean-machine claims are only worth making after they have been checked.
bump
if in_container sh -c "test -e /root/.chaos || command -v chaos >/dev/null 2>&1 || command -v cargo >/dev/null 2>&1"; then
  failure "the container was not clean (an existing ~/.chaos, chaos, or cargo was found)"
else
  ok "no prior install to inherit: no ~/.chaos, no chaos on PATH, no cargo"
fi

# Cross-check the built-in key against the repository variable that the release
# workflow injects at build time. Not a failure without gh: the release itself is
# checked by scripts/verify-release-signature.sh.
# bump only once it is known the check can run, so a skipped cross-check does not
# inflate the total.
published_key=""
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  bump
  published_key=$(gh variable get CHAOS_SIGNING_PUBLIC_KEY -R "$repo" 2>/dev/null || true)
  if [ "$published_key" = "$embedded_key" ]; then
    ok "the built-in key is the repository's CHAOS_SIGNING_PUBLIC_KEY"
  else
    failure "the installer ships ${embedded_key} but the repository publishes ${published_key:-<unreadable>}"
  fi
else
  note "skipped the repository-variable cross-check (needs an authenticated gh); the built-in key is still exercised against the real release below"
fi

in_container mkdir -p /opt/chaos-installer
in_container_write /opt/chaos-installer/install.sh < "$script_src"
in_container chmod +x /opt/chaos-installer/install.sh

header "the command README tells users to run"
# The raw URL is what a reader actually copies. Comparing it against the working tree is
# a note, not a failure, because main only catches up once this commit is pushed.
raw_url="https://raw.githubusercontent.com/${repo}/main/scripts/install.sh"
in_container sh -c "curl -fsSL '${raw_url}' -o /tmp/install-main.sh" 2>/dev/null || true
if in_container test -s /tmp/install-main.sh; then
  if in_container cmp -s /tmp/install-main.sh /opt/chaos-installer/install.sh; then
    ok "the published ${raw_url} is byte-identical to the script under test"
  else
    note "${raw_url} differs from the working-tree script (main lags this change until it is pushed)"
  fi
else
  note "could not fetch ${raw_url} to compare against the working tree"
fi
# Which ever copy is used from here on, it is fetched the way a user fetches it: over
# HTTPS, into a shell, with no environment prepared for it.
install_args=""
if [ -n "$version" ]; then
  install_args="--version ${version}"
fi

run_install() {
  # args: <env assignments, as `env` arguments> <extra install.sh args>
  #
  # They go through `env` rather than as a `VAR=x;` prefix: a bare assignment
  # followed by `;` only sets a shell variable, so the installer never saw it and
  # both fail-closed controls below ran with the script's own built-in key and
  # exited 0. `env VAR=` is also how you express "present but empty".
  env_assignments="$1"
  shift
  in_container sh -c "env ${env_assignments} bash /opt/chaos-installer/install.sh ${install_args} $* 2>&1"
}

set +e
install_out=$(run_install "" "")
install_status=$?
set -e
printf '%s\n' "$install_out" | sed 's/^/     | /'
bump
if [ "$install_status" = "0" ]; then
  ok "install.sh exited 0 with no CHAOS_SIGNING_PUBLIC_KEY in the environment"
else
  failure "install.sh exited ${install_status} using only its built-in key"
fi
bump
if printf '%s\n' "$install_out" | grep -q '^checksum OK'; then
  ok "the digest was checked: $(printf '%s\n' "$install_out" | grep -m1 '^checksum OK')"
else
  failure "no 'checksum OK' line: the published SHA256SUMS was not consulted"
fi
bump
if printf '%s\n' "$install_out" | grep -q '^signature OK'; then
  ok "the signature was checked: $(printf '%s\n' "$install_out" | grep -m1 '^signature OK')"
else
  failure "no 'signature OK' line: signature verification did not run"
fi
if printf '%s\n' "$install_out" | grep -q 'verification skipped'; then
  failure "signature verification reported itself skipped"
fi

installed_version=$(printf '%s\n' "$install_out" | sed -n 's/^  version: \([^ ]*\).*/\1/p' | head -1)
if [ -z "$installed_version" ]; then
  installed_version="$version"
fi
if [ "$install_status" != "0" ]; then
  # Everything below asserts on the installed tree; continuing would bury the real
  # failure under a dozen path-not-found messages.
  printf 'install.sh failed (exit %s); the remaining checks need an installed tree\n' "$install_status" >&2
  printf 'FAILED checks:%s\n' "$failures" >&2
  exit 1
fi

# install.sh stores the artifact under the auto-update name (linux-x86_64), which is
# not the release asset name (chaos-linux-x64).
case "$(in_container uname -m)" in
  x86_64) arch_storage=x86_64 ;;
  aarch64) arch_storage=aarch64 ;;
  *) arch_storage="$(in_container uname -m)" ;;
esac

header "what landed on disk"
chaos_home=$(in_container printenv CHAOS_HOME 2>/dev/null || true)
[ -n "$chaos_home" ] || chaos_home="/root/.chaos"
stored="${chaos_home}/downloads/chaos-${installed_version}-linux-${arch_storage}"

bump
if in_container test -s "$stored"; then
  ok "the artifact is stored under the versioned name auto-update expects: ${stored}"
else
  failure "expected the artifact at ${stored}"
fi

bump
link_target=$(in_container readlink "${chaos_home}/bin/chaos" 2>/dev/null || true)
if [ -n "$link_target" ]; then
  case "$link_target" in
    /*) failure "bin/chaos points at an absolute path (${link_target}); the installer promises relative targets so a remapped \$HOME still resolves" ;;
    *) ok "bin/chaos -> ${link_target} (relative, so it survives a bind-mount that remaps \$HOME)" ;;
  esac
else
  failure "no symlink at ${chaos_home}/bin/chaos"
fi
bump
if in_container test -x "${chaos_home}/bin/chaos"; then
  ok "bin/chaos resolves and is executable"
else
  failure "bin/chaos does not resolve to an executable (broken symlink?)"
fi
bump
if in_container test -x "${chaos_home}/bin/agent"; then
  ok "bin/agent points at the same artifact"
else
  failure "bin/agent is missing or not executable"
fi
bump
if in_container test -x "${chaos_home}/downloads/chaos-latest"; then
  ok "downloads/chaos-latest names the current version"
else
  failure "downloads/chaos-latest is missing"
fi

bump
reported=$(in_container "${chaos_home}/bin/chaos" --version 2>&1 | head -1)
if printf '%s' "$reported" | grep -q "$installed_version"; then
  ok "the installed binary runs and reports: ${reported}"
else
  failure "the installed binary reports '${reported}', expected version ${installed_version}"
fi

# install.sh edits a shell rc by default; a PATH it never sets up means `chaos` is not
# actually available to the user afterwards.
bump
if in_container sh -c "grep -lq 'chaos/bin' /root/.bashrc /root/.bash_profile /root/.profile 2>/dev/null"; then
  ok "PATH was configured in a shell rc"
else
  note "no PATH line added to a shell rc (SHELL unset in this container; install.sh picks the rc from \$SHELL)"
fi

header "a second run"
set +e
again=$(run_install "" "")
again_status=$?
set -e
printf '%s\n' "$again" | sed 's/^/     | /'
bump
if [ "$again_status" = "0" ] && printf '%s\n' "$again" | grep -q 'already installed'; then
  ok "re-running is a no-op: $(printf '%s\n' "$again" | grep -m1 'already installed')"
else
  failure "re-running exited ${again_status}; expected a no-op reporting 'already installed'"
fi

header "controls: the signature check has to be able to say no"
# First prove the harness can put a value in the child's environment at all. Every
# control below reads a refusal as success, so a control that silently ran with the
# built-in key would report "no refusal" and look like an installer bug instead of
# the harness bug it is.
bump
blank_probe=$(in_container env CHAOS_SIGNING_PUBLIC_KEY= sh -c \
  'printf %s "${CHAOS_SIGNING_PUBLIC_KEY+set}"')
if [ "$blank_probe" = "set" ]; then
  ok "the controls really do set CHAOS_SIGNING_PUBLIC_KEY in the child's environment"
else
  failure "could not put CHAOS_SIGNING_PUBLIC_KEY in the child's environment (probe said '${blank_probe}')"
  echo "the controls below would be meaningless" >&2
  exit 1
fi

# Present-but-blank key. This is the fail-fast claim: the artifact is 150 MB+, so the
# refusal must happen before the transfer, not after it.
set +e
blank_out=$(run_install 'CHAOS_SIGNING_PUBLIC_KEY=' "--force")
blank_status=$?
set -e
bump
if [ "$blank_status" != "0" ] \
  && printf '%s\n' "$blank_out" | grep -q 'CHAOS_SIGNING_PUBLIC_KEY is required' \
  && ! printf '%s\n' "$blank_out" | grep -q 'downloading'; then
  ok "a blank key is refused before any download starts"
else
  failure "a blank key did not fail fast (exit ${blank_status}); output was: $(printf '%s\n' "$blank_out" | tr '\n' ' ' | head -c 300)"
fi

if [ "$skip_wrong_key" = "1" ]; then
  note "wrong-key control skipped (--skip-wrong-key); it costs one more full artifact download"
else
  digest_before=$(in_container sha256sum "${stored}" | cut -d' ' -f1)
  set +e
  wrong_out=$(run_install "CHAOS_SIGNING_PUBLIC_KEY=${wrong_key}" "--force")
  wrong_status=$?
  set -e
  printf '%s\n' "$wrong_out" | tail -n 6 | sed 's/^/     | /'
  bump
  if [ "$wrong_status" != "0" ] && printf '%s\n' "$wrong_out" | grep -q 'signature verification FAILED'; then
    ok "a valid-but-foreign key is refused with 'signature verification FAILED'"
  else
    failure "a foreign key did not cause a refusal (exit ${wrong_status})"
  fi
  bump
  digest_after=$(in_container sha256sum "${stored}" 2>/dev/null | cut -d' ' -f1 || true)
  if [ "$digest_after" = "$digest_before" ]; then
    ok "the refused install left the working artifact untouched (${digest_after})"
  else
    failure "the refused install changed ${stored}: ${digest_before} -> ${digest_after}"
  fi
fi

printf '\n'
if [ -n "$notes" ]; then
  printf 'notes:%s\n' "$notes"
fi
if [ -n "$failures" ]; then
  printf 'FAILED checks:%s\n' "$failures" >&2
  printf '%d check(s) run, at least one failed\n' "$checks" >&2
  exit 1
fi
printf 'all %d check(s) passed\n' "$checks"
