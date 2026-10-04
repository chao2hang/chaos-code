#!/usr/bin/env python3
"""Fixtures for `scripts/ci/check-tree-ownership.py`.

The gate measures who owns the paths in a checkout, so that a container running as root through a
bind mount cannot leave behind something its owner has to clean up with a privileged shell. Three
things were measured on 2026-10-04: one gate run left `scripts/ci/__pycache__` holding `.pyc` files
owned by root; a `--full` run over a clean clone left `target/` owned by root, which is the mount
point the runtime makes for the cargo volume; and an `rm -rf` over a root-created non-empty
directory by the owner of the tree prints `Permission denied` and exits 1.

The fixtures cannot create a path owned by a second uid on an ordinary host -- creating one that
root-owned is exactly the damage, and it would need root to clean up -- so the per-path comparison
is driven through `--assume-owner-uid`, which is the seam the gate exists for. `RealOwnershipTests`
does the same thing with genuine ownership changes and runs wherever the process is privileged
enough to undo them, which is the container the gate is aimed at.

The counted tree below is what most assertions quote numbers from. `target` is pruned by default,
and symlinks are statted rather than followed:

    repo/README.md
    repo/scripts/a.py
    repo/scripts/ci/b.py
    repo/scripts/ci/deep/c.py
    repo/target/poison.o        pruned: 1 skipped path
    repo/link        -> scripts
    repo/dangling    -> gone

    python3 scripts/ci/test-check-tree-ownership.py
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
GATE = HERE / "check-tree-ownership.py"

#: A uid no generated tree can hold. Every path a fixture creates belongs to whoever runs the
#: fixtures, so expecting this one turns every judged path into a finding, which is how the
#: comparison is exercised without a second uid.
FOREIGN_UID = 60001

#: Judged paths in the tree `TreeFixtures.build` makes: four under the root (`README.md`, `scripts`,
#: `link`, `dangling`), two under `scripts`, two under `scripts/ci`, one under `deep`.
COUNTED_WALKED = 9


def walked_count(out: str) -> int:
    """The number of judged paths in either verdict line."""
    match = re.search(r"\((\d+) path\(s\) walked", out)
    if match is None:
        raise AssertionError(f"no walked count in: {out!r}")
    return int(match.group(1))


def run_gate(args: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(GATE)] + args, cwd=REPO, capture_output=True, text=True
    )


class TreeFixtures(unittest.TestCase):
    """Builds the counted tree and runs the gate over it as a subprocess."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.base = Path(self._tmp.name)
        self.tree = self.base / "repo"

    def build(self) -> Path:
        """The counted tree from the module docstring."""
        (self.tree / "scripts" / "ci" / "deep").mkdir(parents=True)
        (self.tree / "README.md").write_text("root\n", encoding="utf-8")
        (self.tree / "scripts" / "a.py").write_text("1\n", encoding="utf-8")
        (self.tree / "scripts" / "ci" / "b.py").write_text("2\n", encoding="utf-8")
        (self.tree / "scripts" / "ci" / "deep" / "c.py").write_text("3\n", encoding="utf-8")
        (self.tree / "target").mkdir()
        (self.tree / "target" / "poison.o").write_text("x\n", encoding="utf-8")
        os.symlink(self.tree / "scripts", self.tree / "link")
        os.symlink(self.tree / "gone", self.tree / "dangling")
        return self.tree

    def write_mounts(self, *points: Path) -> Path:
        """A `mountinfo`-formatted file listing exactly these mount points."""
        path = self.base / "mountinfo"
        lines = [
            f"{i} {i} 8:1 / {point} rw,relatime - ext4 /dev/sda1 rw"
            for i, point in enumerate(points, start=1)
        ]
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")
        return path

    def gate(self, *args: str, root: Path | None = None) -> subprocess.CompletedProcess:
        return run_gate(["--root", str(root or self.tree)] + list(args))

    def assume_foreign(self, *args: str) -> subprocess.CompletedProcess:
        return self.gate("--assume-owner-uid", str(FOREIGN_UID), *args)

    def assert_clean(self, proc: subprocess.CompletedProcess) -> subprocess.CompletedProcess:
        self.assertEqual(proc.returncode, 0, f"stderr: {proc.stderr}\nstdout: {proc.stdout}")
        self.assertIn("tree-ownership: OK", proc.stdout)
        return proc

    def assert_flags(self, proc: subprocess.CompletedProcess, *fragments: str) -> None:
        self.assertNotEqual(proc.returncode, 0, f"stdout: {proc.stdout}")
        self.assertIn("tree-ownership: FAIL", proc.stderr)
        for fragment in fragments:
            self.assertIn(fragment, proc.stderr)

    def findings(self, proc: subprocess.CompletedProcess) -> list[str]:
        """The finding lines, i.e. stderr without the verdict line."""
        return [line for line in proc.stderr.splitlines() if not line.startswith("tree-ownership:")]


@unittest.skipIf(os.name == "nt", "the ownership being measured is a POSIX uid")
class WalkAndCompareTests(TreeFixtures):
    # -- what the walk has to reach ------------------------------------------------------------

    def test_counted_tree_is_walked_at_every_depth(self) -> None:
        """4 + 2 + 2 + 1: a walk that stopped at the first level would report 4."""
        self.build()
        proc = self.assert_clean(self.gate())
        self.assertIn(f"{COUNTED_WALKED} path(s) walked", proc.stdout)
        self.assertIn("1 path(s) skipped", proc.stdout)

    def test_the_deepest_path_is_compared(self) -> None:
        """The path four levels down has to reach the comparison, not just the stack."""
        self.build()
        proc = self.assume_foreign()
        self.assert_flags(proc, "scripts/ci/deep/c.py")

    def test_every_judged_path_is_a_finding_when_the_expected_owner_is_elsewhere(self) -> None:
        """Nine judged paths, nine findings: nothing is compared twice or skipped silently."""
        self.build()
        proc = self.assume_foreign()
        self.assertIn(f"{COUNTED_WALKED} path(s) not owned by", proc.stderr)
        self.assertEqual(len(self.findings(proc)), COUNTED_WALKED, proc.stderr)

    def test_the_owner_of_the_root_is_the_expected_owner_with_no_flag(self) -> None:
        self.build()
        proc = self.assert_clean(self.gate())
        self.assertIn(f"({os.stat(self.tree).st_uid})", proc.stdout)

    def test_assuming_the_roots_own_uid_is_the_control_for_the_seam(self) -> None:
        self.build()
        uid = os.stat(self.tree).st_uid
        self.assert_clean(self.gate("--assume-owner-uid", str(uid)))

    def test_finding_names_the_uid_and_the_remedy(self) -> None:
        """The point of the rule is that the developer cannot clean this up alone."""
        self.build()
        proc = self.assume_foreign()
        self.assertIn(str(FOREIGN_UID), proc.stderr)
        self.assertIn("cannot delete", proc.stderr)
        self.assertIn("privileged shell", proc.stderr)

    def test_findings_go_to_stderr_and_stdout_stays_empty_on_failure(self) -> None:
        self.build()
        proc = self.assume_foreign()
        self.assertNotEqual(proc.returncode, 0)
        self.assertEqual(proc.stdout, "")
        self.assertTrue(proc.stderr.strip())

    # -- what is deliberately not judged -------------------------------------------------------

    def test_target_is_skipped_by_default(self) -> None:
        """Build output is not the checkout, and in the container the name is a volume mount."""
        self.build()
        proc = self.assume_foreign()
        self.assertIn("1 path(s) skipped", proc.stderr)
        self.assertNotIn("target", "\n".join(self.findings(proc)))

    def test_a_prune_can_be_replaced(self) -> None:
        """`--prune` replaces the default instead of adding to it, so the build tree is walked."""
        self.build()
        proc = self.assume_foreign("--prune", "nothing")
        # The pruned name and the file inside it are both back: 9 + 2.
        self.assertIn(f"{COUNTED_WALKED + 2} path(s) walked", proc.stderr)
        self.assertIn("target/poison.o", "\n".join(self.findings(proc)))

    def test_prune_is_repeatable(self) -> None:
        self.build()
        proc = self.assume_foreign("--prune", "target", "--prune", "scripts")
        self.assertIn("3 path(s) walked", proc.stderr)
        self.assertIn("2 path(s) skipped", proc.stderr)
        self.assertNotIn("deep", "\n".join(self.findings(proc)))

    def test_a_mount_point_under_the_root_is_skipped_with_its_subtree(self) -> None:
        """What was mounted there came from elsewhere on purpose."""
        self.build()
        mounts = self.write_mounts(self.tree / "scripts")
        proc = self.assert_clean(self.gate("--mounts", str(mounts)))
        self.assertIn("3 path(s) walked", proc.stdout)
        self.assertIn("2 path(s) skipped", proc.stdout)

    def test_a_mount_point_at_the_root_does_not_empty_the_walk(self) -> None:
        """In the container the checkout *is* the bind mount; that must not empty the walk.

        The root is not among the entries of its own listing, so the rule that skips mounted paths
        cannot reach it. This is the contract the container run depends on.
        """
        self.build()
        mounts = self.write_mounts(self.tree)
        proc = self.assert_clean(self.gate("--mounts", str(mounts)))
        self.assertIn(f"{COUNTED_WALKED} path(s) walked", proc.stdout)

    def test_a_mount_point_whose_name_needs_an_escape_is_still_matched(self) -> None:
        """`mountinfo` writes a space in a path as `\\040`; the naive split would miss it."""
        self.build()
        room = self.tree / "with space"
        room.mkdir()
        (room / "inside.txt").write_text("x\n", encoding="utf-8")
        escaped = str(room).replace(" ", "\\040")
        mounts = self.base / "mountinfo-escaped"
        mounts.write_text(
            f"9 9 8:1 / {escaped} rw,relatime - ext4 /dev/sda1 rw\n", encoding="utf-8"
        )
        proc = self.assert_clean(self.gate("--mounts", str(mounts)))
        self.assertIn(f"{COUNTED_WALKED} path(s) walked", proc.stdout)
        self.assertIn("2 path(s) skipped", proc.stdout)

    def test_a_mount_file_that_cannot_be_read_skips_nothing(self) -> None:
        """On a platform with no mountinfo the rule has to be inert, not fatal."""
        self.build()
        proc = self.assert_clean(self.gate("--mounts", str(self.base / "absent")))
        self.assertIn(f"{COUNTED_WALKED} path(s) walked", proc.stdout)

    # -- symlinks ------------------------------------------------------------------------------

    def test_a_symlink_to_a_directory_is_counted_but_not_walked_twice(self) -> None:
        self.build()
        proc = self.assert_clean(self.gate())
        self.assertIn(f"{COUNTED_WALKED} path(s) walked", proc.stdout)

    def test_files_behind_a_symlink_out_of_the_tree_are_neither_read_nor_judged(self) -> None:
        outside = self.base / "outside"
        outside.mkdir()
        for name in ("secret1.txt", "secret2.txt"):
            (outside / name).write_text("x\n", encoding="utf-8")
        self.build()
        os.symlink(outside, self.tree / "ext")
        proc = self.assume_foreign()
        self.assertIn(f"{COUNTED_WALKED + 1} path(s) not owned by", proc.stderr)
        self.assertNotIn("secret", proc.stderr)

    def test_a_dangling_symlink_is_judged_like_any_other_path(self) -> None:
        """A broken link has an owner; the point is ownership, not resolvability."""
        self.build()
        self.assert_clean(self.gate())
        self.assertIn("dangling", "\n".join(self.findings(self.assume_foreign())))

    # -- what a failure looks like -------------------------------------------------------------

    def test_findings_are_capped_but_the_counts_are_not(self) -> None:
        self.build()
        proc = self.assume_foreign("--max-findings", "2")
        self.assertEqual(len([f for f in self.findings(proc) if not f.startswith("...")]), 2)
        self.assertIn(f"{COUNTED_WALKED - 2} more path(s)", proc.stderr)
        self.assertIn(f"{COUNTED_WALKED} path(s) not owned by", proc.stderr)

    def test_a_directory_nobody_can_read_is_a_finding_not_a_silence(self) -> None:
        """`chmod 000` is how a root-created directory looks to the owner of the tree."""
        if os.geteuid() == 0:
            self.skipTest("root reads a 000 directory, so the branch cannot be reached here")
        self.build()
        locked = self.tree / "scripts" / "ci" / "locked"
        locked.mkdir()
        (locked / "inside.txt").write_text("x\n", encoding="utf-8")
        locked.chmod(0o000)
        self.addCleanup(locked.chmod, 0o755)
        proc = self.gate()
        self.assert_flags(proc, "1 unreadable", "scripts/ci/locked")

    def test_the_verdict_line_counts_every_path_even_when_findings_are_printed(self) -> None:
        self.build()
        proc = self.assume_foreign()
        self.assertIn(f"{COUNTED_WALKED} path(s) walked under", proc.stderr)

    def test_verbose_reports_the_expected_owner_and_what_was_skipped(self) -> None:
        self.build()
        proc = self.assert_clean(self.gate("--verbose"))
        self.assertIn("expected owner:", proc.stdout)
        self.assertIn("skipped directory names: target", proc.stdout)
        self.assertIn("mount points known:", proc.stdout)

    # -- misuse of the gate itself -------------------------------------------------------------

    def test_a_tree_with_nothing_in_it_is_an_error_not_a_pass(self) -> None:
        self.tree.mkdir()
        proc = self.gate()
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("nothing walked", proc.stderr)

    def test_a_root_that_was_pruned_away_entirely_is_an_error(self) -> None:
        """A prune that swallows the whole tree compared nothing."""
        (self.tree / "target").mkdir(parents=True)
        (self.tree / "target" / "a.o").write_text("x\n", encoding="utf-8")
        proc = self.gate()
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("nothing walked", proc.stderr)

    def test_a_path_that_is_not_a_directory_is_an_error(self) -> None:
        self.build()
        proc = self.gate("--root", str(self.tree / "README.md"))
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("is not a directory", proc.stderr)

    def test_a_root_that_does_not_exist_is_an_error(self) -> None:
        proc = self.gate("--root", str(self.base / "nope"))
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("is not a directory", proc.stderr)

    # -- the real repository -------------------------------------------------------------------

    def test_the_repository_is_clean_and_actually_walked(self) -> None:
        proc = run_gate(["--root", str(REPO)])
        self.assertEqual(proc.returncode, 0, f"stderr: {proc.stderr}")
        walked = walked_count(proc.stdout)
        self.assertGreaterEqual(walked, 3000, f"only {walked} paths walked: {proc.stdout}")

    def test_the_repository_walk_does_not_stop_at_the_first_level(self) -> None:
        """Same tree, one mount point less: the number has to move with the walk."""
        with_mounts = run_gate(["--root", str(REPO)])
        mounts = self.base / "mountinfo-crates"
        mounts.write_text(
            f"9 9 8:1 / {REPO / 'crates'} rw,relatime - ext4 /dev/sda1 rw\n", encoding="utf-8"
        )
        without = run_gate(["--root", str(REPO), "--mounts", str(mounts)])
        self.assertEqual(without.returncode, 0, f"stderr: {without.stderr}")
        first = walked_count(with_mounts.stdout)
        second = walked_count(without.stdout)
        self.assertGreater(first - second, 100, f"{first} vs {second}: crates/ was never descended")


@unittest.skipUnless(hasattr(os, "chown") and os.geteuid() == 0, "needs the privileges to undo it")
class RealOwnershipTests(TreeFixtures):
    """The same comparison against genuine ownership, which only a privileged run can set up.

    This is the class that runs inside the container the gate is aimed at, where the process is
    root and can both create and remove a path owned by somebody else.
    """

    def test_only_the_path_another_owner_left_behind_is_flagged(self) -> None:
        """One root-owned directory in a tree owned by someone else: exactly one finding."""
        self.build()
        leaked = self.tree / "scripts" / "ci" / "__pycache__"
        leaked.mkdir()
        (leaked / "check-doc-path-refs.cpython-311.pyc").write_text("x\n", encoding="utf-8")
        os.chown(self.tree, 4242, 4242)
        for path in sorted(self.tree.rglob("*"), key=lambda p: str(p).count(os.sep)):
            try:
                # `follow_symlinks=False` because a symlink has an owner of its own, which is what
                # is being measured here; following one would chown its target and leave the link.
                os.chown(path, 4242, 4242, follow_symlinks=False)
            except OSError:
                pass
        os.chown(leaked, 0, 0)

        proc = self.gate()
        self.assertIn("tree-ownership: FAIL", proc.stderr)
        self.assertIn("1 path(s) not owned by 4242", proc.stderr)
        self.assertIn(f"{COUNTED_WALKED + 2} path(s) walked", proc.stderr)
        names = "\n".join(self.findings(proc))
        self.assertIn("scripts/ci/__pycache__", names)
        self.assertNotIn("cpython-311.pyc", names)
        shutil.rmtree(self.tree, ignore_errors=True)


if __name__ == "__main__":
    unittest.main(verbosity=2)
