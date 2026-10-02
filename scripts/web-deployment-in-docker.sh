#!/usr/bin/env bash
# Drive the Web host through a real TLS-terminating reverse proxy in Docker.
#
# Everything the Web host is tested with today is an in-process router, or a
# loopback HTTP server that a test or a local browser reaches directly. The
# deployment the architecture document actually describes (ADR-005/ADR-007) is
# different: `chaos-web` binds loopback, a proxy in front of it terminates TLS,
# and a browser on `https://<public-name>` sends a `Host` and an `Origin` the
# backend has never seen from a test. Those headers are the whole deployment
# risk, and no router test can produce them.
#
# This script builds the real `chaos-web` binary, runs it inside a container on
# loopback, puts stock nginx in the same network namespace as the proxy (which
# is what "the proxy can reach a loopback-only backend" means), terminates TLS
# with a lab CA, and then asks the questions M-1.4/M0.4/M5.3 leave open:
#
#   - does a browser that trusts only the lab CA complete the handshake, and is
#     old TLS refused
#   - is the credential required, ignored in a query string, and absent from
#     every log
#   - is the public `Host` refused until the operator declares it, and refused
#     for names nobody declared afterwards
#   - is an `https` `Origin` accepted only for the declared name *and* only when
#     the proxy says the client's side was TLS
#   - does a real session complete over `wss:` end to end, and does a
#     host-rewriting proxy that drops the TLS claim break exactly the way the
#     error message says
#   - does rotating the credential take effect, and does Safe Web Mode still
#     block a mutation once it arrives through a proxy
#   - is the backend unreachable from another machine on the same network, so
#     that the proxy is genuinely the only way in
#
# Topology:
#
#   this machine (curl, python3) --https--> :8443 nginx --http--> 127.0.0.1:8787 chaos-web
#                                   https--> :8444 nginx (rewrites Host, no TLS claim)
#   outsider container --http--> <web eth0>:8787   must be refused
#
# nginx shares the backend's network namespace with `--network container:...`,
# and restarting the backend is `docker exec`, so the proxy survives a credential
# rotation the way it would survive a service restart on a real host.
#
# Usage:
#   scripts/web-deployment-in-docker.sh             # run everything
#   scripts/web-deployment-in-docker.sh --keep      # leave containers + dir
#
# Environment:
#   IMAGE        image running the backend (default chaos-verify:local)
#   PROXY_IMAGE  nginx image (default nginx:1.27-alpine)
#   WORK_DIR     where the lab root lives (default: a fresh mktemp dir)
#   CARGO_ARGS   extra args for the build (default: --offline --locked)
#
# Capture evidence with:
#   scripts/web-deployment-in-docker.sh 2>&1 | tee web-deployment-$(date +%Y%m%d).log
set -euo pipefail

IMAGE="${IMAGE:-chaos-verify:local}"
PROXY_IMAGE="${PROXY_IMAGE:-nginx:1.27-alpine}"
WORK_DIR="${WORK_DIR:-}"
CARGO_ARGS="${CARGO_ARGS:---offline --locked}"
KEEP=0

for arg in "$@"; do
  case "$arg" in
    --keep) KEEP=1 ;;
    -h | --help)
      sed -n '2,53p' "$0"
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
network="chaos-web-lab-${run_id}"
web_container="chaos-web-lab-web-${run_id}"
proxy_container="chaos-web-lab-proxy-${run_id}"
outsider_container="chaos-web-lab-outsider-${run_id}"

# The name a browser would use. Port 8443 is part of the origin, because an
# origin includes a non-default port.
public_name="chaos.test"
authority="chaos.test:8443"
declared_origin="https://${authority}"
backend_port=8787
proxy_port=8443
naive_proxy_port=8444

token="$(openssl rand -hex 16)"
rotated_token="$(openssl rand -hex 16)"
failures=""
checks=0
capture_file=""
capture_status=0
body_file=""
head_file=""

for required in docker curl openssl python3; do
  if ! command -v "${required}" >/dev/null 2>&1; then
    echo "${required} is required and was not found on PATH." >&2
    exit 2
  fi
done
for required_image in "${IMAGE}" "${PROXY_IMAGE}"; do
  if ! docker image inspect "${required_image}" >/dev/null 2>&1; then
    echo "image ${required_image} not found locally; pull or build it first." >&2
    exit 2
  fi
done

# The proxy is published on fixed ports, because that is the port the declared
# origin carries. Say so up front rather than failing on a bind error later.
for probe in "${proxy_port}" "${naive_proxy_port}"; do
  if (exec 3<>"/dev/tcp/127.0.0.1/${probe}") 2>/dev/null; then
    echo "port ${probe} is already serving on this machine; stop it or edit the ports at the top of this script." >&2
    exit 2
  fi
done

if [ -z "${WORK_DIR}" ]; then
  WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/chaos-web-lab.XXXXXX")"
  work_dir_created=1
else
  mkdir -p "${WORK_DIR}"
  work_dir_created=0
fi
WORK_DIR="$(cd "${WORK_DIR}" && pwd -P)"
lab_root="${WORK_DIR}/run-${run_id}"
certs="${lab_root}/certs"
mkdir -p "${lab_root}/bin" "${certs}" "${lab_root}/nginx" "${lab_root}/work"
capture_file="${lab_root}/work/capture.txt"
body_file="${lab_root}/work/body.txt"
head_file="${lab_root}/work/headers.txt"

log() { printf '\n== %s\n' "$*"; }
say() { printf '   %s\n' "$*"; }
show_capture() {
  if [ -n "${capture_file}" ] && [ -f "${capture_file}" ]; then
    sed 's/^/     | /' "${capture_file}" | head -n 12 >&2
  fi
}
# A failed check is recorded and the run continues, so one run reports every
# problem rather than stopping at the first.
failure() {
  failures="${failures}
  - $1"
  printf '   FAILED %s\n' "$1" >&2
  show_capture
  return 0
}

teardown() {
  if [ "${KEEP}" = "1" ]; then
    echo
    echo "kept: containers ${web_container} ${proxy_container} ${outsider_container}, dir ${WORK_DIR}"
    return
  fi
  # The backend writes its log through a bind mount, as the container's user, so
  # that file has to be deleted there; the rest of the layout is ours already.
  if [ -n "$(docker container inspect -f '{{.Id}}' "${web_container}" 2>/dev/null || true)" ]; then
    docker exec "${web_container}" bash -c 'rm -rf /work/*' >/dev/null 2>&1 || true
  fi
  docker rm -f "${web_container}" "${proxy_container}" "${outsider_container}" >/dev/null 2>&1 || true
  docker network rm "${network}" >/dev/null 2>&1 || true
  if [ "${work_dir_created}" = "1" ]; then
    rm -rf "${WORK_DIR}"
  else
    rm -rf "${lab_root}"
  fi
}
trap teardown EXIT

on_web() { docker exec "${web_container}" bash -c "$*"; }

run_capture() {
  set +e
  "$@" >"${capture_file}" 2>&1
  capture_status=$?
  set -e
  return 0
}

# --------------------------------------------------------------------------------
# The two things every check talks through: curl over the terminator, and a
# WebSocket client that verifies the certificate instead of skipping validation.
# --------------------------------------------------------------------------------

# One HTTPS request through the proxy. The status code is what gets compared;
# headers and body are kept in files for the checks that need to read them.
proxy_request() { # port, path, then extra curl args
  local port="$1" path="$2"
  shift 2
  run_capture curl -sS --cacert "${certs}/ca.pem" \
    --resolve "${public_name}:${port}:127.0.0.1" \
    --connect-timeout 5 --max-time 25 \
    -D "${head_file}" -o "${body_file}" -w '%{http_code}' \
    "$@" "https://${public_name}:${port}${path}"
  return 0
}

expect_status() { # label, want, port, path, then extra curl args
  local label="$1" want="$2" port="$3" path="$4"
  shift 4
  checks=$((checks + 1))
  proxy_request "${port}" "${path}" "$@"
  local got
  got="$(tr -d '[:space:]' <"${capture_file}" 2>/dev/null || true)"
  if [ "${capture_status}" = "0" ] && [ "${got}" = "${want}" ]; then
    say "ok  ${label}"
  else
    failure "${label} (wanted HTTP ${want}, got '${got}', curl exit ${capture_status})"
  fi
  return 0
}

# A check whose point is that nothing answers at all.
expect_refused() { # label, port, path, then extra curl args
  local label="$1" port="$2" path="$3"
  shift 3
  checks=$((checks + 1))
  proxy_request "${port}" "${path}" "$@"
  if [ "${capture_status}" != "0" ]; then
    say "ok  ${label}"
  else
    failure "${label} (the request should not have completed; it got HTTP $(tr -d '[:space:]' <"${capture_file}"))"
  fi
  return 0
}

expect_body() { # label, needle
  local label="$1" needle="$2"
  checks=$((checks + 1))
  if grep -q -- "${needle}" "${body_file}" 2>/dev/null; then
    say "ok  ${label}"
  else
    failure "${label} (\"${needle}\" not in the body)"
  fi
  return 0
}

expect_header() { # label, needle
  local label="$1" needle="$2"
  checks=$((checks + 1))
  if grep -qi -- "${needle}" "${head_file}" 2>/dev/null; then
    say "ok  ${label}"
  else
    failure "${label} (\"${needle}\" not in the response headers)"
  fi
  return 0
}

# --------------------------------------------------------------------------------
# Build, then lay out the deployment: certificates, nginx, the two containers.
# --------------------------------------------------------------------------------

log "building the Web host on this machine"
(cd "${repo_root}" && cargo build --bin chaos-web ${CARGO_ARGS}) >/dev/null
cp "${repo_root}/target/debug/chaos-web" "${lab_root}/bin/chaos-web"
say "the container runs this binary: $(du -h "${lab_root}/bin/chaos-web" | cut -f1)"

log "issuing a lab CA and a certificate for ${public_name}"
# A real chain, not a self-signed leaf: the client is told to trust only the CA,
# so a handshake that verifies proves the leaf carries the right name and purpose.
openssl req -x509 -newkey rsa:2048 -sha256 -days 2 -nodes \
  -keyout "${certs}/ca.key" -out "${certs}/ca.pem" \
  -subj "/CN=Chaos lab CA" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "${certs}/server.key" \
  -out "${certs}/server.csr" -subj "/CN=${public_name}" >/dev/null 2>&1
printf 'subjectAltName=DNS:%s,IP:127.0.0.1\nextendedKeyUsage=serverAuth\nbasicConstraints=critical,CA:FALSE\n' \
  "${public_name}" >"${certs}/server.ext"
openssl x509 -req -in "${certs}/server.csr" -CA "${certs}/ca.pem" \
  -CAkey "${certs}/ca.key" -CAcreateserial -days 2 -sha256 \
  -extfile "${certs}/server.ext" -out "${certs}/server.pem" >/dev/null 2>&1
chmod 644 "${certs}/server.key"
openssl verify -CAfile "${certs}/ca.pem" "${certs}/server.pem" >/dev/null
say "chain verifies: $(openssl x509 -in "${certs}/server.pem" -noout -enddate | cut -d= -f2)"

cat >"${lab_root}/nginx/nginx.conf" <<NGINX
pid /tmp/nginx.pid;
error_log /dev/stderr warn;
events { worker_connections 128; }
http {
  access_log /dev/stdout;
  # The proxy a deployment would actually run: forward the name the browser
  # used, say which protocol it arrived on, and carry the WebSocket upgrade.
  server {
    listen ${proxy_port} ssl;
    ssl_certificate /etc/lab/server.pem;
    ssl_certificate_key /etc/lab/server.key;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_prefer_server_ciphers on;
    add_header Strict-Transport-Security "max-age=31536000" always;
    location / {
      proxy_pass http://127.0.0.1:${backend_port};
      proxy_http_version 1.1;
      proxy_set_header Host \$http_host;
      proxy_set_header Upgrade \$http_upgrade;
      proxy_set_header Connection "upgrade";
      proxy_set_header X-Forwarded-Proto \$scheme;
      proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
    }
  }
  # The proxy someone writes when the first attempt returns 401: rewrite Host to
  # the upstream so the Host check passes, and say nothing about TLS.
  server {
    listen ${naive_proxy_port} ssl;
    ssl_certificate /etc/lab/server.pem;
    ssl_certificate_key /etc/lab/server.key;
    ssl_protocols TLSv1.2 TLSv1.3;
    location / {
      proxy_pass http://127.0.0.1:${backend_port};
      proxy_http_version 1.1;
      proxy_set_header Host \$proxy_host;
      proxy_set_header Upgrade \$http_upgrade;
      proxy_set_header Connection "upgrade";
    }
  }
}
NGINX

# A minimal WebSocket client over a verified TLS socket. It is deliberately
# small: handshake, one masked text frame, read frames back. The point is that
# the certificate is validated and the name checked, the way a browser does it.
cat >"${lab_root}/work/wss-client.py" <<'PYTHON'
import base64
import json
import os
import socket
import ssl
import struct
import sys

authority = os.environ["LAB_AUTHORITY"]
host, _, port_s = authority.rpartition(":")
port = int(os.environ["LAB_PORT"])
origin = os.environ["LAB_ORIGIN"]
token = os.environ["LAB_TOKEN"]
cafile = os.environ["LAB_CA"]
mode = os.environ.get("LAB_MODE", "session")
proto = os.environ.get("LAB_PROTO", "https")

context = ssl.create_default_context(cafile=cafile)
# Deliberately not disabling anything: this is the check.
wrapped = context.wrap_socket(
    socket.create_connection(("127.0.0.1", port), timeout=20), server_hostname=host
)
print(f"TLS negotiated {wrapped.version()}")

key = base64.b64encode(os.urandom(16)).decode()
lines = [
    f"GET /ws HTTP/1.1",
    f"Host: {authority}",
    "Upgrade: websocket",
    "Connection: Upgrade",
    f"Sec-WebSocket-Key: {key}",
    "Sec-WebSocket-Version: 13",
    f"Origin: {origin}",
]
if token:
    lines.append(f"Authorization: Bearer {token}")
wrapped.sendall(("\r\n".join(lines) + "\r\n\r\n").encode())

buffer = b""
while b"\r\n\r\n" not in buffer:
    chunk = wrapped.recv(4096)
    if not chunk:
        break
    buffer += chunk
head, _, rest = buffer.partition(b"\r\n\r\n")
status = head.split(b"\r\n")[0].decode(errors="replace")
print(f"handshake response: {status}")
if " 101 " not in f" {status} ":
    print("body: " + rest.decode(errors="replace")[:200])
    sys.exit(1)
if b"sec-websocket-accept" not in head.lower():
    print("no Sec-WebSocket-Accept header")
    sys.exit(1)


class Reader:
    def __init__(self, source, initial):
        self.source = source
        self.buffer = bytearray(initial)

    def read(self, count):
        while len(self.buffer) < count:
            chunk = self.source.recv(4096)
            if not chunk:
                raise EOFError("socket closed")
            self.buffer += chunk
        taken, self.buffer = bytes(self.buffer[:count]), self.buffer[count:]
        return taken


reader = Reader(wrapped, rest)


def send(text):
    payload = text.encode()
    mask = os.urandom(4)
    masked = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
    header = bytearray([0x81])
    size = len(payload)
    if size < 126:
        header.append(0x80 | size)
    elif size < 65536:
        header.append(0x80 | 126)
        header += struct.pack(">H", size)
    else:
        header.append(0x80 | 127)
        header += struct.pack(">Q", size)
    wrapped.sendall(bytes(header) + mask + masked)


def receive():
    first, second = reader.read(2)
    length = second & 0x7F
    if length == 126:
        length = struct.unpack(">H", reader.read(2))[0]
    elif length == 127:
        length = struct.unpack(">Q", reader.read(8))[0]
    return json.loads(reader.read(length).decode())


send(json.dumps({"type": "create_session", "client_msg_id": "wss-create", "workspace_id": None}))
session_id = None
for _ in range(40):
    event = receive()
    if event.get("type") == "session_created":
        session_id = event["session_id"]
        break
if not session_id:
    print("no session_created event arrived over wss")
    sys.exit(1)
print(f"session created over {proto} websocket: {session_id}")

if mode == "session":
    # A session that only gets as far as `session_created` has proved the
    # upgrade, not the deployment: the answer streaming back is what a user
    # actually notices when a proxy is misconfigured.
    send(
        json.dumps(
            {
                "type": "submit",
                "client_msg_id": "wss-submit",
                "session_id": session_id,
                "prompt": "through the proxy",
            }
        )
    )
    deltas = 0
    for _ in range(200):
        event = receive()
        kind = event.get("type")
        if kind == "text_delta":
            deltas += 1
        elif kind in ("completed", "error"):
            break
    if not deltas:
        print("the reply never streamed over the encrypted session")
        sys.exit(1)
    print(f"streamed {deltas} text deltas over wss")

if mode == "mutation":
    send(
        json.dumps(
            {
                "type": "update_settings",
                "client_msg_id": "wss-settings",
                "base_url": None,
                "model": "through-the-proxy",
            }
        )
    )
    for _ in range(20):
        event = receive()
        code = event.get("code", "")
        if code:
            print(f"mutation reply: {code}")
            if code == "safe_web_mode_blocked":
                sys.exit(0)
            sys.exit(1)
    print("the mutation produced no error code")
    sys.exit(1)
PYTHON

# The WebSocket handshake through the proxy, with the client's Origin and
# credential under the caller's control.
wss_handshake() { # label, port, origin, token, then LAB_MODE=... via MODE env
  local label="$1" port="$2" origin="$3" bearer="$4"
  checks=$((checks + 1))
  run_capture env \
    LAB_AUTHORITY="${authority}" LAB_PORT="${port}" LAB_ORIGIN="${origin}" \
    LAB_TOKEN="${bearer}" LAB_CA="${certs}/ca.pem" \
    LAB_MODE="${MODE:-session}" \
    python3 "${lab_root}/work/wss-client.py"
  if [ "${capture_status}" = "0" ]; then
    say "ok  ${label}"
  else
    failure "${label} (exit ${capture_status})"
  fi
  return 0
}

# The same handshake when a refusal is the point. The client exits nonzero for
# anything other than a 101, so the status has to be read from what it printed.
wss_expect_refusal() { # label, port, origin, bearer, want-needle
  local label="$1" port="$2" origin="$3" bearer="$4" want="$5"
  checks=$((checks + 1))
  run_capture env \
    LAB_AUTHORITY="${authority}" LAB_PORT="${port}" LAB_ORIGIN="${origin}" \
    LAB_TOKEN="${bearer}" LAB_CA="${certs}/ca.pem" \
    LAB_MODE="${MODE:-session}" \
    python3 "${lab_root}/work/wss-client.py"
  if [ "${capture_status}" != "0" ] && grep -q -- "${want}" "${capture_file}"; then
    say "ok  ${label}"
  else
    failure "${label} (expected \"${want}\"; exit ${capture_status}, output: $(tr '\n' ' ' <"${capture_file}"))"
  fi
  return 0
}

start_backend() { # -e NAME=VALUE pairs, applied to chaos-web
  # The log goes to a file the host can read, because a server started with
  # `docker exec` does not appear in `docker logs`.
  docker exec -d -e "CHAOS_WEB_PORT=${backend_port}" -e "CHAOS_WEB_TOKEN=${token}" \
    "$@" "${web_container}" \
    bash -c 'exec /lab/bin/chaos-web >>/work/backend.log 2>&1'
  # Wait for the backend itself, not for a fixed sleep: the proxy is already up.
  local waited=0
  while [ "${waited}" -lt 120 ]; do
    if on_web "curl -fsS --max-time 2 http://127.0.0.1:${backend_port}/health" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.25
    waited=$((waited + 1))
  done
  return 1
}

# Stopping means gone, not signalled: if the old process still held the port, the
# next start would fail to bind and the checks below would silently talk to the
# server that was already running.
stop_backend() {
  docker exec "${web_container}" pkill -x chaos-web >/dev/null 2>&1 || true
  local waited=0
  while [ "${waited}" -lt 40 ]; do
    if ! on_web "curl -fsS --max-time 1 http://127.0.0.1:${backend_port}/health" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.25
    waited=$((waited + 1))
  done
  return 0
}

backend_is_up() { on_web "curl -fsS --max-time 2 http://127.0.0.1:${backend_port}/health" >/dev/null 2>&1; }

log "starting the backend, the proxy, and a machine that should stay outside"
docker network create --driver bridge "${network}" >/dev/null
# The published ports belong to the container that owns the network namespace,
# which is the backend's: nginx joins it rather than having one of its own.
docker run -d --name "${web_container}" --network "${network}" \
  -p "${proxy_port}:${proxy_port}" -p "${naive_proxy_port}:${naive_proxy_port}" \
  -v "${lab_root}:/lab:ro" -v "${lab_root}/work:/work" \
  "${IMAGE}" sleep infinity >/dev/null
docker run -d --name "${outsider_container}" --network "${network}" \
  "${IMAGE}" sleep infinity >/dev/null
docker run -d --name "${proxy_container}" --network "container:${web_container}" \
  -v "${lab_root}/nginx/nginx.conf:/etc/nginx/nginx.conf:ro" \
  -v "${certs}:/etc/lab:ro" "${PROXY_IMAGE}" nginx -g 'daemon off;' >/dev/null

web_ip="$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' \
  "${web_container}" | cut -d/ -f1)"
say "backend address on the shared network: ${web_ip}:${backend_port}"

if ! start_backend; then
  echo "the backend never answered on loopback inside its container; giving up." >&2
  docker logs --tail 20 "${web_container}" >&2 || true
  exit 1
fi
say "the backend listens on loopback only, and nginx shares that namespace"

# --------------------------------------------------------------------------------
log "a browser that trusts only the lab CA can reach the deployment"
expect_ok_body() { # label, path, curl args...
  local label="$1" path="$2"
  shift 2
  checks=$((checks + 1))
  proxy_request "${proxy_port}" "${path}" "$@"
  if [ "${capture_status}" = "0" ]; then
    say "ok  ${label}"
  else
    failure "${label} (curl exit ${capture_status})"
  fi
  return 0
}

expect_ok_body "TLS 1.3 handshake verified against the lab CA alone" /health
expect_body "the health route answers through the terminator" '"status":"ok"'
expect_header "the terminator added HSTS" "strict-transport-security"
checks=$((checks + 1))
if proxy_request "${proxy_port}" /health --tlsv1.2 --tls-max 1.2 >/dev/null 2>&1 \
  && [ "${capture_status}" = "0" ]; then
  say "ok  TLS 1.2 is still available for older clients"
else
  failure "TLS 1.2 should still work"
fi
expect_refused "TLS 1.1 and older are refused" "${proxy_port}" /health --tls-max 1.1
expect_refused "the TLS port will not speak plaintext HTTP" "${proxy_port}" /health \
  --http1.1 --connect-to "${public_name}:${proxy_port}:127.0.0.1:80"

# --------------------------------------------------------------------------------
log "the credential is required, and is never taken from a URL"
# All of these go through the host-rewriting proxy, so that the credential is
# genuinely the only rule under test: on the public-name port a request would
# also be refused for its Host, and the check would pass for the wrong reason.
expect_status "an unauthenticated API call stays closed" 401 "${naive_proxy_port}" /api/handshake
expect_body "and says the credential is what is missing" '"error":"credential_required"'
expect_status "a wrong credential is refused" 401 "${naive_proxy_port}" /api/handshake \
  -H "Authorization: Bearer not-the-token"
expect_body "naming the credential again rather than the Host" '"error":"credential_required"'
checks=$((checks + 1))
proxy_request "${naive_proxy_port}" "/api/handshake?token=${token}"
if [ "$(tr -d '[:space:]' <"${capture_file}")" = "401" ]; then
  say "ok  the credential in a query string is not accepted"
else
  failure "a token in the query string must never authorize"
fi
# Through the host-rewriting proxy, because at this point nobody has declared a
# public name yet: the credential alone must not be enough to answer a name the
# operator never configured (that refusal is checked on purpose below).
checks=$((checks + 1))
proxy_request "${naive_proxy_port}" /api/handshake -H "Authorization: Bearer ${token}"
if [ "$(tr -d '[:space:]' <"${capture_file}")" = "200" ]; then
  say "ok  the bearer credential opens the API when the proxy presents the loopback name"
  expect_body "the handshake carries the protocol version" '"protocol_version"'
else
  failure "the bearer credential should open the API (got $(cat "${capture_file}"))"
fi

# --------------------------------------------------------------------------------
log "the public name is refused until the operator declares it"
checks=$((checks + 1))
proxy_request "${proxy_port}" /api/handshake -H "Authorization: Bearer ${token}"
if [ "$(tr -d '[:space:]' <"${capture_file}")" = "401" ]; then
  say "ok  a Host the operator never declared is refused, credential and all"
  expect_body "and says the Host is the rule that fired" '"error":"host_not_allowed"'
else
  failure "an undeclared public Host must be refused (got $(cat "${capture_file}"))"
fi
checks=$((checks + 1))
if docker exec "${outsider_container}" curl -fsS --max-time 3 \
  "http://${web_ip}:${backend_port}/health" >/dev/null 2>&1; then
  failure "the backend answered a non-loopback address; the bind is not loopback-only"
else
  say "ok  another machine on the same network cannot reach the backend at all"
fi

log "declaring the deployment's origin"
stop_backend
# The log belongs to the container's user, so it is cleared there.
on_web 'rm -f /work/backend.log' || true
if ! start_backend -e "CHAOS_WEB_PUBLIC_ORIGIN=${declared_origin}"; then
  echo "the backend refused to start with a declared public origin." >&2
  tail -20 "${lab_root}/work/backend.log" >&2 || true
  exit 1
fi
checks=$((checks + 1))
if grep -q "accepting Host and Origin for ${authority}" "${lab_root}/work/backend.log"; then
  say "ok  the backend says out loud which public name it will answer for"
else
  failure "the backend did not report the declared origin (log: $(cat "${lab_root}/work/backend.log" 2>/dev/null))"
fi
checks=$((checks + 1))
proxy_request "${proxy_port}" /api/handshake -H "Authorization: Bearer ${token}"
if [ "$(tr -d '[:space:]' <"${capture_file}")" = "200" ]; then
  say "ok  the declared public Host is now served over TLS"
else
  failure "the declared public Host must be served (got $(cat "${capture_file}"))"
fi
checks=$((checks + 1))
proxy_request "${proxy_port}" /api/handshake -H "Host: other.test" \
  -H "Authorization: Bearer ${token}" -H "X-Forwarded-Proto: https"
if [ "$(tr -d '[:space:]' <"${capture_file}")" = "401" ]; then
  say "ok  a different public name is still refused"
  expect_body "naming the Host, not the credential" '"error":"host_not_allowed"'
else
  failure "declaring one host must not admit another (got $(cat "${capture_file}"))"
fi
checks=$((checks + 1))
proxy_request "${proxy_port}" /api/handshake \
  -H "Authorization: Bearer ${token}" \
  -H "Origin: https://victim-page.example" \
  -H "X-Forwarded-Proto: https"
if [ "$(tr -d '[:space:]' <"${capture_file}")" = "401" ]; then
  say "ok  a page from another https origin is refused"
  expect_body "and says the Origin is the rule that fired" '"error":"origin_not_allowed"'
else
  failure "an unrelated https origin must be refused (got $(cat "${capture_file}"))"
fi
# The declared origin alone is not enough: without the proxy's TLS claim the
# `https` Origin matches nothing the backend can be sure the browser used. This
# has to be the second proxy, because nginx overwrites X-Forwarded-Proto with the
# protocol the request really arrived on -- asking it to lie is not a test.
checks=$((checks + 1))
proxy_request "${naive_proxy_port}" /api/handshake \
  -H "Authorization: Bearer ${token}" \
  -H "Origin: ${declared_origin}"
if [ "$(tr -d '[:space:]' <"${capture_file}")" = "401" ]; then
  say "ok  the declared origin still needs the proxy to report TLS"
  expect_body "and says so in the words the proxy operator needs" \
    '"error":"origin_requires_forwarded_proto"'
else
  failure "an https origin claimed over a plaintext hop must be refused (got $(cat "${capture_file}"))"
fi

# --------------------------------------------------------------------------------
log "a real session runs over wss: through the terminator"
wss_handshake "the WebSocket upgrade completes over wss: with a verified certificate" \
  "${proxy_port}" "${declared_origin}" "${token}"
check_wss_text() { # label, needle
  local label="$1" needle="$2"
  checks=$((checks + 1))
  if grep -q -- "${needle}" "${capture_file}"; then
    say "ok  ${label}"
  else
    failure "${label} (\"${needle}\" not in the client output)"
  fi
  return 0
}
check_wss_text "the negotiated protocol is TLS 1.2 or newer" "TLS negotiated TLSv1."
check_wss_text "the reply streams back over the encrypted session" "text deltas over wss"
wss_expect_refusal "without a credential the upgrade never happens" "${proxy_port}" \
  "${declared_origin}" "" "401 Unauthorized"
check_wss_text "and the refusal names the missing credential" "credential_required"

log "the proxy that rewrites Host and stays quiet about TLS"
checks=$((checks + 1))
proxy_request "${naive_proxy_port}" /api/handshake -H "Authorization: Bearer ${token}"
if [ "$(tr -d '[:space:]' <"${capture_file}")" = "200" ]; then
  say "ok  a request with no Origin still works through a host-rewriting proxy"
else
  failure "a Host-rewriting proxy should pass plain requests (got $(cat "${capture_file}"))"
fi
# A WebSocket handshake always carries an Origin, which is why this proxy shape
# appears to work for HTTP and then fails only for the live session.
wss_expect_refusal "the same proxy breaks the WebSocket, and only the WebSocket" \
  "${naive_proxy_port}" "${declared_origin}" "${token}" "401 Unauthorized"
check_wss_text "and it is the missing TLS claim the session is refused for" \
  "origin_requires_forwarded_proto"

log "a spoofed client address grants nothing"
checks=$((checks + 1))
proxy_request "${naive_proxy_port}" /api/handshake \
  -H "X-Forwarded-For: 127.0.0.1" -H "X-Real-IP: 127.0.0.1"
if [ "$(tr -d '[:space:]' <"${capture_file}")" = "401" ]; then
  say "ok  forwarding headers do not substitute for the credential"
else
  failure "X-Forwarded-For must not authorize anything (got $(cat "${capture_file}"))"
fi

# --------------------------------------------------------------------------------
log "rotating the credential"
superseded_token="${token}"
token="${rotated_token}"
stop_backend
if ! start_backend -e "CHAOS_WEB_PUBLIC_ORIGIN=${declared_origin}"; then
  echo "the backend refused to start with the rotated credential." >&2
  exit 1
fi
checks=$((checks + 1))
proxy_request "${proxy_port}" /api/handshake -H "Authorization: Bearer ${token}"
if [ "$(tr -d '[:space:]' <"${capture_file}")" = "200" ]; then
  say "ok  the new credential works through the proxy"
else
  failure "the rotated credential must work (got $(cat "${capture_file}"))"
fi
checks=$((checks + 1))
proxy_request "${proxy_port}" /api/handshake -H "Authorization: Bearer ${superseded_token}"
if [ "$(tr -d '[:space:]' <"${capture_file}")" = "401" ]; then
  say "ok  the credential it replaced is dead"
else
  failure "the superseded credential must be refused"
fi

log "Safe Web Mode still blocks a mutation that arrives through a proxy"
stop_backend
if ! start_backend -e "CHAOS_WEB_PUBLIC_ORIGIN=${declared_origin}" \
  -e "CHAOS_SAFE_WEB_MODE=1"; then
  echo "the backend refused to start in Safe Web Mode." >&2
  exit 1
fi
MODE=mutation wss_handshake "a settings mutation over wss: is refused with a code" \
  "${proxy_port}" "${declared_origin}" "${token}"
check_wss_text "the refusal is the documented safe_web_mode_blocked code" "safe_web_mode_blocked"

# --------------------------------------------------------------------------------
log "no credential appears in any log"
checks=$((checks + 1))
if docker logs "${proxy_container}" 2>&1 | grep -q "${token}"; then
  failure "the proxy's access log contains the live credential"
else
  say "ok  the proxy logged requests and never the credential"
fi
checks=$((checks + 1))
if docker logs "${proxy_container}" 2>&1 | grep -q "GET /health"; then
  say "ok  the requests really were logged, so the absence above means something"
else
  failure "the proxy logged nothing; the check above would pass for the wrong reason"
fi
# The backend was started with `docker exec`, so its output is in the file it
# was told to write, not in `docker logs`.
checks=$((checks + 1))
if grep -q "${token}" "${lab_root}/work/backend.log"; then
  failure "the backend log contains the live credential"
else
  say "ok  the backend log is free of the credential too"
fi
checks=$((checks + 1))
if grep -q "listening on" "${lab_root}/work/backend.log"; then
  say "ok  the backend did log, so that absence is not an empty file"
else
  failure "the backend log holds nothing; the check above would pass for the wrong reason"
fi

# --------------------------------------------------------------------------------
log "summary"
if [ -n "${failures}" ]; then
  echo "checks that failed:${failures}" >&2
  echo "run again with WORK_DIR set to inspect the same layout:" >&2
  echo "  WORK_DIR=${WORK_DIR} $0 --keep" >&2
  exit 1
fi
echo "all ${checks} TLS deployment checks passed against ${PROXY_IMAGE} + ${IMAGE}"
