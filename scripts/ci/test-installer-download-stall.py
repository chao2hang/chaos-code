#!/usr/bin/env python3
"""Fixtures for the stall floor in `install.sh`'s `download_github`.

The binary is fetched with no overall `--max-time`, on purpose: a released artifact is
over 150 MB and any cap short enough to be useful on a bad line also kills a legitimate
slow one. The hole that leaves is the mirror that answers 200 and then trickles bytes
forever. curl waits forever, prints nothing, and the user watching `install.sh` has no
number to act on. `--speed-limit`/`--speed-time` is the shape that closes it: abort when
the average rate over a window falls under a floor, so bytes still moving keep the
transfer alive.

Measured 2026-10-04 with curl 7.81.0, which is what set the two defaults and the wording:

* 1 KiB/s trickle, floor 2048 B/s over 2 s: aborted in 14.23 s (three tries), exit 28 --
  and `-w '%{http_code}'` still printed `200`. With no floor it was still transferring at
  the 25 s bound of the probe.
* 128 KiB/s feed of 1 MiB, same floor: completed in 6.67 s. The floor does not punish a
  connection that is merely slow.
* A server that holds the connection for 3 s before sending the response head, floor
  1024 B/s over 1 s: aborted at 1.20 s, exit 28; same endpoint with the floor off
  completed in 3.02 s. So the window is running before the first byte arrives, which is
  why one of the reason lines below is about *no* bytes rather than about slow bytes.

The `200` on an aborted transfer is the trap this file is most concerned with. Read on its
own it is an accepted download, and the next check -- the size floor -- then reports
"too small (N bytes)", sending the user off to look for a truncated artifact when the
mirror was stalled. So curl's exit gates the decision, and the reason says `stalled`.

Every case runs the shipped function, extracted verbatim from `install.sh` (the same
helper `test-installer-download-size.py` uses), against a paced HTTP server on
127.0.0.1. Only the mirror list is stubbed, because the real one resolves github.com.

    python3 scripts/ci/test-installer-download-stall.py
"""

from __future__ import annotations

import functools
import http.server
import importlib.util
import os
import re
import shutil
import signal
import subprocess
import tempfile
import threading
import time
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
INSTALL_SH = REPO / "scripts/install.sh"
SIZE_GUARD = HERE / "test-installer-download-size.py"


def _load_size_guard():
    """Reuse the sibling guard's extractor rather than a copy of it.

    Two readers of `download_github`'s text would drift the same way the two help blocks
    in `install.sh` do, and this file's cases are only meaningful if they run the bytes
    the shipped installer runs.
    """
    spec = importlib.util.spec_from_file_location("installer_download_size", SIZE_GUARD)
    if spec is None or spec.loader is None:
        raise AssertionError(f"cannot load {SIZE_GUARD}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


size_guard = _load_size_guard()

# The trickle is 8x under the default floor, so the abort is about the floor and not
# about where the boundary sits.
TRICKLE_BYTES = 128
TRICKLE_PAUSE = 1.0
TRICKLE_TOTAL = 4 << 20
# 16 KiB/s: 16x the default floor, over a transfer longer than the window it is checked
# against, so "above the floor finishes" cannot be confused with "finished too fast to
# have been watched".
PACE_BYTES = 64 * 1024
PACE_CHUNK = 1024
PACE_PAUSE = 0.06
SILENT_TOTAL = 4 << 20
FAST_BYTES = 64 * 1024


class PacedHandler(http.server.BaseHTTPRequestHandler):
    """Answers 200, then feeds bytes at the rate each route is named for."""

    # Content-Length is declared on every route, so curl is waiting for bytes it can
    # count; without it a stalled connection and a finished one look the same.
    protocol_version = "HTTP/1.1"

    def log_message(self, _fmt: str, *_args) -> None:
        return None

    def _head(self, total: int) -> None:
        self.send_response(200)
        self.send_header("Content-Length", str(total))
        self.send_header("Content-Type", "application/octet-stream")
        self.end_headers()
        self.wfile.flush()

    def do_GET(self) -> None:  # noqa: N802 - http.server's name
        route = self.path.split("?", 1)[0]
        try:
            if route == "/stall":
                self._head(TRICKLE_TOTAL)
                while True:
                    self.wfile.write(b"x" * TRICKLE_BYTES)
                    self.wfile.flush()
                    time.sleep(TRICKLE_PAUSE)
            elif route == "/slow":
                self._head(PACE_BYTES)
                sent = 0
                while sent < PACE_BYTES:
                    n = min(PACE_CHUNK, PACE_BYTES - sent)
                    self.wfile.write(b"y" * n)
                    self.wfile.flush()
                    sent += n
                    time.sleep(PACE_PAUSE)
            elif route == "/silent":
                # Accepts and answers, then never sends a byte: the case that proves the
                # window is not only about slow bytes.
                self._head(SILENT_TOTAL)
                time.sleep(120)
            elif route == "/fast":
                self._head(FAST_BYTES)
                self.wfile.write(b"z" * FAST_BYTES)
                self.wfile.flush()
            else:
                self.send_error(404)
        except OSError:
            # curl aborting on purpose closes the socket under us. In the stall and
            # silent routes that broken pipe *is* the outcome under test.
            return


@unittest.skipIf(os.name == "nt", "the installer leg under test is bash on a POSIX host")
@unittest.skipUnless(shutil.which("curl"), "download_github shells out to curl")
class DownloadGithubStallTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tmp = Path(self._tmp.name)
        self.server = http.server.ThreadingHTTPServer(
            ("127.0.0.1", 0), functools.partial(PacedHandler)
        )
        self.server_thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.server_thread.start()
        # addCleanup is LIFO: stop serving, then release the listening socket.
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)
        self.base = f"http://127.0.0.1:{self.server.server_address[1]}"
        self.dest = self.tmp / "downloaded.bin"
        self.function_text = size_guard.extract_function(
            INSTALL_SH.read_text(encoding="utf-8"), "download_github"
        )

    def harness_text(self, candidates, connect_timeout: int, max_time: int) -> str:
        listed = "".join(f"  printf '%s\\n' '{c}'\n" for c in candidates)
        return (
            "#!/usr/bin/env bash\n"
            "set -euo pipefail\n"
            f"github_url_candidates() {{\n{listed}}}\n"
            f"{self.function_text}\n"
            # min_bytes stays 1 throughout: every refusal asserted below must be the
            # stall floor's doing, not the size floor's.
            f"download_github '{candidates[0]}' \"$DEST\""
            f" {connect_timeout} {max_time} 1\n"
        )

    def download(
        self,
        *candidates: str,
        connect_timeout: int = 5,
        max_time: int = 0,
        stall_secs: int = 1,
        min_bps: int = 1024,
        timeout: int = 60,
    ):
        harness = self.tmp / "run.sh"
        harness.write_text(
            self.harness_text(candidates, connect_timeout, max_time), encoding="utf-8"
        )
        subprocess.run(["bash", "-n", str(harness)], check=True)
        env = dict(os.environ)
        env["DEST"] = str(self.dest)
        env["CHAOS_DOWNLOAD_STALL_SECS"] = str(stall_secs)
        env["CHAOS_DOWNLOAD_MIN_BPS"] = str(min_bps)
        return subprocess.run(
            ["bash", str(harness)], capture_output=True, text=True, timeout=timeout, env=env
        )

    # == the floor itself =====================================

    def test_trickle_is_aborted_and_named_as_a_stall(self):
        # The defect: at 128 B/s against a 150 MB artifact, nothing ever decided. The
        # pre-fix function reported "too small (N bytes)" for the same endpoint, because
        # curl's -w line still said 200 -- so the assertion that matters here is the
        # wording, not just the non-zero status.
        started = time.monotonic()
        proc = self.download(f"{self.base}/stall")
        elapsed = time.monotonic() - started
        self.assertNotEqual(proc.returncode, 0, f"stall went unwatched: {proc.stderr}")
        self.assertIn("error: download failed", proc.stderr)
        self.assertRegex(
            proc.stderr,
            r"why: stalled under 1024 B/s for 1s from \S+/stall \(\d+ bytes in \d+s\)",
        )
        self.assertNotIn("too small", proc.stderr)
        self.assertLess(elapsed, 30, f"the abort took {elapsed:.1f}s")
        self.assertFalse(self.dest.exists(), "an aborted transfer must not leave a body")

    def test_transfer_above_the_floor_finishes(self):
        # The other direction, and the reason a plain --max-time was not the fix: 64 KiB
        # at 16 KiB/s runs longer than the window it is watched over and must still land.
        proc = self.download(f"{self.base}/slow", stall_secs=3)
        self.assertEqual(proc.returncode, 0, f"stderr: {proc.stderr}")
        self.assertIn(f"{self.base}/slow", proc.stdout)
        self.assertEqual(self.dest.stat().st_size, PACE_BYTES)

    def test_floor_of_zero_leaves_the_transfer_running(self):
        # Documents the escape hatch the header advertises. With no floor and no
        # --max-time there is no timer left on this call site, so the only honest
        # observation is that the download is still going when we cut it off.
        #
        # This one cannot use subprocess.run(timeout=...): killing bash leaves curl alive
        # with the captured pipe still open, and the reaped parent then waits on a
        # transfer that never ends. The harness is put in its own process group and the
        # whole group is killed.
        harness = self.tmp / "run-zero.sh"
        harness.write_text(
            self.harness_text([f"{self.base}/stall"], 5, 0), encoding="utf-8"
        )
        env = dict(os.environ)
        env["DEST"] = str(self.dest)
        env["CHAOS_DOWNLOAD_MIN_BPS"] = "0"
        env["CHAOS_DOWNLOAD_STALL_SECS"] = "1"
        proc = subprocess.Popen(
            ["bash", str(harness)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
            start_new_session=True,
        )
        try:
            with self.assertRaises(subprocess.TimeoutExpired):
                proc.communicate(timeout=6)
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            stdout, stderr = proc.communicate(timeout=30)
        finally:
            if proc.poll() is None:
                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
                proc.wait(timeout=30)
        self.assertIn(f"try: {self.base}/stall", stderr)
        self.assertNotIn("error: download failed", stderr)
        self.assertEqual(stdout, "")

    def test_repeated_reason_is_reported_once(self):
        # Three tries of the same dead URL, one line of output. The report dedupes, or the
        # first thing a user reads is curl's retry count rather than the reason.
        proc = self.download(
            f"{self.base}/missing", f"{self.base}/missing", f"{self.base}/missing",
            stall_secs=5, timeout=45,
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertEqual(
            len(re.findall(r"why: HTTP 404 from \S+/missing", proc.stderr)),
            1,
            f"reason not deduplicated: {proc.stderr}",
        )

    def test_reason_report_is_capped_at_four_lines(self):
        # Four candidates, four distinct reasons, four lines: the cap keeps a long mirror
        # list from burying the `tip:` line that tells the user what to do next.
        proc = self.download(
            *(f"{self.base}/missing?cand={n}" for n in range(1, 6)),
            stall_secs=5,
            timeout=45,
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertEqual(proc.stderr.count("try:"), 5, proc.stderr)
        self.assertEqual(len(re.findall(r"why: ", proc.stderr)), 4, proc.stderr)
        self.assertEqual(
            proc.stderr.count("skipped:"), 0, "skipped report belongs to the success path"
        )
        self.assertIn("tip: set CHAOS_GITHUB_MIRROR", proc.stderr)

    def test_silent_endpoint_reports_no_bytes_not_slow_bytes(self):
        # The window is armed before the first byte: measured, a server holding the
        # response head for 3 s aborts at 1.2 s under a 1 s window. The reason line has
        # to say "no bytes", because "stalled under N B/s" over a 0-byte body would
        # describe a transfer that was never moving.
        proc = self.download(f"{self.base}/silent")
        self.assertNotEqual(proc.returncode, 0)
        self.assertRegex(proc.stderr, r"why: no bytes in \d+s from \S+/silent")
        self.assertNotIn("stalled under", proc.stderr)

    # == what the user is told =================================

    def test_stalled_candidate_is_named_when_the_next_one_works(self):
        # Failover has to work *and* leave a trace. Before this, the only thing on stderr
        # was `try:` plus the winning URL, so "mirror A stalled, origin saved the day" and
        # "mirror A was never tried" were indistinguishable after the fact.
        proc = self.download(f"{self.base}/stall", f"{self.base}/fast")
        self.assertEqual(proc.returncode, 0, f"stderr: {proc.stderr}")
        self.assertIn(f"{self.base}/fast", proc.stdout)
        self.assertRegex(proc.stderr, r"skipped: stalled under .* from \S+/stall")
        self.assertEqual(self.dest.stat().st_size, FAST_BYTES)

    def test_dead_candidate_is_named_on_the_success_path(self):
        # Same report, ordinary cause: the 404 stays a 404 rather than degrading to a
        # curl exit number, and it is still reported when the install goes on to succeed.
        proc = self.download(f"{self.base}/missing", f"{self.base}/fast", stall_secs=5)
        self.assertEqual(proc.returncode, 0, f"stderr: {proc.stderr}")
        self.assertRegex(proc.stderr, r"skipped: HTTP 404 from \S+/missing")

    # == the two help blocks ===================================

    def run_help(self, via_pipe: bool) -> str:
        text = INSTALL_SH.read_text(encoding="utf-8")
        if via_pipe:
            # $0 is "bash" here, so this is the branch a `curl | bash` reader actually
            # gets; the file branch reads the header comment instead.
            proc = subprocess.run(
                ["bash", "-s", "--", "--help"],
                input=text,
                capture_output=True,
                text=True,
                timeout=30,
                check=True,
            )
        else:
            proc = subprocess.run(
                ["bash", str(INSTALL_SH), "--help"],
                capture_output=True,
                text=True,
                timeout=30,
                check=True,
            )
        return proc.stdout

    def test_both_help_paths_document_the_floor(self):
        for via_pipe in (False, True):
            with self.subTest(via_pipe=via_pipe):
                out = self.run_help(via_pipe)
                self.assertIn("CHAOS_DOWNLOAD_MIN_BPS", out)
                self.assertIn("CHAOS_DOWNLOAD_STALL_SECS", out)

    def test_env_names_documented_in_the_header_match_the_heredoc(self):
        # install.sh carries the help twice on purpose (the piped form has no file to
        # read), so a variable added to one block and not the other is invisible to half
        # the users. The names are cheap to compare; the prose around them is not.
        header = set(re.findall(r"CHAOS_[A-Z_]+", self.run_help(via_pipe=False)))
        heredoc = set(re.findall(r"CHAOS_[A-Z_]+", self.run_help(via_pipe=True)))
        self.assertTrue(header, "no environment variables found in the header help")
        self.assertEqual(
            header - heredoc,
            set(),
            f"documented only in the header: {sorted(header - heredoc)}",
        )
        self.assertEqual(
            heredoc - header,
            set(),
            f"documented only in the piped help: {sorted(heredoc - header)}",
        )

    def test_floor_is_read_from_the_environment_with_these_names(self):
        # Pins the defaults the header promises, against the shipped text, so a renamed
        # or re-defaulted variable cannot silently disagree with --help.
        self.assertIn('local min_bps="${CHAOS_DOWNLOAD_MIN_BPS:-1024}"', self.function_text)
        self.assertIn(
            'local stall_secs="${CHAOS_DOWNLOAD_STALL_SECS:-45}"', self.function_text
        )
        self.assertIn('--speed-limit "$min_bps" --speed-time "$stall_secs"', self.function_text)


if __name__ == "__main__":
    unittest.main(verbosity=2)
