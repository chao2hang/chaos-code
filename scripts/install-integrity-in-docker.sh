#!/usr/bin/env bash
# Release-integrity acceptance lab: install from a fake release, in a container
# with no network at all.
#
# scripts/install-sh-in-docker.sh already installs the *real* release over HTTPS and
# checks that the checksum and signature lines say OK. That lab cannot answer the
# question that actually matters about an integrity check: what does it do when the
# bytes are wrong? A feed that only ever serves correct artifacts proves nothing about
# a refusal, and a refusal you cannot trigger is indistinguishable from a check that
# was silently skipped.
#
# So this lab builds its own release -- an artifact, a SHA256SUMS entry and an Ed25519
# signature over the artifact bytes -- and serves it through the ghproxy-style mirror
# path scripts/install.sh already supports (`${CHAOS_GITHUB_MIRROR}/https://github.com/...`).
# The container runs with `--network none`: loopback works, DNS resolves nothing, so
# every byte the installer consumed came from this fixture and github.com was
# physically unreachable. Each scenario gets its own release directory, and the
# scenarios are the interesting part: a tampered artifact, SHA256SUMS rewritten to
# match the tampered bytes while the signature still covers the original, a missing
# sidecar, a valid key that is not ours, an HTML error page served in place of the
# checksums, a truncated download.
#
# Usage:
#   scripts/install-integrity-in-docker.sh              # build the image if needed, run
#   scripts/install-integrity-in-docker.sh --image NAME # use a prebuilt lab image
#   scripts/install-integrity-in-docker.sh --keep       # leave the container and files
#
# Needs: docker. Building the base image needs the network once; the lab itself runs
# with no network access whatsoever.

set -eu

image="chaos-install-lab:local"
base_image="debian:bookworm-slim"
container_name="chaos-install-integrity-$$"
keep=0
script_src="$(cd "$(dirname "$0")" && pwd)/install.sh"
ci_src="$(cd "$(dirname "$0")" && pwd)/ci"
version="9.9.9"
port=8099

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
    --image) image="${2:-}"; shift 2 ;;
    --base-image) base_image="${2:-}"; shift 2 ;;
    --keep) keep=1; shift ;;
    -h|--help) sed -n '2,27p' "$0" | sed 's/^# \?//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required: the point is a machine that cannot reach the network." >&2
  exit 2
fi
if [ ! -f "$script_src" ]; then
  echo "cannot find ${script_src}" >&2
  exit 2
fi

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/chaos-install-integrity-lab.XXXXXX")"

cleanup() {
  if [ "$keep" = "1" ]; then
    say "container ${container_name} left running: docker rm -f ${container_name}"
    say "lab files kept in ${WORK_DIR}"
  else
    docker rm -f "$container_name" >/dev/null 2>&1 || true
    rm -rf "$WORK_DIR"
  fi
}
trap cleanup EXIT

# The image is the one thing that needs the network, and only to build. python3 with
# cryptography is not decoration: scripts/install.sh verifies the signature with it and
# refuses outright without it, so the lab has to offer what a user's distro offers.
if ! docker image inspect "$image" >/dev/null 2>&1; then
  say "building ${image} from ${base_image} (needs network once)"
  {
    printf 'FROM %s\n' "$base_image"
    printf 'ENV DEBIAN_FRONTEND=noninteractive\n'
    printf 'RUN apt-get update -qq \\\n'
    printf ' && apt-get install -y -qq --no-install-recommends \\\n'
    printf '      ca-certificates curl python3 python3-cryptography iproute2 \\\n'
    printf ' && rm -rf /var/lib/apt/lists/*\n'
    printf 'CMD ["sleep", "3600"]\n'
  } > "${WORK_DIR}/Dockerfile"
  docker build -q -t "$image" -f "${WORK_DIR}/Dockerfile" "$WORK_DIR" >/dev/null
fi

mkdir -p "${WORK_DIR}/releases" "${WORK_DIR}/shared"
cp "$script_src" "${WORK_DIR}/install.sh"
log_dir="${WORK_DIR}/logs"
mkdir -p "$log_dir"

# See scripts/install-sh-in-docker.sh: the idle container needs a ceiling so an
# interrupted run cannot leave one behind, and that ceiling has to outlast a slow but
# successful run or the container stops underneath a check and the failure points at the
# wrong thing. This one serves no requests from the network, so it is also offline.
docker run -d --name "$container_name" --network none \
  --entrypoint sleep "$image" "${CHAOS_LAB_KEEPALIVE:-21600}" >/dev/null
in_container() { docker exec "$container_name" "$@"; }
in_container_sh() { docker exec "$container_name" bash -c "$1"; }

header "the machine doing the installing"
say "image:      ${image}"
say "installer:  ${script_src} (working tree)"
say "release:    a fixture built inside the container, version ${version} (never published)"
say "network:    --network none"

# Everything else in this file rests on this one. If the container could reach the real
# feed, a green run would say nothing about the fixture.
bump
dns_probe="$(in_container_sh 'curl -sS --connect-timeout 3 --max-time 8 -o /dev/null https://github.com/ 2>&1; echo "exit=$?"')"
if printf '%s' "$dns_probe" | grep -q "Could not resolve host" \
  && printf '%s' "$dns_probe" | grep -q "exit=6"; then
  ok "github.com is unreachable from this container (curl exit 6, could not resolve host)"
else
  failure "the container reached the network (curl said: $(printf '%s' "$dns_probe" | head -1))"
fi
bump
if in_container_sh "ip -o addr show lo 2>/dev/null | grep -q '127.0.0.1'"; then
  ok "loopback is up, which is all the fake release endpoint needs"
else
  failure "loopback is not up; a --network none container cannot serve its own fixture"
  echo "cannot continue" >&2
  exit 1
fi
bump
if in_container_sh "test -e /root/.chaos || command -v chaos >/dev/null 2>&1 || command -v cargo >/dev/null 2>&1"; then
  failure "the container was not clean (an existing ~/.chaos, chaos, or cargo was found)"
else
  ok "no prior install to inherit: no ~/.chaos, no chaos on PATH, no cargo"
fi

# ---------------------------------------------------------------------------
# the fixture
# ---------------------------------------------------------------------------

# The fixture, the mirror and the request-log assertion live in scripts/ci/ so that
# the PowerShell lab offers the installer under test exactly the same release.
for helper in release-integrity-fixture.py release-integrity-serve.py \
            release-integrity-request-log.py; do
  if [ ! -f "${ci_src}/${helper}" ]; then
    echo "missing ${ci_src}/${helper}" >&2
    exit 2
  fi
  cp "${ci_src}/${helper}" "${WORK_DIR}/shared/${helper}"
done

in_container mkdir -p /lab
docker cp "${WORK_DIR}/." "${container_name}:/lab" >/dev/null

bump
# The same asset name install.sh would ask for on this machine, so the fixture is the
# one thing in the run that is not under test.
asset_name="$(in_container_sh "os=\$(uname -s | tr 'A-Z' 'a-z'); m=\$(uname -m); case \$m in x86_64|amd64) a=x64 ;; aarch64|arm64) a=arm64 ;; *) exit 1 ;; esac; echo \"chaos-\$os-\$a\"" || true)"
case "$asset_name" in chaos-*) ;; *) asset_name="" ;; esac
fixture_out="$(in_container_sh "cd /lab && python3 shared/release-integrity-fixture.py --root /lab --version ${version} --asset ${asset_name:-chaos-none} 2>&1" || true)"
if [ -n "$asset_name" ] \
  && in_container_sh "test -f /lab/releases/good/${asset_name} \
  && test -f /lab/releases/good/SHA256SUMS \
  && test -f /lab/releases/good/${asset_name}.sig"; then
  ok "fixture built for this host's asset: ${asset_name}"
else
  failure "the fixture did not build: $(printf '%s' "$fixture_out" | tail -2 | tr '\n' ' | ' | cut -c1-120)"
  echo "cannot continue" >&2
  exit 1
fi

pubkey="$(in_container_sh 'cat /lab/pubkey-ours' || true)"
other_pubkey="$(in_container_sh 'cat /lab/pubkey-other' || true)"
embedded_key="$(sed -n "s/^DEFAULT_SIGNING_PUBLIC_KEY='\([^']*\)'.*/\1/p" "$script_src")"

# A fixture that could not verify would turn every refusal below into a false pass, so
# its own signature is checked with the library directly, outside the installer.
bump
if in_container_sh "python3 -c \"
import base64
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
pk = Ed25519PublicKey.from_public_bytes(base64.b64decode(open('/lab/pubkey-ours').read()))
pk.verify(base64.b64decode(open('/lab/releases/good/${asset_name}.sig').read()),
          open('/lab/releases/good/${asset_name}', 'rb').read())
print('fixture signature verifies')
\""; then
  ok "the fixture's own signature verifies against its own key before any installer runs"
else
  failure "the fixture is not internally consistent; a refusal below could be explained by that instead"
fi
bump
if [ "$pubkey" != "$embedded_key" ] && [ "$other_pubkey" != "$embedded_key" ] \
  && [ "$pubkey" != "$other_pubkey" ]; then
  ok "three distinct keys are in play: the installer's built-in one, the fixture's, and a second valid one"
else
  failure "the fixture key and the built-in key are not distinct; a 'signature OK' here would be ambiguous"
fi

in_container_sh "nohup python3 /lab/shared/release-integrity-serve.py /lab/releases /lab/requests.log ${port} >/lab/serve.err 2>&1 &
for i in \$(seq 1 100); do
  curl -fsS -o /dev/null 'http://127.0.0.1:${port}/probe/https://github.com/o/r/releases/download/v${version}/${asset_name}' && exit 0
  sleep 0.2
done
exit 1" >/dev/null 2>&1 || true
bump
if in_container_sh "grep -q '^probe ${asset_name}\$' /lab/requests.log"; then
  ok "the fake release endpoint is listening on 127.0.0.1:${port}"
else
  failure "the fake release endpoint never answered: $(in_container_sh 'tr "\n" " " < /lab/serve.err 2>/dev/null | cut -c1-120')"
  echo "cannot continue" >&2
  exit 1
fi
# The probe was a request of its own; scenario checks count from here.
in_container_sh ': > /lab/requests.log'

# ---------------------------------------------------------------------------
# running the installer against it
# ---------------------------------------------------------------------------

# run_case <scenario> <tag> [extra docker-exec flags ...]
# Each scenario gets an empty CHAOS_HOME, so "installed" and "refused" have exactly one
# meaning: does bin/chaos exist and run.
run_case() {
  local case="$1" tag="$2"
  shift 2
  in_container_sh ": > /lab/requests.log; rm -rf /lab/home-${case}"
  docker exec \
    -e "CHAOS_HOME=/lab/home-${case}" \
    -e "CHAOS_VERSION=${version}" \
    -e "CHAOS_GITHUB_MIRROR=http://127.0.0.1:${port}/${case}" \
    -e "CHAOS_SIGNING_PUBLIC_KEY=${pubkey}" \
    "$@" \
    "$container_name" bash /lab/install.sh --no-path \
    >"${log_dir}/${tag}.log" 2>&1
}

installed_version() {
  in_container_sh "/lab/home-$1/bin/chaos --version 2>/dev/null"
}

# expect_refusal <scenario> <tag> <needle> [extra flags ...]
expect_refusal() {
  local case="$1" tag="$2" needle="$3"
  shift 3
  local rc=0
  bump
  run_case "$case" "$tag" "$@" || rc=$?
  if [ "$rc" -eq 0 ]; then
    failure "${tag}: exited 0; the installer accepted a release it should have refused"
  elif grep -q "$needle" "${log_dir}/${tag}.log"; then
    ok "${tag}: refused (exit ${rc}) -- $(grep -m1 "$needle" "${log_dir}/${tag}.log" | sed 's/^ *//' | cut -c1-76)"
  else
    failure "${tag}: failed (exit ${rc}) but not for the reason under test; no line matching '${needle}'"
    say "  last lines: $(tail -3 "${log_dir}/${tag}.log" | tr '\n' ' | ')"
  fi
  bump
  if [ -z "$(installed_version "$case")" ]; then
    ok "${tag}: nothing installed behind the refusal"
  else
    failure "${tag}: refused, yet bin/chaos is present and runnable"
  fi
}

header "the path a user actually takes"
rc=0
run_case good good || rc=$?
bump
if [ "$rc" -eq 0 ]; then
  ok "install from the fixture completed (exit 0)"
else
  failure "install from the fixture failed (exit ${rc})"
  say "  $(tail -5 "${log_dir}/good.log" | tr '\n' ' | ')"
fi
bump
if grep -q "checksum OK" "${log_dir}/good.log"; then
  ok "checksum OK, against SHA256SUMS served by the fixture"
else
  failure "no 'checksum OK' line: the published SHA256SUMS was not consulted"
fi
bump
if grep -q "signature OK" "${log_dir}/good.log"; then
  ok "signature OK, over the artifact bytes, under the key passed in the environment"
else
  failure "no 'signature OK' line"
fi
bump
if [ "$(installed_version good)" = "chaos ${version}" ]; then
  ok "what landed on disk runs and reports ${version}"
else
  failure "bin/chaos did not report ${version} (got: $(installed_version good))"
fi
bump
stored="chaos-${version}-$(in_container_sh "os=\$(uname -s | tr 'A-Z' 'a-z'); m=\$(uname -m); case \$m in x86_64|amd64) a=x86_64 ;; *) a=aarch64 ;; esac; if [ \"\$os\" = darwin ]; then os=macos; fi; echo \"\$os-\$a\"")"
if in_container_sh "test -L /lab/home-good/bin/chaos \
  && test \"\$(readlink /lab/home-good/bin/chaos)\" = '../downloads/${stored}' \
  && test -L /lab/home-good/bin/agent \
  && test -x /lab/home-good/downloads/${stored} \
  && test -L /lab/home-good/downloads/chaos-latest"; then
  ok "bin/chaos and bin/agent are relative symlinks to downloads/${stored}, with chaos-latest beside it"
else
  failure "the install layout is not what chaos update expects: $(in_container_sh 'ls -l /lab/home-good/bin /lab/home-good/downloads 2>&1 | tr "\n" " " | cut -c1-160')"
fi
bump
if grep -q "try: http://127.0.0.1:${port}/good/https://github.com/" "${log_dir}/good.log" \
  && ! grep -q "try: https://" "${log_dir}/good.log"; then
  ok "the fixture was the first candidate tried and no public mirror or origin was ever attempted"
else
  failure "the download order was not what CHAOS_GITHUB_MIRROR promises: $(grep -m3 'try:' "${log_dir}/good.log" | tr '\n' ' | ')"
fi
bump
if in_container_sh "python3 /lab/shared/release-integrity-request-log.py /lab/requests.log ${asset_name}"; then
  ok "the only files the fixture ever served were the artifact, SHA256SUMS and the sidecar"
else
  failure "unexpected requests reached the fixture: $(in_container_sh 'tr "\n" " " < /lab/requests.log')"
fi

bump
rc=0
second_out="$(docker exec \
  -e "CHAOS_HOME=/lab/home-good" \
  -e "CHAOS_VERSION=${version}" \
  -e "CHAOS_GITHUB_MIRROR=http://127.0.0.1:${port}/good" \
  -e "CHAOS_SIGNING_PUBLIC_KEY=${pubkey}" \
  "$container_name" bash /lab/install.sh --no-path 2>&1)" || rc=$?
if [ "$rc" -eq 0 ] && printf '%s' "$second_out" | grep -q "already installed"; then
  ok "a second run is a no-op instead of a second download"
else
  failure "a second run did not short-circuit (exit ${rc}): $(printf '%s' "$second_out" | tail -2 | tr '\n' ' ')"
fi

header "what a wrong byte does"
expect_refusal tampered tampered "checksum mismatch"

header "what a recomputed SHA256SUMS does"
# The digest now matches the tampered bytes, so the signature is the only thing left
# standing between a hostile mirror and the install -- and only if it covers the
# artifact rather than a manifest the same publisher controls.
expect_refusal forged-sums forged-sums "signature verification FAILED"

header "what a missing sidecar does"
expect_refusal no-sig no-sig "signature sidecar unavailable"

header "what a valid key that is not ours does"
expect_refusal good wrong-key "signature verification FAILED" \
  -e "CHAOS_SIGNING_PUBLIC_KEY=${other_pubkey}"

header "what a blank key does, and when"
# Fail closed, and fail before the download: a 150 MB transfer that ends in "no key" is
# the bug this scenario keeps from coming back.
bump
rc=0
run_case good blank-key -e "CHAOS_SIGNING_PUBLIC_KEY=" || rc=$?
if [ "$rc" -ne 0 ] && grep -q "CHAOS_SIGNING_PUBLIC_KEY is required" "${log_dir}/blank-key.log"; then
  ok "present-but-blank key is refused with the reason, not treated as unset"
else
  failure "a blank key did not produce the documented refusal (exit ${rc})"
fi
bump
if in_container_sh 'test ! -s /lab/requests.log'; then
  ok "it refused before requesting anything: the fixture served zero files"
else
  failure "the blank-key case downloaded something before refusing: $(in_container_sh 'tr "\n" " " < /lab/requests.log')"
fi

header "what an unusable manifest or a truncated transfer does"
expect_refusal no-sums-entry no-sums-entry "has no entry for"

# The HTML case proves two things at once: that the run refuses, and that it refused
# because the anti-HTML guard fired rather than because of a transport error.
bump
rc=0
run_case html-sums html-sums || rc=$?
if [ "$rc" -ne 0 ] && grep -q "could not fetch SHA256SUMS" "${log_dir}/html-sums.log"; then
  ok "html-sums: refused a manifest served as an HTML error page (exit ${rc})"
else
  failure "html-sums: did not refuse an HTML manifest (exit ${rc}): $(tail -3 "${log_dir}/html-sums.log" | tr '\n' ' | ')"
fi
bump
if grep -q "HTML response from" "${log_dir}/html-sums.log" \
  && [ -z "$(installed_version html-sums)" ]; then
  ok "html-sums: the guard that fired is the one that rejects proxy error pages, and nothing was installed"
else
  failure "html-sums: refused, but not by the HTML guard, or it installed anyway"
fi

expect_refusal empty-artifact empty-artifact "too small"

# The artifact floor is the one integrity decision the installers used to make
# differently: install.ps1 refused anything under 1 MiB, install.sh accepted it and let
# the checksum be the thing that complained, and install.bat used the size only to decide
# whether to sniff for HTML -- so a short non-HTML body fell through to certutil. All
# three now refuse it up front, and a future change to one has to answer for the others.
bump
sh_floor="$(sed -n 's/^USED_URL="\$(download_github "[^"]*" "\$TMP" [0-9]* [0-9]* \([0-9]*\))".*/\1/p' "$script_src")"
ps1_floor="$(sed -n 's/.*-OutFile \$tmp -Headers \$headers -MinBytes \(1MB\).*/\1/p' "${script_src%.sh}.ps1")"
bat_floor="$(sed -n 's/^if defined SZ if !SZ! LSS \([0-9]*\) (.*/\1/p' "${script_src%.sh}.bat" | head -1)"
if [ "$sh_floor" = "1048576" ] && [ "$ps1_floor" = "1MB" ] && [ "$bat_floor" = "1048576" ]; then
  ok "all three installers refuse an artifact under 1 MiB before hashing it (install.sh ${sh_floor}, install.ps1 ${ps1_floor}, install.bat ${bat_floor})"
else
  failure "the artifact floors have drifted: install.sh '${sh_floor}', install.ps1 '${ps1_floor}', install.bat '${bat_floor}'"
fi

header "what the two escape hatches actually cost"
# CHAOS_SKIP_CHECKSUM=1 is documented as leaving you trusting the download. That is only
# true if the signature still runs; if skipping one check quietly skipped the other, the
# hatch would be a way to install tampered bytes with a clear conscience.
bump
rc=0
run_case tampered skip-checksum -e CHAOS_SKIP_CHECKSUM=1 || rc=$?
if [ "$rc" -ne 0 ] \
  && grep -q "checksum verification skipped" "${log_dir}/skip-checksum.log" \
  && grep -q "signature verification FAILED" "${log_dir}/skip-checksum.log"; then
  ok "with the checksum skipped, the tampered artifact is still refused by the signature"
else
  failure "skipping the checksum also let the tampered artifact through (exit ${rc})"
fi
bump
rc=0
run_case good skip-signature -e CHAOS_SKIP_SIGNATURE=1 || rc=$?
if [ "$rc" -eq 0 ] && grep -q "signature verification skipped" "${log_dir}/skip-signature.log" \
  && grep -q "checksum OK" "${log_dir}/skip-signature.log"; then
  ok "skipping the signature alone still verifies the checksum"
else
  failure "CHAOS_SKIP_SIGNATURE=1 did not keep the checksum check running (exit ${rc})"
fi
bump
rc=0
run_case tampered skip-both -e CHAOS_SKIP_CHECKSUM=1 -e CHAOS_SKIP_SIGNATURE=1 || rc=$?
if [ "$rc" -eq 0 ] && [ -n "$(installed_version tampered)" ]; then
  note "both hatches together install a tampered artifact, which is what 'you are then trusting the download' means; no document in this repo sets either"
else
  failure "CHAOS_SKIP_CHECKSUM=1 with CHAOS_SKIP_SIGNATURE=1 did not install (exit ${rc}); the hatches no longer do what the help text says they do"
fi

header "summary"
say "${checks} checks run against a release this lab built itself, in a container with no route to the network"
if [ -n "$notes" ]; then
  say "notes:${notes}"
fi
if [ -n "$failures" ]; then
  printf '\nfailures:%s\n' "$failures" >&2
  exit 1
fi
echo "all ${checks} release-integrity checks passed"
