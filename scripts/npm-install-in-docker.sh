#!/usr/bin/env bash
# Install the published npm package in a container that has never seen this repo
# and run the binary it puts on PATH.
#
# The release path has two halves. `release.yml` builds six platform artifacts and
# signs them; separately it publishes an npm meta package plus one package per
# platform. The second half had only ever been exercised by its own shell fixtures --
# `scripts/ci/test-publish-npm.sh` feeds the publish script directories of fake
# tarballs and checks what it would send. Nothing here had ever asked the public
# registry for the package and run the thing that came back, which is the only way to
# find out whether the `bin` shim, the `os`/`cpu` fields and the platform-package
# resolution actually work together.
#
# So this script uses a stock `node` image: no cargo, no repo checkout, no `~/.npm`
# cache, nothing that could make the install succeed for a reason that a customer
# would not have. It then asks the questions MT-1 leaves open:
#
#   - does `npm install -g chaos-code` from the public registry succeed at all
#   - does the `chaos` shim on PATH run, and report the version that was installed
#   - is the binary that ran the platform package's payload rather than the JS shim
#   - for a two-platform question: is a *foreign* platform package present, which would
#     mean `os`/`cpu` are not being honoured
#   - what does the install do about the two Windows platform packages, whose names on
#     the public registry are held by `0.0.1-security` placeholder releases while the
#     meta package pins this version
#
# The last one is reported, not asserted: the placeholder names are an npm-support
# problem, and this build cannot resolve them. What this script can pin down is whether
# they make the Linux/macOS install fail, which is the part a user actually hits.
#
# The version installable from npm is whatever was last published; the repository may
# already be ahead of it. The script prints both and treats the gap as information.
#
# Usage:
#   scripts/npm-install-in-docker.sh                 # latest published version
#   scripts/npm-install-in-docker.sh --version 0.2.110
#   scripts/npm-install-in-docker.sh --image node:22-bookworm-slim --keep
#
# Needs: docker, and network access to the public npm registry.

set -eu

image="node:22-bookworm-slim"
package="${CHAOS_NPM_PACKAGE:-chaos-code}"
version=""
container_name="chaos-npm-install-$$"
keep=0

failures=""
checks=0

say() { printf '   %s\n' "$*"; }
header() { printf '\n== %s\n' "$*"; }
ok() { say "ok  $1"; }
bump() { checks=$((checks + 1)); }
note() { say "note  $1"; }
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
    --package) package="${2:-}"; shift 2 ;;
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

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required: the point of this check is a machine with no repo state." >&2
  exit 2
fi
if ! docker image inspect "$image" >/dev/null 2>&1; then
  echo "pulling ${image}" >&2
  docker pull "$image" >&2
fi

cleanup() {
  if [ "$keep" = "1" ]; then
    say "container ${container_name} left running; remove it with: docker rm -f ${container_name}"
  else
    docker rm -f "$container_name" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

# A container of its own, started idle, so every later step is `docker exec` against
# the same fresh state (one install, several observations of it). It is created here,
# before anything is printed, because the registry line below asks the container what
# registry it is configured for -- a host-side mirror would make the run about
# something other than the public registry.
# See scripts/install-sh-in-docker.sh: the idle container needs a ceiling so an
# interrupted run cannot leave one behind, and that ceiling has to outlast a slow but
# successful run or the containers stop underneath a check and the failure points at
# the wrong thing.
docker run -d --name "$container_name" --entrypoint sleep "$image" "${CHAOS_LAB_KEEPALIVE:-21600}" >/dev/null

in_container() {
  docker exec "$container_name" "$@"
}

header "the machine doing the installing"
say "image:      ${image} (stock, no repo, no npm cache)"
say "package:    ${package}"
say "registry:   $(in_container npm config get registry) (as the container sees it)"
say "repo HEAD:  $(git -C "$(dirname "$0")/.." rev-parse --short HEAD 2>/dev/null || echo unknown)"

bump
if in_container node --version >/dev/null 2>&1; then
  ok "the container is up: node $(in_container node --version), npm $(in_container npm --version)"
else
  failure "the container did not come up"
  echo "cannot continue" >&2
  exit 1
fi

# The content cache is `_cacache`; `_logs` and `_update-notifier-last-checked` appear as
# soon as npm runs at all and say nothing about whether the package was pre-seeded.
bump
if in_container sh -c "test -e /root/.npm/_cacache"; then
  failure "the container already had an npm content cache"
else
  ok "the container starts with no npm content cache (/root/.npm/_cacache is absent)"
fi

header "install from the public registry"
spec="${package}"
if [ -n "$version" ]; then
  spec="${package}@${version}"
fi

set +e
in_container sh -c "npm install -g --loglevel=warn ${spec} 2>&1" >"/tmp/npm-install-${container_name}.log" 2>&1
install_status=$?
set -e
bump
if [ "$install_status" = "0" ]; then
  ok "npm install -g ${spec} succeeded"
else
  failure "npm install -g ${spec} exited ${install_status}"
fi
sed 's/^/     | /' "/tmp/npm-install-${container_name}.log" | tail -n 25

installed_version=$(in_container sh -c "npm ls -g --depth=0 --json 2>/dev/null" \
  | tr -d '\n' \
  | sed -n "s/.*\"${package}\": *{[^}]*\"version\": *\"\([^\"]*\)\".*/\1/p")
if [ -z "$installed_version" ]; then
  installed_version="unknown"
fi
bump
if [ "$installed_version" != "unknown" ]; then
  ok "the registry served ${package}@${installed_version}"
else
  failure "could not read the installed version back out of npm ls -g"
fi
note "repository version: $(grep -m1 '^version = ' "$(dirname "$0")/../crates/codegen/xai-grok-pager-bin/Cargo.toml" 2>/dev/null | cut -d'"' -f2 || echo unknown)"

header "what got installed"
# `npm ls --json` lists a package's declared optional dependencies even when it
# skipped them, so it cannot answer "what landed on disk". The installed tree can.
# npm put the platform package under the meta package's own node_modules, so both
# roots are looked at.
global_root=$(in_container sh -c "npm root -g")
in_container sh -c "ls ${global_root} ${global_root}/${package}/node_modules 2>/dev/null" \
  | grep "^${package}-" | sort -u >"/tmp/npm-platforms-${container_name}.txt" || true

bump
platform_count=$(wc -l <"/tmp/npm-platforms-${container_name}.txt" | tr -d ' ')
if [ "$platform_count" = "1" ]; then
  ok "exactly one platform package landed on disk: $(cat "/tmp/npm-platforms-${container_name}.txt")"
elif [ "$platform_count" = "0" ]; then
  failure "no platform package landed on disk; the shim on PATH is not backed by a binary"
else
  failure "${platform_count} platform packages landed on disk, expected 1: $(tr '\n' ' ' <"/tmp/npm-platforms-${container_name}.txt")"
fi

host_os=$(uname -s)
case "$host_os" in
  Linux) own_os=linux ;;
  Darwin) own_os=darwin ;;
  *) own_os="$host_os" ;;
esac
host_arch=$(uname -m)
case "$host_arch" in
  x86_64|amd64) own_arch=x64 ;;
  aarch64|arm64) own_arch=arm64 ;;
  *) own_arch="$host_arch" ;;
esac
own_pkg="${package}-${own_os}-${own_arch}"

bump
if grep -qx -- "$own_pkg" "/tmp/npm-platforms-${container_name}.txt"; then
  ok "the one that landed is the one for this machine's os/cpu (${own_pkg})"
else
  failure "installed ${platform_count} platform package(s) but not ${own_pkg}; os/cpu resolution picked the wrong one"
fi

header "the binary on PATH"
set +e
in_container sh -c "command -v chaos" >"/tmp/npm-which-${container_name}.log" 2>&1
which_status=$?
set -e
bump
if [ "$which_status" = "0" ]; then
  ok "the shim is on PATH: $(cat "/tmp/npm-which-${container_name}.log")"
else
  failure "no chaos on PATH after a successful install"
fi

set +e
in_container sh -c "chaos --version 2>&1" >"/tmp/npm-version-${container_name}.log" 2>&1
version_status=$?
set -e
bump
if [ "$version_status" = "0" ] && grep -Eq '[0-9]+\.[0-9]+' "/tmp/npm-version-${container_name}.log"; then
  ok "chaos --version ran: $(cat "/tmp/npm-version-${container_name}.log")"
else
  failure "chaos --version exited ${version_status}: $(cat "/tmp/npm-version-${container_name}.log")"
fi

# The shim resolves the platform package and execs the binary inside it; if what
# printed the version was the shim itself, the platform package is doing nothing.
# npm nests the platform package under the meta package's own node_modules.
payload_dir="${global_root}/${package}/node_modules/${own_pkg}"
if ! in_container sh -c "test -d ${payload_dir}"; then
  payload_dir="${global_root}/${own_pkg}"
fi
bump
payload_bin=$(in_container sh -c "ls ${payload_dir}/bin 2>/dev/null" | tr '\n' ' ')
if [ -n "$payload_bin" ]; then
  ok "the platform package carries the binary: ${payload_dir}/bin ${payload_bin}"
else
  failure "${payload_dir}/bin is empty or missing"
fi

header "a real command, not just --version"
set +e
in_container sh -c "GROK_HOME=/tmp/chaos-home chaos doctor --json 2>&1 | head -c 400" >"/tmp/npm-doctor-${container_name}.log" 2>&1
doctor_status=$?
set -e
bump
if [ "$doctor_status" = "0" ] && grep -q '{' "/tmp/npm-doctor-${container_name}.log"; then
  ok "chaos doctor starts and prints JSON"
else
  note "chaos doctor exited ${doctor_status}: $(head -c 200 "/tmp/npm-doctor-${container_name}.log")"
fi

header "the same install asked to resolve for a foreign platform"
# The two Windows platform packages are the interesting case, and npm can be asked to
# filter optional dependencies for a platform other than the host's (`--os`/`--cpu`, npm
# 9.5+). That is the only way to ask the Windows question from a Linux box, and it is
# worth asking: an unresolvable *optional* dependency is skipped rather than reported, so
# `npm install` can print success while installing something that cannot run.
win_prefix=/tmp/win32-x64-prefix
set +e
win_out=$(in_container sh -c "npm install -g --prefix ${win_prefix} --loglevel=warn --os=win32 --cpu=x64 ${spec} 2>&1")
win_status=$?
set -e
printf '%s\n' "$win_out" | sed 's/^/     | /' | tail -n 6
bump
win_platforms=$(in_container sh -c "ls ${win_prefix}/lib/node_modules/${package}/node_modules 2>/dev/null" \
  | grep "^${package}-" || true)
if [ -z "$win_platforms" ]; then
  note "for win32-x64 npm exited ${win_status} and installed no platform package: the optional dependency was skipped, not resolved"
else
  ok "for win32-x64 npm installed: $(printf '%s' "$win_platforms" | tr '\n' ' ')"
fi

# The failure a Windows user actually sees must be a written message with a usable exit
# code, not a stack trace and not a silently empty command. Only node's view of the
# platform is stubbed here; the launcher under test is the file that ships.
probe_local="/tmp/chaos-probe-win32-${container_name}.js"
cat >"$probe_local" <<EOF
Object.defineProperty(process, 'platform', { value: 'win32' });
Object.defineProperty(process, 'arch', { value: 'x64' });
require('${win_prefix}/lib/node_modules/${package}/bin/chaos');
EOF
# docker exec only forwards stdin with -i.
docker exec -i "$container_name" sh -c 'cat > /tmp/probe-win32.js' < "$probe_local"
rm -f "$probe_local"
set +e
shim_out=$(in_container node /tmp/probe-win32.js 2>&1)
shim_status=$?
set -e
bump
if [ "$shim_status" != "0" ] && printf '%s' "$shim_out" | grep -q 'no platform binary installed for win32-x64'; then
  ok "with no win32 binary present the launcher fails loudly (exit ${shim_status}): $(printf '%s\n' "$shim_out" | head -1)"
else
  failure "launcher with no win32 binary exited ${shim_status}: $(printf '%s' "$shim_out" | head -c 200)"
fi

# Does every version the meta package pins actually exist under that name? An
# optionalDependency pinned to an unpublished version is invisible at install time and
# fatal at run time, so this is the check that says which of the six platforms npm can
# serve at all.
bump
pins=$(in_container sh -c "node -e '
const p = require(\"${win_prefix}/lib/node_modules/${package}/package.json\");
for (const [k, v] of Object.entries(p.optionalDependencies || {})) console.log(k + \" \" + v);
'")
missing_pins=""
while read -r pin_name pin_version; do
  [ -n "$pin_name" ] || continue
  have=$(in_container sh -c "npm view ${pin_name} versions --json 2>/dev/null" | tr -d '[]" \n' || true)
  case ",$have," in
    *",$pin_version,"*) : ;;
    *) missing_pins="${missing_pins}
     - ${pin_name}@${pin_version} (published: ${have:-none})" ;;
  esac
done <<EOF
$pins
EOF
if [ -z "$missing_pins" ]; then
  ok "every platform package version pinned by the meta package exists on the registry"
else
  failure "pinned but unpublished platform packages (npm installs these silently):${missing_pins}"
fi

printf '\n'
if [ -n "$failures" ]; then
  printf 'FAILED checks:%s\n' "$failures" >&2
  printf '%d check(s) run, at least one failed\n' "$checks" >&2
  exit 1
fi
printf 'all %d check(s) passed\n' "$checks"
