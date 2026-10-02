#!/usr/bin/env bash
# Run the M4 remote-workspace acceptance gate against a clean Linux host.
#
# The unit and integration tests for `chaos-remote-server` / `chaos-remote` run on
# the machine that wrote them, against a server that machine started. That proves
# the code works; it does not prove the *documented deployment* works, because a
# developer box already has git, a warm cargo cache, a permissive filesystem and a
# shell full of tools. This script puts the server on a stock Debian container that
# has never seen the repository, reaches it through a tunnel the way a real remote
# session does, and checks what M4.6 asks about: deployment, version negotiation,
# reading, searching, writing, git diff, tool execution, credential handling, path
# escape, a failed upgrade, a dropped connection, an out-of-space remote disk and the
# two clocks a session runs on -- the wait for a transport that is not up yet, and
# the deadline on a reply that never comes.
#
# Two further containers stand in for hostile hosts rather than clean ones: one
# whose disk fills mid-upload, and one whose workspace is mounted `noexec`, so that
# an artifact which can never start is refused instead of published.
#
# Topology. The transport dials loopback only -- deliberately, because the tunnel is
# what carries the security (ADR-004). Both containers therefore use `--network
# host`, and `socat` on this machine is the tunnel:
#
#   chaos-remote (dev container) -> 127.0.0.1:$TUNNEL_PORT
#       -> socat on this machine  -> 127.0.0.1:$SERVER_PORT
#           -> chaos-remote-server (host container, standing in for the remote box)
#
# Killing socat is how a dropped network is produced: nothing else about the setup
# changes between a working session and a broken one.
#
# Usage:
#   scripts/remote-acceptance-in-docker.sh              # run everything
#   scripts/remote-acceptance-in-docker.sh --keep       # leave containers + dir
#
# Environment:
#   IMAGE      image standing in for the remote host (default chaos-verify:local;
#              any Debian-family image with git works -- the server shells out to
#              git for `diff`)
#   WORK_DIR   where the two machine roots live (default: a fresh mktemp dir)
#   CARGO_ARGS extra args for the artifact build (default: --offline --locked)
#
# Capture evidence with:
#   scripts/remote-acceptance-in-docker.sh 2>&1 | tee remote-acceptance-$(date +%Y%m%d).log
set -euo pipefail

IMAGE="${IMAGE:-chaos-verify:local}"
WORK_DIR="${WORK_DIR:-}"
CARGO_ARGS="${CARGO_ARGS:---offline --locked}"
KEEP=0

for arg in "$@"; do
  case "$arg" in
    --keep) KEEP=1 ;;
    -h | --help)
      sed -n '2,39p' "$0"
      exit 0
      ;;
    *)
      echo "unknown argument: $arg (expected --keep or --help)" >&2
      exit 2
      ;;
  esac
done

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
run_id="$$"
host_container="chaos-remote-lab-host-${run_id}"
dev_container="chaos-remote-lab-dev-${run_id}"
disk_container="chaos-remote-lab-disk-${run_id}"
noexec_container="chaos-remote-lab-noexec-${run_id}"
port_base=$((20000 + RANDOM % 17000))
server_port="${port_base}"
tunnel_port="$((port_base + 1))"
server2_port="$((port_base + 2))"
tunnel2_port="$((port_base + 3))"
expiry_port="$((port_base + 4))"
# A port whose tunnel appears only later, and one where a peer accepts and then
# says nothing. Both feed the checks about the two waits a session can be given.
later_port="$((port_base + 5))"
silent_port="$((port_base + 6))"

tunnel_pid=""
tunnel2_pid=""
silent_pid=""
failures=""
checks=0
capture_file=""
capture_status=0
session_status=0

if ! docker image inspect "${IMAGE}" >/dev/null 2>&1; then
  echo "image ${IMAGE} not found; build it with:" >&2
  echo "  docker build -f docker/verify.Dockerfile -t ${IMAGE} ${repo_root}" >&2
  exit 2
fi
if ! command -v socat >/dev/null 2>&1; then
  echo "socat is required: it is the tunnel between the two machines." >&2
  exit 2
fi

if [ -z "${WORK_DIR}" ]; then
  WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/chaos-remote-lab.XXXXXX")"
  work_dir_created=1
else
  mkdir -p "${WORK_DIR}"
  work_dir_created=0
fi
WORK_DIR="$(cd "${WORK_DIR}" && pwd -P)"
# One directory per run, for the same reason: a run whose containers have already
# written credentials, git state and installed artifacts into it cannot be reused.
lab_root="${WORK_DIR}/run-${run_id}"
served_host_dir="${lab_root}/host/workspace"
# The same directory seen from the two sides of the tunnel.
served_in="/lab/workspace"
installed_in="${served_in}/.chaos-server"
# `work` stays outside the bind mounts: the containers write as root, and this is
# the script's own scratch for captured output.
mkdir -p "${lab_root}/host/bin" "${served_host_dir}" "${lab_root}/host/workspace2" \
  "${lab_root}/dev/bin" "${lab_root}/dev/local" "${lab_root}/shared" "${lab_root}/disk" \
  "${lab_root}/noexec" "${lab_root}/work"
capture_file="${lab_root}/work/capture.txt"
log_dir="${lab_root}/work"

log() { printf '\n== %s\n' "$*"; }
say() { printf '   %s\n' "$*"; }
show_capture() {
  if [ -n "${capture_file}" ] && [ -f "${capture_file}" ]; then
    sed 's/^/     | /' "${capture_file}" | head -n 12 >&2
  fi
}
# A failed check is recorded and the run continues: one run should report every
# problem, not stop at the first.
failure() {
  failures="${failures}
  - $1"
  printf '   FAILED %s\n' "$1" >&2
  show_capture
  return 0
}

stop_tunnels() {
  # `silent_pid` is the mute peer, not a tunnel, but it is torn down the same way
  # and for the same reason: `fork` leaves a child per accepted connection.
  for pid in "${tunnel_pid}" "${tunnel2_pid}" "${silent_pid}"; do
    if [ -n "${pid}" ]; then
      # `socat ... fork` leaves a child per established connection; killing only
      # the parent would leave an open session and the run would prove nothing.
      for child in $(pgrep -P "${pid}" 2>/dev/null || true); do
        kill "${child}" 2>/dev/null || true
      done
      kill "${pid}" 2>/dev/null || true
    fi
  done
  tunnel_pid=""
  tunnel2_pid=""
}

teardown() {
  stop_tunnels
  if [ "${KEEP}" = "1" ]; then
    echo
    echo "kept: containers ${host_container} ${dev_container} ${disk_container} \
${noexec_container}, dir ${WORK_DIR}"
    return
  fi
  # Everything the containers wrote through a bind mount belongs to root inside
  # them, so the delete happens there; a host `rm -rf` would fail on it.
  for container in "${host_container}" "${dev_container}" "${disk_container}" \
    "${noexec_container}"; do
    if [ -n "$(docker container inspect -f '{{.Id}}' "${container}" 2>/dev/null || true)" ]; then
      docker exec "${container}" bash -c 'rm -rf /lab /shared' >/dev/null 2>&1 || true
    fi
  done
  docker rm -f "${host_container}" "${dev_container}" "${disk_container}" \
    "${noexec_container}" >/dev/null 2>&1 || true
  if [ "${work_dir_created}" = "1" ]; then
    rm -rf "${WORK_DIR}"
  else
    # A directory the caller named is theirs, not ours: take back only the four
    # subdirectories this script lays down.
    rm -rf "${lab_root}"
  fi
}
trap teardown EXIT

on_host() { docker exec "${host_container}" bash -c "$*"; }
on_host_d() { docker exec -d "${host_container}" bash -c "$*"; }
on_dev() { docker exec "${dev_container}" bash -c "$*"; }

# The server removes a credential from its published file as it redeems one, and
# the client removes it from the file it was pointed at. Handing the client some
# credentials is the operator's job, so that is what these do: top the local copy
# up from the server's live list whenever it runs out.
# Two things shape this. The credential files are mode 0600 owned by the
# container's root, so the copy is made there. And the server never rewrites the
# file it published, so the copy has to advance through it: re-reading the first
# line would hand back a credential that already opened a session.
handed=0
hand_out() {
  local from="$1" sink="$2" count="$3" start=$((handed + 1))
  on_host "sed -n '${start},$((start + count - 1))p' /shared/${from} >/shared/${sink}"
  handed=$((handed + count))
  return 0
}
top_up() {
  if [ ! -s "${lab_root}/shared/$2" ]; then
    hand_out "$1" "$2" 8
  fi
  return 0
}
# A credential that has never been handed to any client, for the tests that need to
# watch one being spent.
fresh_token() {
  local token
  token="$(on_host "sed -n '$((handed + 1))p' /shared/tokens")"
  handed=$((handed + 1))
  printf '%s\n' "${token}"
}
remote() {
  top_up tokens token
  on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${tunnel_port} --token-file /shared/token $*"
}
remote2() {
  top_up tokens2 token2
  on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${tunnel2_port} --token-file /shared/token2 $*"
}
in_disk() { docker exec "${disk_container}" bash -c "$*"; }
on_disk() {
  docker exec "${disk_container}" bash -c \
    "if [ ! -s /shared/token-disk ]; then head -n 8 /shared/tokens-disk >/shared/token-disk; fi
     /lab/chaos-remote --unix /shared/disk.sock --token-file /shared/token-disk $*"
}
in_noexec() { docker exec "${noexec_container}" bash -c "$*"; }
on_noexec() {
  docker exec "${noexec_container}" bash -c \
    "if [ ! -s /shared/token-noexec ]; then head -n 8 /shared/tokens-noexec >/shared/token-noexec; fi
     /lab/chaos-remote --unix /shared/noexec.sock --token-file /shared/token-noexec $*"
}

# Runs a command, keeps its combined output, and returns its status without letting
# `set -e` end the run. The assertion helpers below read the captured text.
run_capture() {
  set +e
  "$@" >"${capture_file}" 2>&1
  capture_status=$?
  set -e
  return 0
}

expect_ok() {
  local label="$1"
  shift
  checks=$((checks + 1))
  run_capture "$@"
  if [ "${capture_status}" = "0" ]; then
    say "ok  ${label}"
  else
    failure "${label} (exit ${capture_status})"
  fi
  return 0
}

expect_output() {
  local label="$1" needle="$2"
  shift 2
  checks=$((checks + 1))
  run_capture "$@"
  if [ "${capture_status}" = "0" ] && grep -q -- "${needle}" "${capture_file}"; then
    say "ok  ${label}"
  else
    failure "${label} (exit ${capture_status}, wanted \"${needle}\")"
  fi
  return 0
}

expect_refused() {
  local label="$1" needle="$2"
  shift 2
  checks=$((checks + 1))
  run_capture "$@"
  if [ "${capture_status}" != "0" ] && grep -qi -- "${needle}" "${capture_file}"; then
    say "ok  ${label}"
  else
    failure "${label} (exit ${capture_status}, wanted refusal \"${needle}\")"
  fi
  return 0
}

expect_status() {
  local label="$1" want="$2"
  shift 2
  checks=$((checks + 1))
  run_capture "$@"
  if [ "${capture_status}" = "${want}" ]; then
    say "ok  ${label}"
  else
    failure "${label} (exit ${capture_status}, wanted ${want})"
  fi
  return 0
}

# A statement about text the lab already captured, or about a file on this machine.
check_text() {
  local label="$1" file="$2" needle="$3"
  checks=$((checks + 1))
  if grep -qi -- "${needle}" "${file}"; then
    say "ok  ${label}"
  else
    failure "${label} (\"${needle}\" not found)"
  fi
  return 0
}

check_absent() {
  local label="$1" file="$2" needle="$3"
  checks=$((checks + 1))
  if grep -qi -- "${needle}" "${file}"; then
    failure "${label} (found \"${needle}\")"
  else
    say "ok  ${label}"
  fi
  return 0
}

# A statement about the remote disk, evaluated where that disk is.
check_on_host() {
  local label="$1" command_line="$2"
  checks=$((checks + 1))
  if on_host "${command_line}" >"${capture_file}" 2>&1; then
    say "ok  ${label}"
  else
    failure "${label}"
  fi
  return 0
}

check_in_disk() {
  local label="$1" command_line="$2"
  checks=$((checks + 1))
  if docker exec "${disk_container}" bash -c "${command_line}" >"${capture_file}" 2>&1; then
    say "ok  ${label}"
  else
    failure "${label}"
  fi
  return 0
}

check_in_noexec() {
  local label="$1" command_line="$2"
  checks=$((checks + 1))
  if docker exec "${noexec_container}" bash -c "${command_line}" >"${capture_file}" 2>&1; then
    say "ok  ${label}"
  else
    failure "${label}"
  fi
  return 0
}

wait_for_file() {
  local path="$1" what="$2" tries=0
  while [ ! -s "${path}" ]; do
    tries=$((tries + 1))
    if [ "${tries}" -gt 80 ]; then
      echo "timed out waiting for ${what}: ${path}" >&2
      for logfile in "${lab_root}"/shared/server*.log; do
        if [ -f "${logfile}" ]; then
          sed 's/^/  | /' "${logfile}" >&2 || true
        fi
      done
      on_host 'tail -n 40 /shared/server.log' >&2 || true
      exit 1
    fi
    sleep 0.25
  done
  return 0
}

start_tunnel() {
  socat "TCP-LISTEN:${1},reuseaddr,fork" "TCP:127.0.0.1:${2}" >/dev/null 2>&1 &
  echo $!
}

# ---------------------------------------------------------------- artifacts ----

log "building the artifacts on this machine"
(cd "${repo_root}" && cargo build -p chaos-engine --bins ${CARGO_ARGS})
cp "${repo_root}/target/debug/chaos-remote-server" "${lab_root}/host/bin/"
cp "${repo_root}/target/debug/chaos-remote-server" "${lab_root}/dev/bin/"
cp "${repo_root}/target/debug/chaos-remote" "${lab_root}/dev/bin/"

# The workspace the remote host will serve, laid down from this machine so the run
# is reproducible: one committed state plus one uncommitted change, because `diff`
# is one of the things under test.
git -C "${served_host_dir}" init -q
git -C "${served_host_dir}" config user.email lab@example.invalid
git -C "${served_host_dir}" config user.name "Remote Lab"
printf 'buildbox\n' >"${served_host_dir}/README.md"
printf 'first line\nsecond needle line\nthird line\n' >"${served_host_dir}/notes.txt"
mkdir -p "${served_host_dir}/src"
printf 'fn main() {}\n' >"${served_host_dir}/src/main.rs"
git -C "${served_host_dir}" add -A
git -C "${served_host_dir}" commit -qm 'initial state'
printf 'a line that only exists in the working tree\n' >>"${served_host_dir}/README.md"
printf 'the second workspace\n' >"${lab_root}/host/workspace2/other.txt"

log "starting the clean remote host container (${IMAGE})"
docker run -d --name "${host_container}" --network host \
  -v "${lab_root}/host:/lab" -v "${lab_root}/shared:/shared" \
  "${IMAGE}" sleep 3600 >/dev/null
docker run -d --name "${dev_container}" --network host \
  -v "${lab_root}/dev:/lab" -v "${lab_root}/shared:/shared" \
  "${IMAGE}" sleep 3600 >/dev/null
say "remote host: $(on_host 'sed -n 2p /etc/os-release'), $(on_host uname -m)"
say "its only chaos binaries are the ones copied in: $(on_host 'ls /lab/bin | tr "\n" " "')"

# The bind mount crosses a uid boundary: this machine's user owns the files and the
# container's root serves them, and git refuses to open a repository owned by
# somebody else. A real remote host runs the server as the workspace owner, so this
# is an artifact of the lab rather than of the transport; declaring the exception is
# how scripts/verify-in-docker.sh handles the same mount.
on_host 'git config --global --add safe.directory "*"'
check_on_host "the remote host's git will read the served repository" \
  "git -C ${served_in} status --short"

# ------------------------------------------------------------- deployment -----

log "the artifacts run on a stock Linux that has never seen this repository"
expect_output "the server artifact starts and reports a version" '.' \
  on_host '/lab/bin/chaos-remote-server --version'
expect_output "the client artifact starts" 'drive one remote workspace' \
  on_dev '/lab/bin/chaos-remote --help'

log "a routable address is refused where it is parsed, on both sides"
expect_refused "the server refuses 0.0.0.0 as a listen address" "loopback" \
  on_host "/lab/bin/chaos-remote-server --workspace ${served_in} --tcp 0.0.0.0:${server_port}"
expect_refused "the client refuses a routable dial address" "loopback" \
  on_dev "/lab/bin/chaos-remote --tcp 203.0.113.7:22 --token 00000000000000000000000000000000 ping"
expect_refused "a server with no transport is refused instead of guessing" "unix\|tcp" \
  on_host "/lab/bin/chaos-remote-server --workspace ${served_in}"

log "starting the server, loopback-only, on the remote host"
on_host_d "exec /lab/bin/chaos-remote-server \
  --workspace ${served_in} \
  --tcp 127.0.0.1:${server_port} \
  --token-file /shared/tokens \
  --capability tool-execution --allow echo --allow sleep --allow false \
  --tokens 256 --token-ttl 3600 \
  >/shared/server.log 2>&1"
wait_for_file "${lab_root}/shared/tokens" "the published credentials"
say "credentials published: $(on_host 'wc -l < /shared/tokens' | tr -d ' ')"
expect_output "the listener is reachable on loopback" "open" \
  on_host "(exec 3<>/dev/tcp/127.0.0.1/${server_port}) 2>/dev/null && echo open || echo closed"

tunnel_pid="$(start_tunnel "${tunnel_port}" "${server_port}")"
sleep 0.5

# --------------------------------------------------------- the happy path -----

log "version negotiation and the capabilities this session was granted"
expect_output "ping reports the negotiated protocol" "protocol" remote 'ping'
expect_output "info reports the server build" '"server"' remote 'info --json'
check_text "info grants write" "${capture_file}" '"write"'
check_text "info grants tool-execution" "${capture_file}" 'tool-execution'

log "listing, reading and searching the remote workspace"
expect_output "list shows the tracked files" "README.md" remote 'list --depth 3'
check_absent "list does not expose the .git directory" "${capture_file}" '\.git'
expect_output "cat returns the committed bytes" "buildbox" remote 'cat README.md'
expect_output "search reports a 1-based line number" "notes.txt:2" remote 'search needle'
on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${tunnel_port} --token-file /shared/token \
  read notes.txt --out /lab/local/fetched.txt" >/dev/null
checks=$((checks + 1))
remote_digest="$(on_host 'sha256sum /lab/workspace/notes.txt' | cut -c1-64)"
fetched_digest="$(on_dev 'sha256sum /lab/local/fetched.txt' | cut -c1-64)"
if [ "${remote_digest}" = "${fetched_digest}" ]; then
  say "ok  a read delivered the same bytes that are on the remote disk"
else
  failure "the read delivered ${fetched_digest}, the remote file is ${remote_digest}"
fi
expect_output "a Range read returns the requested window" "second needle line" \
  remote 'read notes.txt --offset 11 --length 19'

log "a write through the tunnel lands on the remote disk"
printf 'written from the developer machine\n' >"${lab_root}/dev/local/note.txt"
expect_output "write reports the bytes and the digest it stored" 'wrote .*bytes.*sha256' \
  remote 'write inbox/note.txt --from /lab/local/note.txt --mkdir'
check_on_host "the file is on the remote host with those bytes" \
  "grep -q 'written from the developer machine' ${served_in}/inbox/note.txt"
expect_refused "a stale digest is refused instead of overwriting" "changed since\|conflict" \
  remote 'write inbox/note.txt --text "overwrite" --expect deadbeef'
check_on_host "the refused write left the file untouched" \
  "grep -q 'written from the developer machine' ${served_in}/inbox/note.txt"

log "git diff is produced by the remote host's own git"
expect_output "diff shows the uncommitted change" "only exists in the working tree" \
  remote 'diff'

log "tool execution, and the edge of the allowlist"
expect_output "an allowlisted program runs remotely" "hello from buildbox" \
  remote 'exec -- echo hello from buildbox'
expect_status "the remote program's exit status is propagated" 1 remote 'exec -- false'
expect_refused "a program that was not allowlisted is refused" "allow" remote 'exec -- rm -rf /'
expect_status "a program that hits its timeout is killed" 124 \
  remote 'exec --timeout 1 -- sleep 30'

log "paths that leave the served workspace are refused, not resolved"
for attempt in 'cat ../../etc/os-release' 'read /etc/passwd' 'cat ../../../etc/hostname' \
  'write ../escape.txt --text x --mkdir' 'write /etc/escape.txt --text x'; do
  expect_refused "refused: ${attempt}" "reject\|outside\|refus\|absolute" remote "${attempt}"
done

# ------------------------------------------------------- credentials ----------

log "credentials are one-time: a replayed one is refused"
used="$(fresh_token)"
expect_ok "the credential is accepted once" \
  on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${tunnel_port} --token '${used}' ping"
expect_refused "the same credential refused a second time" \
  "one-time\|already opened\|unknown\|invalid\|reject\|unauthor" \
  on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${tunnel_port} --token '${used}' ping"
expect_refused "a credential the server never issued is refused" "not issued\|unknown\|invalid\|reject" \
  on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${tunnel_port} --token 00000000000000000000000000000000 ping"

log "a credential past its ttl is refused"
on_host_d "exec /lab/bin/chaos-remote-server --workspace ${served_in} \
  --tcp 127.0.0.1:${expiry_port} --token-file /shared/tokens-expiry \
  --token-ttl 1 --tokens 2 >/shared/server-expiry.log 2>&1"
wait_for_file "${lab_root}/shared/tokens-expiry" "the short-lived credentials"
expiry="$(on_host 'head -n 1 /shared/tokens-expiry')"
sleep 3
# The server sweeps stale credentials before it redeems anything, so this is the
# answer a client that merely connected late gets. It must not be the replay
# message: being accused of reuse when the credential was never used sends
# whoever reads it looking for a leak that does not exist.
run_capture on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${expiry_port} --token '${expiry}' ping"
checks=$((checks + 1))
if [ "${capture_status}" != "0" ] && grep -qi expired "${capture_file}" \
  && ! grep -qi 'already opened' "${capture_file}"; then
  say "ok  an expired credential is refused as expired, not as a replay"
else
  failure "an expired credential was not reported as expired (exit ${capture_status}): \
$(tr '\n' ' ' <"${capture_file}")"
fi

log "the server log carries no credential material and no errors"
on_host 'cat /shared/server.log' >"${log_dir}/server.log" 2>/dev/null || :
# Anything shaped like a credential counts, not only the ones still in the file:
# a redeemed credential is gone from the file but was still issued.
check_on_host "no credential-shaped secret appears in the server log" \
  '! grep -Eq "[0-9a-f]{64}" /shared/server.log'
check_on_host "no live credential appears in the server log" \
  '! grep -qf /shared/tokens /shared/server.log'
check_absent "no authorization header material in the log" \
  "${log_dir}/server.log" "authorization\|bearer"
check_absent "the server reported no error doing any of the above" \
  "${log_dir}/server.log" "error\|panic"

# --------------------------------------------------- capability boundaries ----

log "capabilities this build does not implement are named, not faked"
for missing in interactive-pty port-forward detached-agent; do
  expect_refused "--capability ${missing} is refused by the client" "not\|unsupported\|refus" \
    on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${tunnel_port} --token-file /shared/token \
      --capability ${missing} ping"
done
expect_refused "an unknown capability name is refused" "capabilit" \
  on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${tunnel_port} --token-file /shared/token \
    --capability telepathy ping"

log "a server started with --no-write really cannot be written to"
on_host_d "exec /lab/bin/chaos-remote-server --workspace /lab/workspace2 \
  --tcp 127.0.0.1:${server2_port} --token-file /shared/tokens2 \
  --no-write --tokens 32 --token-ttl 3600 >/shared/server2.log 2>&1"
wait_for_file "${lab_root}/shared/tokens2" "the read-only server's credentials"
tunnel2_pid="$(start_tunnel "${tunnel2_port}" "${server2_port}")"
sleep 0.5
expect_output "the read-only server answers" '"capabilities"' remote2 'info --json'
checks=$((checks + 1))
if grep -q '"write"' "${capture_file}"; then
  failure "--no-write still offered the write capability"
else
  say "ok  write is absent from the capabilities it granted"
fi
expect_refused "a write against the read-only server is refused" "capab\|grant\|refus" \
  remote2 'write blocked.txt --text nope'

log "two remote workspaces keep their own state"
expect_output "the second server lists only its own file" "other.txt" remote2 'list'
expect_refused "the second server cannot read the first workspace" \
  "not found\|no such\|reject\|outside" remote2 'cat notes.txt'
expect_output "the first server still reads its own" "buildbox" remote 'cat README.md'
# Credentials are part of that state: a credential one server published must not
# open a session at the other, or which workspace you are driving depends on which
# file you happened to read the line out of.
cross="$(fresh_token)"
expect_refused "a credential published by one server opens nothing at the other" \
  "not issued\|unauthor\|refus" \
  on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${tunnel2_port} --token '${cross}' ping"

# ------------------------------------------------------------- upgrades -------

log "deploying a new server build over the session"
cp "${lab_root}/dev/bin/chaos-remote-server" "${lab_root}/dev/local/server-9.9.9"
checks=$((checks + 1))
run_capture remote 'install 9.9.9 --from /lab/local/server-9.9.9'
if [ "${capture_status}" = "0" ] && grep -q '9.9.9' "${capture_file}" &&
  grep -q 'still the previous build' "${capture_file}"; then
  say "ok  install publishes 9.9.9 and says the running process is still the old build"
else
  failure "install did not report the deployment as expected (exit ${capture_status})"
  show_capture
fi
check_on_host "the pointer on the remote host selects 9.9.9" \
  "test \"\$(tr -d '\n' <${installed_in}/current)\" = 9.9.9"
check_on_host "the version is recorded next to the artifact" \
  "test \"\$(tr -d '\n' <${installed_in}/9.9.9/VERSION)\" = 9.9.9"
checks=$((checks + 1))
deployed_digest="$(on_host "sha256sum ${installed_in}/9.9.9/chaos-remote-server" | cut -c1-64)"
uploaded_digest="$(sha256sum "${lab_root}/dev/local/server-9.9.9" | cut -c1-64)"
if [ "${deployed_digest}" = "${uploaded_digest}" ]; then
  say "ok  the deployed artifact is byte-identical to the uploaded one"
else
  failure "the deployed artifact is ${deployed_digest}, the upload was ${uploaded_digest}"
fi
check_on_host "the deployed artifact starts on the remote host" \
  "${installed_in}/9.9.9/chaos-remote-server --version"

log "an upload that fails part-way leaves the installed version alone"
# The remote disk is filled until the next artifact cannot fit, so the staging
# write fails for real instead of on a timer that could land anywhere.
docker run -d --name "${disk_container}" --network host \
  -v "${lab_root}/disk:/lab" -v "${lab_root}/shared:/shared" \
  --tmpfs "/lab/workspace:size=32m,exec" \
  "${IMAGE}" sleep 3600 >/dev/null
cp "${repo_root}/target/debug/chaos-remote-server" "${lab_root}/disk/chaos-remote-server"
cp "${repo_root}/target/debug/chaos-remote" "${lab_root}/disk/chaos-remote"
head -c 3000000 /dev/zero >"${lab_root}/disk/blob"
docker exec -d "${disk_container}" bash -c \
  "exec /lab/chaos-remote-server --workspace /lab/workspace --unix /shared/disk.sock \
   --token-file /shared/tokens-disk --tokens 16 --token-ttl 3600 \
   >/shared/server-disk.log 2>&1"
wait_for_file "${lab_root}/shared/tokens-disk" "the deployment server's credentials"
expect_output "the first deployment fits" '9.9.9' \
  on_disk 'install 9.9.9 --from /lab/chaos-remote-server'
expect_output "a small file is written before the disk fills" 'wrote' \
  on_disk 'write keep.txt --text "the file to keep"'
docker exec "${disk_container}" bash -c 'head -c 28900000 /dev/zero > /lab/workspace/filler'
expect_refused "a deployment that no longer fits is refused" "no space\|os error 28\|space" \
  on_disk 'install 8.8.8 --from /lab/chaos-remote-server'
check_in_disk "no version directory was published for the failed upload" \
  "test ! -e /lab/workspace/.chaos-server/8.8.8"
check_in_disk "the pointer still selects the version that was working" \
  "test \"\$(tr -d '\n' </lab/workspace/.chaos-server/current)\" = 9.9.9"
expect_output "the installed artifact still starts after the failed upgrade" '.' \
  in_disk '/lab/workspace/.chaos-server/9.9.9/chaos-remote-server --version'
expect_refused "a write that no longer fits is refused" "no space\|os error 28\|space" \
  on_disk 'write big.bin --from /lab/blob'
check_in_disk "the failed write left no partial file behind" \
  "test ! -e /lab/workspace/big.bin"
expect_output "the file written before the disk filled is still readable" "the file to keep" \
  on_disk 'cat keep.txt'

log "an artifact the host will not execute is refused rather than published"
# A workspace on a volume mounted `noexec` is an ordinary arrangement — container
# volumes, hardened hosts — and the artifact copied into it keeps a perfectly good
# 0755 mode. Publishing that would leave the host believing it runs a build it can
# never start, so `install` asks the kernel the question the next start will ask.
docker run -d --name "${noexec_container}" --network host \
  -v "${lab_root}/noexec:/lab" -v "${lab_root}/shared:/shared" \
  --tmpfs /lab/workspace:size=32m \
  "${IMAGE}" sleep 3600 >/dev/null
cp "${repo_root}/target/debug/chaos-remote-server" "${lab_root}/noexec/chaos-remote-server"
cp "${repo_root}/target/debug/chaos-remote" "${lab_root}/noexec/chaos-remote"
check_in_noexec "the workspace is a filesystem the host will not execute from" \
  "grep -q ' /lab/workspace .*noexec' /proc/mounts"
check_in_noexec "and a 0755 file in it really is unrunnable" \
  "cp /lab/chaos-remote-server /lab/workspace/probe && chmod 755 /lab/workspace/probe \
   && ! test -x /lab/workspace/probe"
docker exec -d "${noexec_container}" bash -c \
  "exec /lab/chaos-remote-server --workspace /lab/workspace --unix /shared/noexec.sock \
   --token-file /shared/tokens-noexec --tokens 16 --token-ttl 3600 \
   >/shared/server-noexec.log 2>&1"
wait_for_file "${lab_root}/shared/tokens-noexec" "the noexec host's credentials"
expect_refused "an install the host could never start is refused" "execut\|noexec" \
  on_noexec 'install 7.7.7 --from /lab/chaos-remote-server'
check_in_noexec "no pointer was published for an artifact that cannot run" \
  "test ! -e /lab/workspace/.chaos-server/current"

# The same refusal with something to go back to, which is the case that matters:
# the host has a working version pointed at, and a failed upgrade must leave the
# pointer where it was. The artifact and the pointer are laid down here because the
# point is only that a previous version was installed and selected.
in_noexec 'mkdir -p /lab/workspace/.chaos-server/1.0.0
  cp /lab/chaos-remote-server /lab/workspace/.chaos-server/1.0.0/chaos-remote-server
  chmod 755 /lab/workspace/.chaos-server/1.0.0/chaos-remote-server
  printf "1.0.0\n" >/lab/workspace/.chaos-server/current'
run_capture on_noexec 'install 7.7.7 --from /lab/chaos-remote-server'
checks=$((checks + 1))
if [ "${capture_status}" != "0" ] && grep -qi 'execut' "${capture_file}" \
  && grep -qi 'restored' "${capture_file}"; then
  say "ok  the upgrade is refused and the pointer is put back, reported together"
else
  failure "the refused upgrade did not report a restoration (exit ${capture_status}): \
$(tr '\n' ' ' <"${capture_file}")"
fi
check_in_noexec "the pointer still selects the version installed before the attempt" \
  "test \"\$(tr -d '\n' </lab/workspace/.chaos-server/current)\" = 1.0.0"

log "a dropped connection mid-session fails instead of hanging"
start="$(date +%s)"
on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${tunnel_port} --token-file /shared/token \
  exec --timeout 120 -- sleep 90" >/dev/null 2>&1 &
session_job=$!
sleep 2
stop_tunnels
run_capture wait "${session_job}"
session_status="${capture_status}"
elapsed=$(( $(date +%s) - start ))
checks=$((checks + 1))
if [ "${session_status}" != "0" ] && [ "${elapsed}" -lt 40 ]; then
  say "ok  the in-flight exec failed after ${elapsed}s with exit ${session_status}"
else
  failure "the in-flight exec exited ${session_status} after ${elapsed}s"
fi
tunnel_pid="$(start_tunnel "${tunnel_port}" "${server_port}")"
sleep 0.5
expect_output "a new session works once the tunnel is back" "protocol" remote 'ping'
expect_output "the server is still serving after the drop" "buildbox" remote 'cat README.md'

# ------------------------------------------------------------------ the waits ----

# Two clocks exist for two different failures. A transport that is not up yet can be
# waited for, because dialling presents nothing; a request that has already been sent
# cannot be, because its credential is spent and a reply arriving late would answer
# the wrong question. Everything below drives both against the deployed binaries over
# the real tunnel, so the numbers are the ones an operator gets.

log "a reply that never arrives ends the session instead of hanging"
top_up tokens token
tokens_before="$(on_dev 'wc -l </shared/token' | tr -d ' ')"
start="$(date +%s)"
# The reply is genuinely late rather than synthetic: the server really does run
# `sleep 8` and only then answers. The client has to stop at its own deadline.
run_capture on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${tunnel_port} \
  --token-file /shared/token --reply-timeout 2 \
  exec --timeout 120 -- sleep 8"
elapsed=$(( $(date +%s) - start ))
checks=$((checks + 1))
if [ "${capture_status}" != "0" ] && grep -qi 'no reply to exec' "${capture_file}" \
  && [ "${elapsed}" -ge 2 ] && [ "${elapsed}" -lt 15 ]; then
  say "ok  the 8s exec was abandoned after ${elapsed}s, and named as exec"
else
  failure "a 2s reply-timeout did not bound an 8s exec (exit ${capture_status}, ${elapsed}s): \
$(tr '\n' ' ' <"${capture_file}")"
fi
# The request died, but the credential was spent all the same. If it stayed in the
# file, the next run would present a credential the server has already seen.
tokens_after="$(on_dev 'wc -l </shared/token' | tr -d ' ')"
checks=$((checks + 1))
if [ "${tokens_after}" = "$((tokens_before - 1))" ]; then
  say "ok  the credential the abandoned run sent is out of the file (${tokens_before} -> ${tokens_after})"
else
  failure "the abandoned run left ${tokens_after} credentials in the file (was ${tokens_before})"
fi
# Sessions are independent on the server side, so a request still running there must
# not stop a new session from opening.
expect_output "a new session opens while the abandoned request still runs remotely" \
  "protocol" remote 'ping'

log "a transport that is not up yet can be waited for, and by default is not"
top_up tokens token
start="$(date +%s)"
run_capture on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${later_port} \
  --token-file /shared/token ping"
elapsed=$(( $(date +%s) - start ))
checks=$((checks + 1))
if [ "${capture_status}" != "0" ] && [ "${elapsed}" -lt 10 ]; then
  say "ok  with no --connect-wait the dead port was refused after ${elapsed}s"
else
  failure "a dial without --connect-wait took ${elapsed}s (exit ${capture_status}): \
$(tr '\n' ' ' <"${capture_file}")"
fi
# The case the flag exists for: the tunnel is being set up while the client starts.
# The status file is written by the container-side command when the client exits, so
# "still waiting" is observed on the shared volume rather than inferred from a
# process id on this machine.
rm -f "${lab_root}/shared/wait.txt" "${lab_root}/shared/wait.status"
start="$(date +%s)"
set +e
on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${later_port} --token-file /shared/token \
    --connect-wait 30 ping >/shared/wait.txt 2>&1; echo \$? >/shared/wait.status" \
  >"${capture_file}" 2>&1 &
late_job=$!
set -e
sleep 3
checks=$((checks + 1))
if [ ! -e "${lab_root}/shared/wait.status" ]; then
  say "ok  3s in, still waiting with nothing to connect to"
else
  failure "the client stopped waiting before the tunnel existed ($(cat "${lab_root}/shared/wait.status"))"
fi
late_tunnel="$(start_tunnel "${later_port}" "${server_port}")"
run_capture wait "${late_job}"
elapsed=$(( $(date +%s) - start ))
exit_code="$(cat "${lab_root}/shared/wait.status" 2>/dev/null || echo never)"
checks=$((checks + 1))
if [ "${exit_code}" = "0" ] && grep -qi 'protocol' "${lab_root}/shared/wait.txt" \
  && [ "${elapsed}" -lt 25 ]; then
  say "ok  the session opened ${elapsed}s in, as soon as the tunnel appeared"
else
  failure "waiting for a late tunnel failed (client exit ${exit_code}, ${elapsed}s): \
$(tr '\n' ' ' <"${lab_root}/shared/wait.txt" 2>/dev/null)"
fi
kill "${late_tunnel}" 2>/dev/null || true

log "a peer that accepts and never speaks is a timeout, not a hang"
# `silent_port` has no server behind it at all: socat here stands in for a host that
# takes the TCP connection and then has nothing to say, which is the shape of a
# tunnel opened at the wrong port. The client's own default handshake deadline is
# the thing under test, so no flag is passed and this check takes that long on
# purpose -- it is the number an operator without any flags gets.
top_up tokens token
socat "TCP-LISTEN:${silent_port},reuseaddr,fork" SYSTEM:'sleep 60' >/dev/null 2>&1 &
silent_pid=$!
sleep 0.5
start="$(date +%s)"
run_capture on_dev "/lab/bin/chaos-remote --tcp 127.0.0.1:${silent_port} \
  --token-file /shared/token ping"
elapsed=$(( $(date +%s) - start ))
checks=$((checks + 1))
if [ "${capture_status}" != "0" ] && grep -qi 'no reply to the handshake' "${capture_file}" \
  && [ "${elapsed}" -ge 28 ] && [ "${elapsed}" -lt 50 ]; then
  say "ok  the mute peer was given up on after ${elapsed}s and reported as the handshake"
else
  failure "the default handshake deadline did not fire as expected (exit ${capture_status}, ${elapsed}s): \
$(tr '\n' ' ' <"${capture_file}")"
fi
for child in $(pgrep -P "${silent_pid}" 2>/dev/null || true); do
  kill "${child}" 2>/dev/null || true
done
kill "${silent_pid}" 2>/dev/null || true
silent_pid=""

# ----------------------------------------------------------------- summary ----

log "summary"
if [ -n "${failures}" ]; then
  echo "FAILED checks:${failures}" >&2
  echo >&2
  echo "not covered by this lab:" >&2
  echo "  - SSH host-key verification. There is no SSH transport in this build; the" >&2
  echo "    tunnel here carries the trust, so a wrong host key has no path to occur." >&2
  echo "  - a macOS or Windows remote host. Both containers are Linux; those hosts" >&2
  echo "    are what the platform legs of CI are for." >&2
  echo "  - many sessions at once against one server." >&2
  exit 1
fi
echo "all ${checks} M4 acceptance checks passed against ${IMAGE}"
