#!/usr/bin/env python3
"""Fixtures for the bash lookup in `scripts/ci/test-installer-asset-names.py`.

The asset-name check runs `install.sh`'s `detect_platform` through whichever bash it
settles on, so a "bash" that cannot run a command turns a working installer into a red
build. That is what happened on the Windows leg: `PATH` resolves `bash` to
`C:\\Windows\\System32\\bash.exe`, the WSL launcher, which with no distro installed
exits non-zero with its complaint on stdout and nothing on stderr -- indistinguishable
from `install.sh` itself having failed.

Every rule that decides whether a candidate is usable therefore gets a generated
executable here: one that exits non-zero, one that mimics the launcher's stdout-only
complaint, one that is a directory, and a System32 path that must lose to Git for
Windows even when it works. What the asset-name check reports when *nothing* is usable
is pinned too, because "found no bash" and "checked everything" must not look alike.

    python3 scripts/ci/test-installer-bash-resolution.py
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import os
import shutil
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "installer_asset_names", HERE / "test-installer-asset-names.py"
)
asset_names = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(asset_names)

SH = shutil.which("sh") or "/bin/sh"
GOOD = f'exec "{SH}" "$@"'
WSL_WITHOUT_A_DISTRO = 'echo "Windows Subsystem for Linux has no installed distributions."; exit 1'


def make_bash(directory: Path, name: str, body: str) -> str:
    """An executable named like bash that runs `body` instead of the command it is given."""
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / name
    path.write_text(f"#!{SH}\n{body}\n", encoding="utf-8")
    path.chmod(0o755)
    return str(path)


@unittest.skipIf(
    os.name == "nt",
    "the candidates below are POSIX shebang scripts; the Windows leg exercises the "
    "real lookup in RealHostTests instead",
)
class CandidateRuleTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tmp = Path(self._tmp.name)

    def test_no_candidates_at_all_returns_none(self):
        self.assertIsNone(asset_names.find_bash(environ={}, which=lambda _name: None))

    def test_broken_bash_on_path_is_not_returned(self):
        broken = make_bash(self.tmp, "bash", "exit 3")
        self.assertIsNone(asset_names.find_bash(environ={}, which=lambda _name: broken))

    def test_launcher_that_only_prints_to_stdout_is_not_usable(self):
        # The System32 launcher's exact failure shape, checked on its own as well as
        # through the lookup: a non-zero exit with output is not a bash.
        launcher = make_bash(self.tmp, "bash", WSL_WITHOUT_A_DISTRO)
        self.assertFalse(asset_names.bash_runs_commands(launcher))
        self.assertIsNone(asset_names.find_bash(environ={}, which=lambda _name: launcher))

    def test_bash_env_var_wins_over_path(self):
        # A `shell: bash` step exports BASH as the interpreter running the step, which is
        # the bash the leg was configured for; PATH is the fallback.
        chosen = make_bash(self.tmp / "git", "bash", GOOD)
        on_path = make_bash(self.tmp / "other", "bash", GOOD)
        got = asset_names.find_bash(environ={"BASH": chosen}, which=lambda _name: on_path)
        self.assertEqual(got, chosen)

    def test_system32_bash_is_never_chosen_on_windows(self):
        # A *working* System32 bash, and no discoverable Git install, so the only thing
        # that can reject it is the path filter: without that rule this returns the WSL
        # launcher, which is the failure that turned the Windows leg red.
        launcher = make_bash(self.tmp / "System32", "bash", GOOD)
        self.assertIsNone(asset_names.find_bash(environ={}, sep="\\", which=lambda _name: launcher))

    def test_system32_bash_is_skipped_even_when_named_by_bash_var(self):
        # Same rule on the highest-priority candidate: $BASH pointing at the launcher
        # must fall through to Git for Windows rather than be trusted.
        launcher = make_bash(self.tmp / "System32", "bash", GOOD)
        git = make_bash(self.tmp / "Git" / "bin", "bash.exe", GOOD)
        got = asset_names.find_bash(
            environ={"BASH": launcher, "ProgramFiles": str(self.tmp)},
            sep="\\",
            which=lambda _name: None,
        )
        self.assertEqual(got, git)

    def test_git_install_wins_over_what_path_resolves(self):
        # Both bashes work here, so this pins the ordering rather than the validation:
        # PATH on a Windows runner lists System32 before Git's bin directory.
        on_path = make_bash(self.tmp / "other", "bash", GOOD)
        program_files = self.tmp / "Program Files"
        git = make_bash(program_files / "Git" / "bin", "bash.exe", GOOD)
        got = asset_names.find_bash(
            environ={"ProgramFiles": str(program_files)}, sep="\\", which=lambda _name: on_path
        )
        self.assertEqual(got, git)

    def test_system32_is_not_filtered_on_posix(self):
        # The filter is about the WSL launcher specifically. A path that merely contains
        # "system32" on a Linux or macOS host is an ordinary bash and must still work.
        working = make_bash(self.tmp / "System32", "bash", GOOD)
        got = asset_names.find_bash(environ={}, which=lambda _name: working)
        self.assertEqual(got, working)

    def test_broken_preferred_candidate_yields_the_next_usable_one(self):
        git = make_bash(self.tmp / "Git" / "bin", "bash.exe", GOOD)
        broken = make_bash(self.tmp / "broken", "bash", "exit 3")
        got = asset_names.find_bash(
            environ={"BASH": broken, "ProgramFiles": str(self.tmp)},
            sep="\\",
            which=lambda _name: None,
        )
        self.assertEqual(got, git)

    def test_candidate_that_is_a_directory_is_skipped(self):
        good = make_bash(self.tmp / "bin", "bash", GOOD)
        got = asset_names.find_bash(environ={"BASH": str(self.tmp)}, which=lambda _name: good)
        self.assertEqual(got, good)


class DegradedLookupTests(unittest.TestCase):
    """What the asset-name check does when the lookup finds nothing usable.

    `find_bash` is replaced here -- the branch under test is the reporting, not the
    lookup: whether a Linux host is allowed to fall back to reading install.sh as text
    (it must not, or the check looks green while nothing was executed), versus a
    Windows host, where install.sh is not the installer anyone uses and a note is the
    honest answer.
    """

    def run_check_without_bash(self) -> tuple[asset_names.Result, str]:
        res = asset_names.Result()
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            published = asset_names.published_assets(res)
            self.assertEqual(res.failures, [], "release.yml must publish a readable asset list")
            original = asset_names.find_bash
            asset_names.find_bash = lambda *_args, **_kwargs: None
            try:
                asset_names.check_install_sh(res, published)
            finally:
                asset_names.find_bash = original
        return res, out.getvalue()

    def test_a_posix_host_says_so_as_a_failure(self):
        res, out = self.run_check_without_bash()
        self.assertIn("detect_platform was not executed", out)
        if os.name == "nt":
            self.assertEqual(res.failures, [], f"Windows may degrade, but not error: {res.failures}")
        else:
            self.assertEqual([f for f in res.failures if "bash" in f], res.failures, res.failures)
            self.assertIn("no usable bash", res.failures[0])

    def test_the_rest_of_the_check_still_runs_when_bash_is_found(self):
        # The mirror case: with the real lookup, install.sh's leg must produce no
        # failure and must report that it executed the shipped function.
        res = asset_names.Result()
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            published = asset_names.published_assets(res)
            asset_names.check_install_sh(res, published)
        self.assertEqual(res.failures, [], res.failures)
        self.assertIn("executing install.sh's detect_platform", out.getvalue())
        self.assertIn("this host's own uname", out.getvalue())


class RealHostTests(unittest.TestCase):
    def test_this_host_offers_a_usable_bash(self):
        # The production call, with the real environment: this is the assertion that
        # would have caught the Windows leg before it was pushed.
        found = asset_names.find_bash()
        self.assertIsNotNone(
            found,
            "no usable bash via $BASH, Git for Windows, or PATH; "
            "the asset-name check would degrade to reading install.sh as text",
        )
        self.assertTrue(asset_names.bash_runs_commands(found or ""), found)


if __name__ == "__main__":
    unittest.main(verbosity=2)
