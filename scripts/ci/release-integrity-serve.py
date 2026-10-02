#!/usr/bin/env python3
"""Serve a fixture release the way a ghproxy-style mirror does.

Both integrity labs point an installer at `${CHAOS_GITHUB_MIRROR}/https://github.com/
<repo>/releases/download/v<version>/<file>`, which is the URL shape scripts/install.sh
and scripts/install.ps1 build when a mirror is configured. The path segment before the
embedded origin URL names the scenario directory, so one listener presents every
scenario at once.

Each request is appended to a log the checks read back, so "nothing else was fetched"
is an assertion rather than a hope.

Usage: release-integrity-serve.py RELEASES_DIR LOG_PATH PORT
"""

import http.server
import os
import sys

root = sys.argv[1]
log_path = sys.argv[2]


class Handler(http.server.BaseHTTPRequestHandler):
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
        self.wfile.write(body)

    def log_message(self, fmt, *args):
        pass


http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[3])), Handler).serve_forever()
