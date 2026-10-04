#!/usr/bin/env python3
"""Serve a fixture release the way a ghproxy-style mirror does.

Both integrity labs point an installer at `${CHAOS_GITHUB_MIRROR}/https://github.com/
<repo>/releases/download/v<version>/<file>`, which is the URL shape scripts/install.sh
and scripts/install.ps1 build when a mirror is configured. The path segment before the
embedded origin URL names the scenario directory, so one listener presents every
scenario at once.

`--throttle CASE=BYTES_PER_SEC` (repeatable) paces that scenario's responses instead of
dumping the body in one write. That is the only way to offer the installer a mirror that
is up and *useless*: it answers 200, the checksum on the far side is correct, and bytes
arrive slowly enough that no honest user would wait. install.sh's floor is what decides,
and a scenario served at full speed can never reach it.

Each request is appended to a log the checks read back, so "nothing else was fetched"
is an assertion rather than a hope. The log records one `case name` line per request
regardless of pacing, because the readers compare that shape.

Usage: release-integrity-serve.py RELEASES_DIR LOG_PATH PORT [--throttle CASE=BPS]...
"""

import argparse
import http.server
import os
import sys
import time

parser = argparse.ArgumentParser()
parser.add_argument("releases_dir")
parser.add_argument("log_path")
parser.add_argument("port", type=int)
parser.add_argument(
    "--throttle",
    action="append",
    default=[],
    metavar="CASE=BPS",
    help="pace CASE's responses to about BPS bytes/second",
)
args = parser.parse_args()

root = args.releases_dir
log_path = args.log_path

# Pacing granularity: ten writes a second tracks a rate closely enough to stay clearly
# over or clearly under a floor without needing sub-millisecond sleeps.
TICK_SECONDS = 0.1

throttle = {}
for spec in args.throttle:
    case, sep, bps = spec.partition("=")
    if not sep or not bps.isdigit() or int(bps) < 1:
        raise SystemExit(f"--throttle wants CASE=BYTES_PER_SEC, got {spec!r}")
    throttle[case] = int(bps)


class Handler(http.server.BaseHTTPRequestHandler):
    # 1.1 so Content-Length means what the throttled routes need it to mean: curl keeps
    # waiting for the declared remainder, which is the state a stalled mirror leaves it in.
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        head, found, rest = self.path.lstrip("/").partition("/https://github.com/")
        case, _, extra = head.partition("/")
        name = rest.rsplit("/", 1)[-1].split("?")[0]

        def record(line):
            with open(log_path, "a") as handle:
                handle.write(line + "\n")

        if not found or extra or not name:
            record("rejected " + self.path)
            self.send_error(404)
            return
        record("{} {}".format(case, name))
        path = os.path.join(root, case, name)
        if not os.path.isfile(path):
            self.send_error(404)
            return
        with open(path, "rb") as handle:
            body = handle.read()
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        rate = throttle.get(case, 0)
        if not rate:
            self.wfile.write(body)
            return
        step = max(int(rate * TICK_SECONDS), 1)
        try:
            for offset in range(0, len(body), step):
                self.wfile.write(body[offset : offset + step])
                self.wfile.flush()
                time.sleep(TICK_SECONDS)
        except OSError:
            # The client giving up mid-body is the outcome several scenarios are built
            # to provoke, not an error in the endpoint.
            return

    def log_message(self, fmt, *args):
        pass


http.server.ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()
