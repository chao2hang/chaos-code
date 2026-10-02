#!/usr/bin/env python3
"""Adversarial self-test for scripts/l10n-guard.sh.

The guard exists for one event: an upstream merge leaves a file in place and
strips the Chinese out of it. Everything else it used to report as that event
was noise -- a renamed module, or dead code somebody deleted on purpose -- and
noise is how a guard gets ignored. This test pins both halves of the bargain:

  * the clobber still fails, and the new allow list cannot talk the guard out of
    reporting it;
  * the benign cases pass on their own merits (a move is detected from the
    content, not from a name), and the allow list cannot go stale silently.

Each case builds a throwaway git repo, runs the real script against it, and
asserts on the exit code *and* on which section the file was reported in. A case
that merely asserted "exit 0" would pass with a guard that checks nothing.

The last four cases are about the other half of the bargain: a guard that cannot
do its comparison must refuse to answer. Listing used to be read through a
process substitution, which discards the exit status, so a `git` that died left
both sides empty and every "did the Chinese disappear?" test then passed
vacuously. Same for a killed `rg` (counted as zero Han, dropping the file from
one side) and for an unreadable file at the ref. Those cases put a hostile helper
on PATH and require a non-zero exit, a `NO VERDICT` line, and *no* report.

Run:  python3 scripts/l10n-guard-selftest.py
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
GUARD = str(REPO / "scripts" / "l10n-guard.sh")

# Files every case starts from, committed as the `--before` ref. Chinese in all
# of them, under a path the cases pass to `--fortress` and outside it.
BASE: dict[str, str] = {
    "crates/pager/src/views/panel.rs": (
        '/// 配置面板\npub fn label() -> &str {\n    "已保存"\n}\n'
    ),
    "crates/pager/src/views/detail.rs": (
        '/// 详情面板\npub fn title() -> &str {\n    "详情"\n}\n'
    ),
    "crates/pager/src/app/mod.rs": (
        '// 应用入口\npub fn run() {\n    // 启动日志\n}\n'
    ),
}

FORTRESS = "crates/pager/src/views"
DETAIL = "crates/pager/src/views/detail.rs"
PANEL = "crates/pager/src/views/panel.rs"

# Ten distinct Chinese lines, for the threshold case.
TEN_LINES = "\n".join(
    f'    "{w}"  // 第{i}条说明文案' for i, w in enumerate(["甲", "乙", "丙", "丁", "戊"])
) + "\n"


class Case:
    """A throwaway repo plus one mutation, then a real guard run."""

    def __init__(self, name: str) -> None:
        self.name = name
        self.root = Path(tempfile.mkdtemp(prefix="l10n-selftest-"))
        self.repo = self.root / "repo"
        self.repo.mkdir(parents=True)
        self.allow = self.root / "allow.tsv"
        self.report = self.root / "report"
        self.git("init", "-q")
        self.git("config", "user.email", "selftest@example.invalid")
        self.git("config", "user.name", "selftest")
        for rel, text in BASE.items():
            self.write(rel, text)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "before")

    # -- fixture helpers ---------------------------------------------------
    def git(self, *args: str) -> str:
        return subprocess.run(
            ["git", *args], cwd=self.repo, check=True,
            capture_output=True, text=True,
        ).stdout

    def write(self, rel: str, text: str) -> None:
        path = self.repo / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def remove(self, rel: str) -> None:
        (self.repo / rel).unlink()

    def set_allow(self, text: str) -> None:
        self.allow.write_text(text, encoding="utf-8")

    def run(
        self,
        extra: list[str] | None = None,
        env: dict[str, str] | None = None,
    ) -> tuple[int, str]:
        argv = [
            "bash", GUARD,
            "--before", "HEAD", "--after", "WORKTREE",
            "--fortress", FORTRESS,
            "--allow-file", str(self.allow),
            "--report", str(self.report),
        ]
        if extra:
            argv += extra
        proc = subprocess.run(
            argv, cwd=self.repo, capture_output=True, text=True,
            env=env if env is not None else os.environ.copy(),
        )
        return proc.returncode, proc.stdout + proc.stderr

    def cleanup(self) -> None:
        shutil.rmtree(self.root, ignore_errors=True)


def section(out: str, header: str) -> str:
    """Body of one report section, up to the next `OK —`/`FAIL —` header."""
    m = re.search(
        rf"^(?:OK|FAIL) — {re.escape(header)}.*?:\n(.*?)(?:\n(?:(?:OK|FAIL) — |\n)|\Z)",
        out, re.M | re.S,
    )
    return m.group(1) if m else ""


def expect(out: str, rc: int, want_rc: int, where: str, needle: str = "") -> list[str]:
    errs: list[str] = []
    if rc != want_rc:
        errs.append(f"{where}: exit {rc}, want {want_rc}\n{out}")
    if needle and needle not in out:
        errs.append(f"{where}: output lacks {needle!r}\n{out}")
    return errs


# ---- Cases ---------------------------------------------------------------
def case_untouched() -> list[str]:
    c = Case("untouched")
    try:
        rc, out = c.run()
        # Sanity: with no changes at all, the guard must pass. If this fails,
        # every other case is meaningless.
        return expect(out, rc, 0, "untouched", "L10n Guard: PASS")
    finally:
        c.cleanup()


def case_clobber_in_place() -> list[str]:
    """THE regression: file stays, Chinese goes."""
    c = Case("clobber")
    try:
        c.write(DETAIL, "// Rewritten in English, file still here.\npub fn title() -> &str {\n    \"Details\"\n}\n")
        rc, out = c.run()
        errs = expect(out, rc, 1, "clobber", "regressed")
        body = section(out, "regressed")
        if DETAIL not in body:
            errs.append(f"clobber: {DETAIL} not listed under regressed\n{out}")
        if "path exists at WORKTREE" not in body:
            errs.append(f"clobber: regressed entry does not say the path still exists\n{out}")
        return errs
    finally:
        c.cleanup()


def case_clobber_cannot_be_allow_listed() -> list[str]:
    """The allow list must have no power over an in-place strip."""
    c = Case("clobber-allowed")
    try:
        c.write(DETAIL, "pub fn title() -> &str {\n    \"Details\"\n}\n")
        c.set_allow(f"{DETAIL}\t这条记录不该放行任何东西\n")
        rc, out = c.run()
        errs = expect(out, rc, 1, "clobber-allowed", "regressed")
        if DETAIL not in section(out, "regressed"):
            errs.append(f"clobber-allowed: allow-listed clobber escaped regressed\n{out}")
        if DETAIL in section(out, "removed as recorded"):
            errs.append("clobber-allowed: allow list excused a file that still exists\n" + out)
        return errs
    finally:
        c.cleanup()


def case_rename_is_a_move() -> list[str]:
    """Content, not the path, decides: the same lines at a new path = moved."""
    c = Case("rename")
    try:
        c.remove(DETAIL)
        c.write("crates/pager/src/views/detail_v2.rs", BASE[DETAIL] + "// 拆分自 detail.rs\n")
        rc, out = c.run()
        errs = expect(out, rc, 0, "rename", "moved")
        body = section(out, "moved")
        if DETAIL not in body:
            errs.append(f"rename: {DETAIL} not reported as moved\n{out}")
        if "2 of 2" not in body:
            errs.append(f"rename: moved entry does not report 2 of 2 lines\n{out}")
        if DETAIL in section(out, "fortress breach"):
            errs.append("rename: a move inside a fortress path counted as a breach\n" + out)
        return errs
    finally:
        c.cleanup()


def case_move_below_threshold_is_a_loss() -> list[str]:
    """Only some of the lines reappearing is a loss, not a move."""
    c = Case("partial")
    try:
        c.write("crates/pager/src/views/many.rs", "pub const LINES: [&str; 5] = [\n" + TEN_LINES + "];\n")
        c.git("add", "-A")
        c.git("commit", "-q", "-m", "with ten lines")
        c.remove("crates/pager/src/views/many.rs")
        # Keep two of the five lines somewhere else: 40 % < the 90 % default.
        c.write("crates/pager/src/app/echo.rs", 'pub fn a() -> &str {\n    "甲"  // 第0条说明文案\n    "乙"  // 第1条说明文案\n}\n')
        rc, out = c.run()
        errs = expect(out, rc, 1, "partial", "regressed")
        body = section(out, "regressed")
        if "crates/pager/src/views/many.rs" not in body:
            errs.append("partial: the partly-lost file is not listed as regressed\n" + out)
        if "2 of 5" not in body:
            errs.append(f"partial: regressed entry does not report the 2 of 5 ratio\n{out}")
        return errs
    finally:
        c.cleanup()


def case_unrecorded_deletion_fails() -> list[str]:
    c = Case("unrecorded")
    try:
        c.remove(DETAIL)
        rc, out = c.run()
        errs = expect(out, rc, 1, "unrecorded", "regressed")
        if DETAIL not in section(out, "regressed"):
            errs.append("unrecorded: deletion of Chinese without a record passed\n" + out)
        if DETAIL not in section(out, "fortress breach"):
            errs.append("unrecorded: fortress under views was not breached\n" + out)
        return errs
    finally:
        c.cleanup()


def case_recorded_deletion_passes() -> list[str]:
    c = Case("recorded")
    try:
        c.remove(DETAIL)
        c.set_allow(f"{DETAIL}\t死代码，界面在 panel.rs\n")
        rc, out = c.run()
        errs = expect(out, rc, 0, "recorded", "removed as recorded")
        if DETAIL not in section(out, "removed as recorded"):
            errs.append("recorded: the allow-list entry was not used\n" + out)
        if DETAIL in section(out, "fortress breach"):
            errs.append("recorded: a recorded removal still breached the fortress\n" + out)
        return errs
    finally:
        c.cleanup()


def case_removed_deletion_with_no_allow_list() -> list[str]:
    """--no-allow-list must actually disable the list."""
    c = Case("recorded-no-list")
    try:
        c.remove(DETAIL)
        c.set_allow(f"{DETAIL}\t死代码\n")
        rc, out = c.run(["--no-allow-list"])
        return expect(out, rc, 1, "recorded-no-list", "regressed")
    finally:
        c.cleanup()


def case_stale_entry_fails() -> list[str]:
    """A path that is in the tree must force the entry to be pruned."""
    c = Case("stale")
    try:
        c.set_allow(f"{PANEL}\t这条已经不适用了\n")
        rc, out = c.run()
        errs = expect(out, rc, 1, "stale", "stale allow-list")
        if PANEL not in section(out, "stale allow-list"):
            errs.append("stale: the existing path was not reported\n" + out)
        if "path is in the working tree" not in section(out, "stale allow-list"):
            errs.append("stale: the entry does not say why it is stale\n" + out)
        return errs
    finally:
        c.cleanup()


def case_committed_replacement_survives_a_degenerate_range() -> list[str]:
    """The CI smoke run: the removal is committed, HEAD vs HEAD must pass.

    Staleness used to be judged against `--after`, so in a comparison where both
    sides were the same ref every entry looked stale -- which is how this was
    caught, by wiring the smoke run into CI and running it locally first.
    """
    c = Case("committed-smoke")
    try:
        c.remove(DETAIL)
        c.set_allow(f"{DETAIL}\t死代码，界面在 panel.rs\n")
        c.git("add", "-A")
        c.git("commit", "-q", "-m", "retire detail.rs")
        rc, out = c.run(["--before", "HEAD", "--after", "HEAD"])
        errs = expect(out, rc, 0, "committed-smoke", "L10n Guard: PASS")
        if DETAIL in section(out, "stale allow-list"):
            errs.append("committed-smoke: a committed removal was called stale\n" + out)
        return errs
    finally:
        c.cleanup()


def case_shrunk_still_fails() -> list[str]:
    c = Case("shrunk")
    try:
        c.write(PANEL, '/// 配置面板\npub fn label() -> &str {\n    "Saved"\n}\n')
        rc, out = c.run()
        errs = expect(out, rc, 1, "shrunk", "shrunk")
        if PANEL not in section(out, "shrunk"):
            errs.append("shrunk: the decreased file was not listed\n" + out)
        return errs
    finally:
        c.cleanup()


def case_allow_list_without_reason_fails() -> list[str]:
    c = Case("malformed")
    try:
        c.remove(DETAIL)
        c.set_allow(f"{DETAIL}\n")
        rc, out = c.run()
        errs = expect(out, rc, 1, "malformed", "without a reason")
        if DETAIL in section(out, "removed as recorded"):
            errs.append("malformed: a reasonless entry excused a deletion\n" + out)
        return errs
    finally:
        c.cleanup()


def case_move_min_override() -> list[str]:
    """Same fixture as the partial case, with the bar lowered: passes as moved."""
    c = Case("move-min")
    try:
        c.write("crates/pager/src/views/many.rs", "pub const LINES: [&str; 5] = [\n" + TEN_LINES + "];\n")
        c.git("add", "-A")
        c.git("commit", "-q", "-m", "with ten lines")
        c.remove("crates/pager/src/views/many.rs")
        c.write("crates/pager/src/app/echo.rs", 'pub fn a() -> &str {\n    "甲"  // 第0条说明文案\n    "乙"  // 第1条说明文案\n    "丙"  // 第2条说明文案\n    "丁"  // 第3条说明文案\n}\n')
        rc, out = c.run(["--move-min", "80"])
        errs = expect(out, rc, 0, "move-min", "moved")
        if "4 of 5" not in section(out, "moved"):
            errs.append("move-min: moved entry does not report 4 of 5\n" + out)
        return errs
    finally:
        c.cleanup()


def case_death_is_announced() -> list[str]:
    """A broken helper must produce a loud failure, never a verdict.

    `set -e` alone made this indistinguishable from an interrupted run -- which is
    exactly how the 2026-10-03 Docker gate log reads: `FAIL`, then nothing. Worse,
    the listing used to be read through a process substitution, so a `git` that
    died (137 = 128+SIGKILL, the OOM shape below) produced *no files on either
    side* and every disappeared-file comparison then passed vacuously. The guard
    must name the broken side, say it reached no verdict, and exit non-zero.
    """
    c = Case("death")
    try:
        shim = c.root / "shim"
        shim.mkdir()
        fake_git = shim / "git"
        fake_git.write_text("#!/bin/sh\nexit 137\n", encoding="utf-8")
        fake_git.chmod(0o755)
        env = os.environ.copy()
        env["PATH"] = f"{shim}{os.pathsep}{os.environ['PATH']}"
        rc, out = c.run(env=env)
        errs: list[str] = []
        if rc == 0:
            errs.append("death: a guard whose git helper dies must not pass\n" + out)
        if "NO VERDICT" not in out:
            errs.append("death: no NO VERDICT line, so the failure is silent\n" + out)
        if "cannot list .rs files at HEAD" not in out:
            errs.append("death: the broken side is not named\n" + out)
        if "L10n Guard Report" in out:
            errs.append("death: a report was printed although no comparison ran\n" + out)
        return errs
    finally:
        c.cleanup()


def case_killed_counter_is_announced() -> list[str]:
    """`rg` dying on a real file must not be read as "this file has no Chinese".

    The counter runs once per file under `xargs`. Before the exit-status check, a
    killed rg yielded an empty count, which drops the file from one side of the
    comparison -- and "missing from one side" is the very signal this guard
    reports, so the failure lands as a wrong verdict rather than an error.
    The shim answers the preflight probe (bare `-o [\p{Han}]` on stdin) and dies
    on every real count.
    """
    c = Case("killed-counter")
    try:
        shim = c.root / "shim"
        shim.mkdir()
        fake_rg = shim / "rg"
        fake_rg.write_text(
            "#!/bin/sh\n"
            'if [ "$*" = "-o [\\p{Han}]" ]; then echo 中; exit 0; fi\n'
            "exit 137\n",
            encoding="utf-8",
        )
        fake_rg.chmod(0o755)
        env = os.environ.copy()
        env["PATH"] = f"{shim}{os.pathsep}{os.environ['PATH']}"
        rc, out = c.run(env=env)
        errs: list[str] = []
        if rc == 0:
            errs.append("killed-counter: a guard whose counter dies must not pass\n" + out)
        if "cannot count Han" not in out:
            errs.append("killed-counter: the dead counter is not named\n" + out)
        if "NO VERDICT" not in out:
            errs.append("killed-counter: no NO VERDICT line, so the failure is silent\n" + out)
        if "L10n Guard Report" in out:
            errs.append("killed-counter: a report was printed although no counting ran\n" + out)
        return errs
    finally:
        c.cleanup()


def case_unreadable_at_ref_is_announced() -> list[str]:
    """A file that cannot be read at the ref must stop the run, not vanish.

    Listing works here and only `git show` fails, which is the shape of a
    corrupted object or a repo the caller may not read. Counting that file as zero
    Han drops it from the `--before` side, and a path missing from one side is
    classified as moved/removed -- a wrong verdict with a confident-looking label.
    """
    c = Case("unreadable-ref")
    try:
        shim = c.root / "shim"
        shim.mkdir()
        real_git = shutil.which("git")
        assert real_git, "git not on PATH"
        fake_git = shim / "git"
        fake_git.write_text(
            "#!/bin/sh\n"
            'if [ "$1" = "show" ]; then echo "shim: object unreadable" >&2; exit 128; fi\n'
            f"exec {real_git} \"$@\"\n",
            encoding="utf-8",
        )
        fake_git.chmod(0o755)
        env = os.environ.copy()
        env["PATH"] = f"{shim}{os.pathsep}{os.environ['PATH']}"
        rc, out = c.run(env=env)
        errs: list[str] = []
        if rc == 0:
            errs.append("unreadable-ref: an unreadable file at the ref must not pass\n" + out)
        if "cannot read" not in out:
            errs.append("unreadable-ref: the unreadable path is not named\n" + out)
        if "NO VERDICT" not in out:
            errs.append("unreadable-ref: no NO VERDICT line, so the failure is silent\n" + out)
        if "L10n Guard Report" in out:
            errs.append("unreadable-ref: a report was printed despite the read failure\n" + out)
        return errs
    finally:
        c.cleanup()


def case_empty_listing_is_refused() -> list[str]:
    """An empty side is not a clean comparison; it is a broken one.

    Here `git` succeeds but lists nothing (a bad pathspec, a repo checked out
    somewhere unexpected). Both manifests come back empty, so nothing looks
    disappeared, nothing looks shrunk, and the guard certifies a tree it never
    read. The 395-Han-file fork cannot legitimately have zero `.rs` files under
    `crates/` on either side, so refusing is the only sound answer.
    """
    c = Case("empty-listing")
    try:
        shim = c.root / "shim"
        shim.mkdir()
        real_git = shutil.which("git")
        assert real_git, "git not on PATH"
        fake_git = shim / "git"
        fake_git.write_text(
            "#!/bin/sh\n"
            'if [ "$1" = "ls-files" ] || [ "$1" = "ls-tree" ]; then exit 0; fi\n'
            f"exec {real_git} \"$@\"\n",
            encoding="utf-8",
        )
        fake_git.chmod(0o755)
        env = os.environ.copy()
        env["PATH"] = f"{shim}{os.pathsep}{os.environ['PATH']}"
        rc, out = c.run(env=env)
        errs: list[str] = []
        if rc == 0:
            errs.append("empty-listing: an unread tree must not pass\n" + out)
        if "no .rs files listed at" not in out:
            errs.append("empty-listing: the empty side is not named\n" + out)
        if "L10n Guard Report" in out:
            errs.append("empty-listing: a verdict was reached from two empty sides\n" + out)
        return errs
    finally:
        c.cleanup()


CASES = [
    case_untouched,
    case_clobber_in_place,
    case_clobber_cannot_be_allow_listed,
    case_rename_is_a_move,
    case_move_below_threshold_is_a_loss,
    case_unrecorded_deletion_fails,
    case_recorded_deletion_passes,
    case_removed_deletion_with_no_allow_list,
    case_stale_entry_fails,
    case_committed_replacement_survives_a_degenerate_range,
    case_shrunk_still_fails,
    case_allow_list_without_reason_fails,
    case_move_min_override,
    case_death_is_announced,
    case_killed_counter_is_announced,
    case_unreadable_at_ref_is_announced,
    case_empty_listing_is_refused,
]


def main() -> int:
    if not os.access(GUARD, os.R_OK):
        print(f"guard not found: {GUARD}", file=sys.stderr)
        return 2
    failures = 0
    for case in CASES:
        errs = case()
        name = case.__name__.removeprefix("case_")
        if errs:
            failures += 1
            print(f"FAIL  {name}")
            for e in errs:
                print("      " + e.replace("\n", "\n      "))
        else:
            print(f"ok    {name}")
    print(f"\n{len(CASES) - failures}/{len(CASES)} l10n-guard cases passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
