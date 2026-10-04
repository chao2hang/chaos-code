#!/usr/bin/env python3
"""Fixtures for the size floor in `install.sh`'s `download_github`.

The installer refuses a candidate whose body is smaller than `min_bytes`, because a
released artifact is over 150 MB and a short 200 is a truncated transfer or a proxy body;
the same floor also catches an HTML error page served with status 200. Refusing is not
fatal -- the loop moves to the next mirror and, if all of them fail, prints one `why:`
line per distinct reason. That report is the difference between "github is unreachable
from this network" and a silent exit, and it is what `CHAOS_GITHUB_MIRROR` advice hangs
off.

On 2026-10-04 the size probe was found able to end the installer before any of that.
`size="$(wc -c < "$dest" 2>/dev/null | tr -d '[:space:]')"` is a bare assignment in a
`set -euo pipefail` script, so when `wc` fails the assignment fails, `set -e` fires, and
the `|| size=0` fallback written on the very next line -- which shows what the author
expected to happen -- is unreachable. Measured against a local endpoint answering 200 with
a 4 KiB body and a `wc` that exits 1: the pre-fix function printed one `try:` line and
stopped, having tried one of two candidates and reported no reason. The fix appends
`|| true` to the substitution so an unreadable size means "too small", which is what the
fallback already encoded.

These fixtures run the shipped function, extracted verbatim from `install.sh`, against an
HTTP server on 127.0.0.1. The only stub is the mirror list, because the real one resolves
github.com. Each failing case asserts on the report, not only the exit status: the status
was already non-zero while the installer was broken.

    python3 scripts/ci/test-installer-download-size.py
"""

from __future__ import annotations

import functools
import http.server
import os
import re
import shutil
import subprocess
import tempfile
import threading
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
INSTALL_SH = HERE.parent.parent / "scripts/install.sh"
ARTIFACT_BYTES = 4096


def extract_function(source: str, name: str) -> str:
    """The function's text, read from the shipped script; it closes at column 0."""
    lines = source.splitlines(keepends=True)
    start = next((i for i, line in enumerate(lines) if line.startswith(f"{name}()")), None)
    if start is None:
        raise AssertionError(f"{name}() not found in {INSTALL_SH}")
    for end in range(start + 1, len(lines)):
        if lines[end].startswith("}"):
            return "".join(lines[start : end + 1])
    raise AssertionError(f"{name}() never closes in {INSTALL_SH}")


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    """The fixture's own requests are not test output; the default logs to stderr."""

    def log_message(self, _fmt: str, *_args) -> None:
        return None


def stub_server(directory: Path):
    handler = functools.partial(QuietHandler, directory=str(directory))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server


@unittest.skipIf(os.name == "nt", "the installer leg under test is bash on a POSIX host")
@unittest.skipUnless(shutil.which("curl"), "download_github shells out to curl")
class DownloadGithubSizeTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tmp = Path(self._tmp.name)
        self.www = self.tmp / "www"
        self.www.mkdir()
        (self.www / "artifact.bin").write_bytes(b"z" * ARTIFACT_BYTES)
        (self.www / "proxy-page.html").write_text(
            "<!DOCTYPE html>\n<html><body>proxy error</body></html>\n", encoding="utf-8"
        )
        self.server = stub_server(self.www)
        # addCleanup is LIFO: stop serving, then release the listening socket.
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)
        self.base = f"http://127.0.0.1:{self.server.server_address[1]}"

        # A `wc` that always fails, stood in front of PATH to mimic an unreadable body:
        # a full filesystem or a removed temp file, not a different installer.
        self.broken_bin = self.tmp / "broken-bin"
        self.broken_bin.mkdir()
        fake_wc = self.broken_bin / "wc"
        fake_wc.write_text("#!/bin/sh\nexit 1\n", encoding="utf-8")
        fake_wc.chmod(0o755)

        self.function_text = extract_function(
            INSTALL_SH.read_text(encoding="utf-8"), "download_github"
        )

    def harness_text(self, candidates) -> str:
        # The real candidate list resolves github.com and its mirrors; the loop under test
        # only needs to be handed the URLs and asked to work through them in order.
        listed = "".join(f"  printf '%s\\n' '{c}'\n" for c in candidates)
        return (
            "#!/usr/bin/env bash\n"
            "set -euo pipefail\n"
            f"github_url_candidates() {{\n{listed}}}\n"
            f"{self.function_text}\n"
            # The origin URL is only used in the failure message, so the first candidate
            # stands in for it rather than threading an argument through the harness.
            f"download_github '{candidates[0]}' \"$DEST\" 5 10 \"$MIN_BYTES\"\n"
        )

    def download(self, *candidates: str, min_bytes: int, broken_wc: bool = False):
        harness = self.tmp / "run.sh"
        harness.write_text(self.harness_text(candidates), encoding="utf-8")
        subprocess.run(["bash", "-n", str(harness)], check=True)
        env = dict(os.environ)
        env["MIN_BYTES"] = str(min_bytes)
        env["DEST"] = str(self.tmp / "downloaded.bin")
        if broken_wc:
            env["PATH"] = f"{self.broken_bin}{os.pathsep}{env['PATH']}"
        return subprocess.run(
            ["bash", str(harness)], capture_output=True, text=True, timeout=90, env=env
        )

    def test_full_body_is_accepted_and_the_url_is_reported(self):
        # Guards the fixture: were the endpoint or the harness broken, every "reports a
        # reason" case below would pass for the wrong reason.
        proc = self.download(f"{self.base}/artifact.bin", min_bytes=1)
        self.assertEqual(proc.returncode, 0, f"stdout: {proc.stdout}\nstderr: {proc.stderr}")
        self.assertIn(f"{self.base}/artifact.bin", proc.stdout)
        self.assertEqual((self.tmp / "downloaded.bin").stat().st_size, ARTIFACT_BYTES)

    def test_unreadable_size_reports_a_reason_instead_of_dying(self):
        # The 2026-10-04 defect. Pre-fix this returned 1 after a single `try:` line, with
        # neither `error: download failed` nor a `why:` line, and it never reached the
        # second candidate.
        proc = self.download(
            f"{self.base}/artifact.bin",
            f"{self.base}/artifact.bin?mirror=1",
            min_bytes=1,
            broken_wc=True,
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("error: download failed", proc.stderr)
        self.assertIn("too small (0 bytes)", proc.stderr)
        self.assertEqual(proc.stderr.count("try:"), 2, f"mirror not reached: {proc.stderr}")

    def test_short_body_reports_the_size_it_measured(self):
        # Same report, reached with a working `wc`: pins that the reason carries the real
        # byte count, so the `0 bytes` above is a measured empty read and not a constant.
        proc = self.download(f"{self.base}/artifact.bin", min_bytes=ARTIFACT_BYTES * 4)
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn(f"too small ({ARTIFACT_BYTES} bytes)", proc.stderr)

    def test_html_error_page_is_refused_by_name(self):
        # The other 200-with-a-body case the floor exists for, on the same code path.
        proc = self.download(f"{self.base}/proxy-page.html", min_bytes=1)
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("HTML response from", proc.stderr)

    def test_missing_object_reports_the_status_code(self):
        # Keeps the failure report honest for the ordinary case too, so the fixture is
        # not proving only the branch it was written for.
        proc = self.download(f"{self.base}/absent.bin", min_bytes=1)
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("error: download failed", proc.stderr)
        self.assertTrue(
            re.search(r"why: HTTP (404|000) from", proc.stderr),
            f"no status reason: {proc.stderr}",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
