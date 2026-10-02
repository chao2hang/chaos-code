#!/usr/bin/env bash
# Release-integrity acceptance lab for the Windows installer, driven for real.
#
# scripts/install-integrity-in-docker.sh asks scripts/install.sh to install a release
# that is wrong in seven specific ways, and checks that each one is refused. The
# PowerShell installer is the one that a Windows user actually runs, and until now the
# strongest statement this repository could make about it was that it parses
# (scripts/ci/check-powershell-syntax.py). A check that has never been executed cannot
# have been seen to refuse anything.
#
# So this lab runs scripts/install.ps1 under pwsh against the same fixture release the
# shell lab uses -- same generator (scripts/ci/release-integrity-fixture.py), same
# eight scenarios, same signature over the artifact's exact bytes -- served by the same
# mirror shim, with the whole run inside a network namespace that has no route to
# anything but loopback. github.com is asserted unreachable before anything installs.
#
# What this covers is therefore the whole download-and-verify path: version and asset
# resolution, mirror candidate ordering, the minimum-size and HTML-error-page guards,
# SHA256SUMS parsing and comparison, the Get-FileHash digest, the ed25519 sidecar check
# through python, and where the bytes end up. What it does not cover is the genuinely
# Windows-only remainder: [RuntimeInformation] picking the asset on a real Windows host,
# executing a PE binary, and the user-PATH registry write. Those need a Windows runner,
# and -NoPath is passed so the registry step is skipped rather than imitated.
#
# Usage:
#   scripts/install-integrity-powershell.sh            # run the lab
#   scripts/install-integrity-powershell.sh --keep     # leave the lab files behind
#   CHAOS_PS1_LAB_DIR=DIR                              # build the fixture in DIR
#
# Needs: pwsh (PowerShell 7+), python3 with the cryptography package, and unshare
# (util-linux) able to create a network namespace -- as the calling user where the
# machine allows unprivileged user namespaces, otherwise as root via passwordless sudo,
# which is what Ubuntu 24.04 images need. Nothing is downloaded: there is no
# need to be able to reach the network for this to run, and it refuses to run if it
# could.

set -eu

version="9.9.9"
port=8098
asset="chaos-win32-x64.exe"
# Survives the re-exec into the network namespace, where --keep is not passed again.
keep="${keep:-0}"
script_dir="$(cd "$(dirname "$0")" && pwd)"
script_src="${script_dir}/install.ps1"
ci_src="${script_dir}/ci"
# Resolved before the namespace hop, because the inner half is the same file re-run
# with a bare environment and cannot look it up from a shell function there.
python3="$(command -v python3 || true)"

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
    --keep) keep=1; shift ;;
    --port) port="${2:-}"; shift 2 ;;
    -h|--help) sed -n '2,33p' "$0" | sed 's/^# \?//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

# ---------------------------------------------------------------------------
# the run, inside the namespace
# ---------------------------------------------------------------------------
# Everything below this point runs with no route off the machine. The outer half of
# this file only checks prerequisites, builds the fixture and enters the namespace.

# run_case <scenario> <tag> [extra NAME=value ...]
# Each scenario gets an empty CHAOS_HOME, so "installed" and "refused" have exactly one
# meaning: does bin/chaos.exe exist, and are its bytes the fixture's?
run_case() {
  local case="$1" tag="$2"
  shift 2
  : > "${WORK_DIR}/requests.log"
  rm -rf "${WORK_DIR}/home-${case}"
  mkdir -p "${WORK_DIR}/home-${case}" "${WORK_DIR}/tmp"
  # env -i on purpose. An inherited CHAOS_SIGNING_PUBLIC_KEY, CHAOS_VERSION or mirror
  # would let a check pass for a reason this run did not establish.
  #
  # PATH leads with a directory holding only a `python` shim, because install.ps1 calls
  # `python` (that is what the Windows installer puts on PATH) and this host only has
  # python3. TEMP is what install.ps1 uses for scratch, and is always present on
  # Windows.
  env -i \
    PATH="${WORK_DIR}/bin:/usr/local/bin:/usr/bin:/bin" \
    HOME="${WORK_DIR}/home-${case}" \
    TEMP="${WORK_DIR}/tmp" \
    CHAOS_HOME="${WORK_DIR}/home-${case}" \
    CHAOS_VERSION="${version}" \
    CHAOS_GITHUB_MIRROR="http://127.0.0.1:${port}/${case}" \
    CHAOS_SIGNING_PUBLIC_KEY="${pubkey}" \
    "$@" \
    pwsh -NoProfile -NonInteractive -File "${script_src}" -NoPath \
    > "${log_dir}/${tag}.log" 2>&1
}

# PowerShell wraps a thrown message to the console width and puts a "| " gutter on
# every continuation line, so a reason can span three lines. Matching a flattened copy
# keeps the needles about what the installer says, not about where it wrapped. The
# Write-Host lines -- progress and the checksum/signature verdicts -- are matched raw.
flat_log() {
  tr -d '\033' < "$1" | tr '\n' ' ' \
    | sed 's/\[[0-9;]*[mHKH]//g; s/ *| */ /g; s/(default)//g' | tr -s ' '
}

# install.ps1 names the file it writes after the command, not the asset: $BinName is
# chaos.exe whatever the release asset is called.
installed_digest() {
  sha256sum "${WORK_DIR}/home-$1/bin/chaos.exe" 2>/dev/null | cut -c1-64
}

landed_bytes_match_fixture() {
  [ -n "$(installed_digest "$1")" ] \
    && [ "$(installed_digest "$1")" = "$artifact_digest" ]
}

# expect_refusal <scenario> <tag> <needle> [extra NAME=value ...]
expect_refusal() {
  local case="$1" tag="$2" needle="$3" rc=0
  shift 3
  bump
  run_case "$case" "$tag" "$@" || rc=$?
  if [ "$rc" -eq 0 ]; then
    failure "${tag}: exited 0; the installer accepted a release it should have refused"
  elif flat_log "${log_dir}/${tag}.log" | grep -qi "$needle"; then
    ok "${tag}: refused (exit ${rc}) -- $(flat_log "${log_dir}/${tag}.log" | grep -oi -m1 "$needle" | cut -c1-76)"
  else
    failure "${tag}: failed (exit ${rc}) but not for the reason under test; no line matching '${needle}'"
    say "  last lines: $(tail -3 "${log_dir}/${tag}.log" | tr '\n' ' | ')"
  fi
  bump
  if [ -z "$(installed_digest "$case")" ]; then
    ok "${tag}: nothing installed behind the refusal"
  else
    failure "${tag}: refused, yet bin/chaos.exe is present with the fixture's bytes"
  fi
}

lab_body() {
  pubkey="$(cat "${WORK_DIR}/pubkey-ours")"
  other_pubkey="$(cat "${WORK_DIR}/pubkey-other")"
  artifact_digest="$(sha256sum "${WORK_DIR}/releases/good/${asset}" | cut -c1-64)"
  embedded_key="$(sed -n 's/^\$DefaultSigningPublicKey = "\([^"]*\)".*/\1/p' "$script_src")"
  log_dir="${WORK_DIR}/logs"
  mkdir -p "$log_dir" "${WORK_DIR}/bin" "${WORK_DIR}/tmp"
  printf '#!/bin/sh\nexec %s "$@"\n' "$python3" > "${WORK_DIR}/bin/python"
  chmod +x "${WORK_DIR}/bin/python"

  header "the machine doing the installing"
  say "pwsh:       $(pwsh -NoProfile -Command '$PSVersionTable.PSVersion.ToString()')"
  say "installer:  ${script_src} (working tree)"
  say "python:     ${python3} (reached through a \`python\` shim on PATH)"
  say "release:    ${WORK_DIR}/releases, version ${version} (never published)"
  say "network:    fresh network namespace; loopback only"

  bump
  # A fresh network namespace starts with loopback administratively down. Binding to
  # 127.0.0.1 still succeeds, so without this the fixture would look like a listener
  # that refuses connections rather than a machine that cannot leave itself.
  if ip link set lo up 2>/dev/null && ip -o addr show lo 2>/dev/null | grep -q '127.0.0.1'; then
    ok "loopback is up, which is all the fake release endpoint needs"
  else
    failure "loopback could not be brought up; nothing can be served to 127.0.0.1"
    echo "cannot continue" >&2
    exit 1
  fi
  bump
  offline="$(curl -sS --connect-timeout 3 --max-time 8 -o /dev/null https://github.com/ 2>&1; echo "exit=$?")"
  if printf '%s' "$offline" | grep -q "exit=6"; then
    ok "github.com cannot be resolved from this namespace (curl exit 6)"
  else
    failure "this namespace can reach the network (curl said: $(printf '%s' "$offline" | head -1 | cut -c1-90))"
    echo "cannot continue: an install that could reach the real feed proves nothing about the fixture" >&2
    exit 1
  fi
  bump
  if [ -f "${WORK_DIR}/releases/good/${asset}" ] \
    && [ -f "${WORK_DIR}/releases/good/SHA256SUMS" ] \
    && [ -f "${WORK_DIR}/releases/good/${asset}.sig" ]; then
    ok "fixture built for ${asset}: artifact, SHA256SUMS entry and signature"
  else
    failure "the fixture is incomplete; cannot continue"
    exit 1
  fi
  bump
  if "$python3" -c "
import base64, sys
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
pk = Ed25519PublicKey.from_public_bytes(base64.b64decode(open('${WORK_DIR}/pubkey-ours').read()))
pk.verify(base64.b64decode(open('${WORK_DIR}/releases/good/${asset}.sig').read()),
          open('${WORK_DIR}/releases/good/${asset}', 'rb').read())
"; then
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

  "$python3" "${ci_src}/release-integrity-serve.py" \
    "${WORK_DIR}/releases" "${WORK_DIR}/requests.log" "$port" &
  server_pid=$!
  if [ "$keep" != "1" ]; then
    cleanup() {
      kill "$server_pid" >/dev/null 2>&1 || true
      rm -rf "$WORK_DIR"
    }
  else
    cleanup() { kill "$server_pid" >/dev/null 2>&1 || true; say "lab files kept in ${WORK_DIR}"; }
  fi
  trap cleanup EXIT

  # Readiness is a real 200 for a real file, not merely a recorded request, so the
  # loop below cannot be satisfied by a listener that is up but misconfigured.
  ready=0
  i=0
  while [ "$i" -lt 100 ]; do
    if curl -fs --max-time 2 -o /dev/null \
      "http://127.0.0.1:${port}/good/https://github.com/o/r/releases/download/v${version}/${asset}" 2>/dev/null; then
      ready=1
      break
    fi
    i=$((i + 1))
    sleep 0.2
  done
  bump
  if [ "$ready" = "1" ]; then
    ok "the fake release endpoint answers 200 on 127.0.0.1:${port}"
  else
    failure "the fake release endpoint never served the fixture"
    exit 1
  fi
  # The readiness probe was a request of its own; scenario checks count from here.
  : > "${WORK_DIR}/requests.log"

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
  if landed_bytes_match_fixture good; then
    ok "the bytes that landed at bin/chaos.exe are the fixture's, digest for digest"
  else
    failure "bin/chaos.exe is absent or is not the artifact that was signed (got: $(installed_digest good))"
  fi
  bump
  if grep -q "try: http://127.0.0.1:${port}/good/https://github.com/" "${log_dir}/good.log" \
    && ! grep -q "try: https://" "${log_dir}/good.log"; then
    ok "the fixture was the first candidate tried and no public mirror or origin was ever attempted"
  else
    failure "the candidate order is not what the installer documents: $(grep -m3 'try:' "${log_dir}/good.log" | tr '\n' ' | ')"
  fi
  bump
  if "$python3" "${ci_src}/release-integrity-request-log.py" \
       "${WORK_DIR}/requests.log" "${asset}" >/dev/null; then
    ok "the only files the fixture ever served were the artifact, SHA256SUMS and the sidecar"
  else
    failure "$("$python3" "${ci_src}/release-integrity-request-log.py" "${WORK_DIR}/requests.log" "${asset}")"
  fi

  header "refusals, each checked twice: right reason, and nothing installed"
  expect_refusal tampered tampered "checksum mismatch"
  expect_refusal forged-sums forged_sums "signature verification FAILED"
  expect_refusal no-sig no_sig "download failed for https://github\.com/[^ ]*\.sig"
  expect_refusal good other_key "signature verification FAILED" CHAOS_SIGNING_PUBLIC_KEY="${other_pubkey}"
  expect_refusal no-sums-entry no_sums_entry "has no entry for ${asset}"
  expect_refusal html-sums html_sums "HTML response from"
  expect_refusal empty-artifact empty_artifact "too small"

  # The sidecar 404 is the case that used to be unreportable: the fixture answered, the
  # installer rejected the body, and every later candidate's DNS failure overwrote the
  # reason. The refusal is not enough -- the reason that names the 404 has to survive.
  bump
  if flat_log "${log_dir}/no_sig.log" | grep -q "why: .*404 (Not Found)"; then
    ok "the candidate that actually answered is still named: $(flat_log "${log_dir}/no_sig.log" | grep -o 'why: [^w]\{0,60\}' | head -1)"
  else
    failure "no_sig: the 404 from the fixture was buried under the later candidates' DNS failures"
  fi

  bump
  run_case good builtin_key_refused CHAOS_SIGNING_PUBLIC_KEY= || true
  if flat_log "${log_dir}/builtin_key_refused.log" | grep -q "CHAOS_SIGNING_PUBLIC_KEY is required" \
    && [ ! -f "${WORK_DIR}/home-good/bin/${asset}" ]; then
    ok "a set-but-blank public key is refused before any request is made: the fixture saw $(wc -l < "${WORK_DIR}/requests.log" | tr -d ' ') request(s)"
  else
    failure "a blank CHAOS_SIGNING_PUBLIC_KEY did not fail closed before downloading"
  fi

  header "what the two escape hatches actually cost"
  rc=0
  run_case tampered skip_checksum CHAOS_SKIP_CHECKSUM=1 || rc=$?
  if [ "$rc" -ne 0 ] && grep -q "signature verification FAILED" "${log_dir}/skip_checksum.log" \
    && [ -z "$(installed_digest tampered)" ]; then
    ok "with the checksum skipped, the tampered artifact is still refused by the signature"
  else
    failure "CHAOS_SKIP_CHECKSUM=1 did not leave the signature as the last line of defence (exit ${rc})"
  fi
  bump
  rc=0
  run_case good skip_signature CHAOS_SKIP_SIGNATURE=1 || rc=$?
  if [ "$rc" -eq 0 ] && grep -q "signature verification skipped" "${log_dir}/skip_signature.log" \
    && grep -q "checksum OK" "${log_dir}/skip_signature.log"; then
    ok "skipping the signature alone still verifies the checksum"
  else
    failure "CHAOS_SKIP_SIGNATURE=1 did not keep the checksum check running (exit ${rc})"
  fi
  bump
  rc=0
  run_case tampered skip_both CHAOS_SKIP_CHECKSUM=1 CHAOS_SKIP_SIGNATURE=1 || rc=$?
  if [ "$rc" -eq 0 ] && [ -n "$(installed_digest tampered)" ]; then
    note "both hatches together install a tampered artifact, which is what 'you are then trusting the download' means; no document in this repo sets either"
  else
    failure "CHAOS_SKIP_CHECKSUM=1 with CHAOS_SKIP_SIGNATURE=1 did not install (exit ${rc}); the hatches no longer do what the help text says they do"
  fi

  header "summary"
  say "${checks} checks run against a release this lab built itself, with no route to the network"
  if [ -n "$notes" ]; then
    say "notes:${notes}"
  fi
  if [ -n "$failures" ]; then
    printf '\nfailures:%s\n' "$failures" >&2
    exit 1
  fi
  echo "all ${checks} PowerShell release-integrity checks passed"
}

# ---------------------------------------------------------------------------
# the outer half: prerequisites, fixture, and one trip into the namespace
# ---------------------------------------------------------------------------

if [ "${CHAOS_PS1_LAB_NS:-0}" = "1" ]; then
  # lab_body exits non-zero itself on a failed premise; getting here means it was green.
  lab_body
  exit 0
fi

for tool in pwsh python3 unshare curl sha256sum ip; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "${tool} is required and was not found on PATH." >&2
    exit 2
  fi
done
if [ -z "$python3" ]; then
  echo "python3 is required and was not found on PATH." >&2
  exit 2
fi
if ! "$python3" -c 'from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey' >/dev/null 2>&1; then
  echo "python3 needs the cryptography package: install.ps1 verifies the release signature with it." >&2
  exit 2
fi
if [ ! -f "$script_src" ]; then
  echo "cannot find ${script_src}" >&2
  exit 2
fi
# What it takes to get a network namespace with no route but loopback. Unprivileged
# user-namespace creation is turned off on Ubuntu 24.04 -- GitHub-hosted runners among
# them, where `unshare -rn` fails writing /proc/self/uid_map -- while root can still
# create one. The root route is `unshare -n` and deliberately not `unshare -rn`: the
# `-r` would put the lab in a child user namespace, where a process has no privileges
# over anything outside it and could not even read this repository when the home
# directory is group-readable rather than world-readable. Root with a network namespace
# only keeps its normal access to the tree, and `ip link set lo up` inside the namespace
# still works because the namespace belongs to root's own user namespace. A machine that
# can do neither is told so and the lab does not run: that is the point of it.
# CHAOS_PS1_LAB_NS_ROUTE pins the route; CI can only take the root one, so a machine
# that allows both uses it to run that branch on purpose rather than trusting it.
ns_route="${CHAOS_PS1_LAB_NS_ROUTE:-}"
case "$ns_route" in
  "" | auto) ns_route="" ;;
  unshare | sudo) ;;
  *)
    echo "CHAOS_PS1_LAB_NS_ROUTE must be auto, unshare or sudo, not '${ns_route}'." >&2
    exit 2
    ;;
esac
# Whether each route is available on this machine.
ns_unshare_works=no
ns_sudo_works=no
unshare -rn true >/dev/null 2>&1 && ns_unshare_works=yes
command -v sudo >/dev/null 2>&1 && sudo -n unshare -n true >/dev/null 2>&1 && ns_sudo_works=yes
if [ -z "$ns_route" ]; then
  if [ "$ns_unshare_works" = "yes" ]; then
    ns_route="unshare"
  elif [ "$ns_sudo_works" = "yes" ]; then
    ns_route="sudo"
    say "this machine refuses unprivileged user namespaces, so the network namespace is made as root"
  else
    echo "cannot create a network namespace here: unshare -rn was refused and neither is passwordless sudo." >&2
    echo "The lab is only worth running if the installer provably cannot reach github.com." >&2
    exit 2
  fi
fi
# A pinned route is checked too, so asking for the root one on a machine that turns
# passwordless sudo off fails here rather than halfway through the run.
case "$ns_route" in
  unshare) [ "$ns_unshare_works" = "yes" ] || pinned_route_broken=yes ;;
  sudo) [ "$ns_sudo_works" = "yes" ] || pinned_route_broken=yes ;;
esac
if [ "${pinned_route_broken:-no}" = "yes" ]; then
  echo "CHAOS_PS1_LAB_NS_ROUTE=${ns_route} but that route is not available on this machine." >&2
  exit 2
fi

WORK_DIR="${CHAOS_PS1_LAB_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/chaos-install-integrity-ps1.XXXXXX")}"
export WORK_DIR
mkdir -p "${WORK_DIR}/releases"
for helper in release-integrity-fixture.py release-integrity-serve.py \
              release-integrity-request-log.py; do
  if [ ! -f "${ci_src}/${helper}" ]; then
    echo "missing ${ci_src}/${helper}" >&2
    exit 2
  fi
done
"$python3" "${ci_src}/release-integrity-fixture.py" \
  --root "$WORK_DIR" --version "$version" --asset "$asset" >/dev/null

echo "entering a network namespace with no route to anything but loopback"
if [ "$ns_route" = "sudo" ]; then
  # sudo does not carry the caller's environment across, so the three things the inner
  # half needs are named. The lab then runs as root, which is also what removes the lab
  # directory it wrote -- the cleanup trap lives in the inner half.
  sudo -n env CHAOS_PS1_LAB_NS=1 "keep=${keep}" "WORK_DIR=${WORK_DIR}" "PATH=${PATH}" \
    unshare -n bash "${script_dir}/install-integrity-powershell.sh" --port "$port"
else
  CHAOS_PS1_LAB_NS=1 keep="$keep" unshare -rn bash "${script_dir}/install-integrity-powershell.sh" \
    --port "$port"
fi
