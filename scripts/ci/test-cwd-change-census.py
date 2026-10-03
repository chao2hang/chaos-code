#!/usr/bin/env python3
"""Fixtures for `cwd-change-census.py`.

The guard exists because one test that left the process cwd inside a deleted
directory broke ~33 of its siblings, and the poisoned sibling is never the guilty
one. So these fixtures are not "does it count lines" -- they are the dodges. Each
case is a way to make a hazardous cwd change look ordinary, and each has to stay
red:

  * a new call site with no baseline row, in a test and in production code;
  * a baseline row whose line has moved (the site was edited under it);
  * a guarded test site whose guard was removed, or bound after the call;
  * deleting the baseline row of the guarded site, which must not turn the
    failure into a pass;
  * declaring a test site as `product` in the baseline, the other way out;
  * an empty or stubbed reason column, including the writer's own output;
  * `set_current_dir` written in a doc comment, a line comment and a log
    message, which must *not* count -- and the same file is then made to count
    by turning one mention into live code, so a clean run here proves the
    blanking works rather than that the scan finds nothing;
  * a test-only file with no attribute of its own, reached through a host's
    `#[cfg(test)] #[path = "..."] mod`, which is how the real `gate.rs` is
    compiled; reading that file alone would call it production.

One case runs the guard over this repository with its shipped baseline, so a
change to the sources that outruns the baseline fails here as well as in CI.

    python3 scripts/ci/test-cwd-change-census.py
"""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("cwd-change-census.py")
REPO = SCRIPT.parents[2]
BASELINE = SCRIPT.with_name("cwd-change-baseline.tsv")
_spec = importlib.util.spec_from_file_location("cwd_change_census", SCRIPT)
guard = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(guard)

REASON = "Restored by the guard bound above the call, before the temp dir goes away."

# Lines are numbered in the expectations below, so edits here move call sites and
# the assertions move with them on purpose.
GUARDED_TEST = """\
use std::path::PathBuf;

#[cfg(test)]
pub(crate) struct CwdGuard(pub PathBuf);

#[cfg(test)]
impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::CwdGuard;

    #[test]
    fn scans_its_own_cwd() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _guard = CwdGuard(std::env::current_dir().unwrap());
        std::env::set_current_dir(tmp.path()).unwrap();
        assert!(std::env::current_dir().is_ok());
    }
}
"""

GUARD_LINE = "        let _guard = CwdGuard(std::env::current_dir().unwrap());"
CHDIR_LINE = "        std::env::set_current_dir(tmp.path()).unwrap();"

PRODUCT_CHDIR = """\
pub fn apply_cwd_from(cwd: &std::path::Path) -> anyhow::Result<()> {
    std::env::set_current_dir(cwd).map_err(|e| anyhow::anyhow!("--cwd: {e}"))?;
    Ok(())
}
"""

PROSE_MENTIONS = """\
//! Notes about `std::env::set_current_dir` in a module doc comment.

pub fn warn_only() {
    // A line comment naming std::env::set_current_dir() without calling it.
    tracing::warn!(error = "change location: failed to set_current_dir");
}
"""

PROSE_CALL = """\
//! Notes about `std::env::set_current_dir` in a module doc comment.

pub fn warn_only() {
    // A line comment naming std::env::set_current_dir() without calling it.
    let _ = std::env::set_current_dir(std::env::temp_dir());
    tracing::warn!(error = "change location: failed to set_current_dir");
}
"""

TEST_ONLY_HOST = """\
pub fn helper() -> u32 {
    4
}

#[cfg(test)]
#[path = "inner/cases.rs"]
mod cases;
"""

TEST_ONLY_CASES = """\
struct RestoreCwd(std::path::PathBuf);

impl Drop for RestoreCwd {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

#[test]
fn hazard_needs_a_guard() {
    let _restore = RestoreCwd(std::env::current_dir().unwrap());
    std::env::set_current_dir(std::env::temp_dir()).unwrap();
}
"""

CLEAN = [
    ("crates/one/src/cli.rs", 2, "product", ""),
    ("crates/one/src/lib.rs", 9, "test", "drop-restore"),
    ("crates/one/src/lib.rs", 21, "test", "CwdGuard"),
    ("crates/two/src/inner/cases.rs", 5, "test", "drop-restore"),
    ("crates/two/src/inner/cases.rs", 12, "test", "RestoreCwd"),
]


def write(root: Path, rel: str, text: str) -> Path:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


def build(root: Path) -> Path:
    """A repository-shaped tree: a guarded test, a production chdir, prose."""
    write(root, "crates/one/src/lib.rs", GUARDED_TEST)
    write(root, "crates/one/src/cli.rs", PRODUCT_CHDIR)
    write(root, "crates/two/src/lib.rs", TEST_ONLY_HOST)
    write(root, "crates/two/src/inner/cases.rs", TEST_ONLY_CASES)
    write(root, "crates/two/src/notes.rs", PROSE_MENTIONS)
    return root


def rows_of(root: Path) -> list[tuple[str, int, str, str]]:
    return [
        (str(r["path"]), int(r["line"]), str(r["role"]), str(r["guard"]))
        for r in guard.sites(root)
    ]


def write_baseline(
    root: Path, rows: list[tuple[str, int, str, str]], reason: str = REASON
) -> Path:
    path = root / "baseline.tsv"
    path.write_text(
        "\n".join(
            "\t".join((p, str(line), role, guard_name or "none", reason))
            for p, line, role, guard_name in rows
        )
        + "\n",
        encoding="utf-8",
    )
    return path


def run(*argv: str) -> tuple[int, str]:
    proc = subprocess.run(
        [sys.executable, str(SCRIPT), *argv], capture_output=True, text=True, check=False
    )
    return proc.returncode, proc.stdout + proc.stderr


class MeasuredSites(unittest.TestCase):
    """The table itself, pinned before any baseline is involved."""

    def test_sites_roles_and_guards(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            self.assertEqual(
                rows_of(root),
                CLEAN,
                "prose must not count, a Drop impl counts as the restore, and a file "
                "reached through a host's cfg(test) #[path] declaration must be a test site",
            )

    def test_prose_mention_is_load_bearing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            self.assertEqual(
                [r for r in rows_of(root) if "notes.rs" in r[0]],
                [],
                "a module doc, a line comment and a log message naming the function are "
                "not call sites",
            )
            write(root, "crates/two/src/notes.rs", PROSE_CALL)
            self.assertEqual(
                [r for r in rows_of(root) if "notes.rs" in r[0]],
                [("crates/two/src/notes.rs", 5, "product", "")],
                "the same file must count once the call is real code, so the clean run "
                "above shows the blanking working, not a scan that finds nothing",
            )

    def test_guard_bound_after_the_call_is_not_a_restore(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(
                root,
                "crates/one/src/lib.rs",
                GUARDED_TEST.replace(f"{GUARD_LINE}\n{CHDIR_LINE}", f"{CHDIR_LINE}\n{GUARD_LINE}"),
            )
            self.assertEqual(
                [r for r in rows_of(root) if r[0].endswith("one/src/lib.rs")],
                [
                    ("crates/one/src/lib.rs", 9, "test", "drop-restore"),
                    ("crates/one/src/lib.rs", 20, "test", ""),
                ],
                "a guard bound after the call cannot put back a cwd the call already left "
                "in a deleted directory, so it must not be credited",
            )


class BaselineCheck(unittest.TestCase):
    def check(self, root: Path, baseline: Path) -> tuple[int, str]:
        return run("--root", str(root), "--check-baseline", str(baseline))

    def test_shipped_baseline_matches_the_shipped_sources(self) -> None:
        code, out = run("--root", str(REPO), "--check-baseline", str(BASELINE))
        self.assertEqual(code, 0, out)
        self.assertIn("every test site guarded", out)

    def test_clean_tree_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            baseline = write_baseline(root, rows_of(root))
            code, out = self.check(root, baseline)
            self.assertEqual(code, 0, out)
            self.assertIn("5 call site(s) match baseline.tsv (1 production, 4 test)", out)

    def test_new_test_site_needs_a_row(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            baseline = write_baseline(root, rows_of(root))
            write(
                root,
                "crates/three/src/lib.rs",
                "#[test]\nfn sneaks_a_chdir() {\n"
                "    std::env::set_current_dir(std::env::temp_dir()).unwrap();\n}\n",
            )
            code, out = self.check(root, baseline)
            self.assertEqual(code, 1, out)
            self.assertIn("crates/three/src/lib.rs:3 changes the process cwd", out)
            self.assertIn("is not in baseline.tsv", out)

    def test_moved_line_is_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            baseline = write_baseline(root, rows_of(root))
            write(root, "crates/one/src/cli.rs", "\n" + PRODUCT_CHDIR)
            code, out = self.check(root, baseline)
            self.assertEqual(code, 1, out)
            self.assertIn("baseline.tsv lists crates/one/src/cli.rs:2", out)
            self.assertIn("crates/one/src/cli.rs:3 changes the process cwd", out)

    def test_removed_guard_fails_with_the_row_left_alone(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            baseline = write_baseline(root, rows_of(root))
            write(
                root,
                "crates/one/src/lib.rs",
                GUARDED_TEST.replace(
                    GUARD_LINE, "        let _nothing = std::env::current_dir().unwrap();"
                ),
            )
            code, out = self.check(root, baseline)
            self.assertEqual(code, 1, out)
            self.assertIn("crates/one/src/lib.rs:21 is a test call site", out)
            self.assertIn("no CwdGuard/RestoreCwd bound", out)

    def test_dropping_the_row_is_not_an_escape(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            rows = [r for r in rows_of(root) if (r[0], r[1]) != ("crates/one/src/lib.rs", 21)]
            baseline = write_baseline(root, rows)
            code, out = self.check(root, baseline)
            self.assertEqual(code, 1, out)
            self.assertIn("crates/one/src/lib.rs:21 changes the process cwd", out)

    def test_declaring_a_test_site_as_product_is_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            rows = [
                (p, line, "product" if (p, line) == ("crates/one/src/lib.rs", 21) else role, g)
                for p, line, role, g in rows_of(root)
            ]
            baseline = write_baseline(root, rows)
            code, out = self.check(root, baseline)
            self.assertEqual(code, 1, out)
            self.assertIn("is role=test by the rules in this file but baseline.tsv says product", out)

    def test_guard_column_is_cross_checked(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            rows = [
                (
                    p,
                    line,
                    role,
                    "RestoreCwd" if (p, line) == ("crates/one/src/lib.rs", 21) else g,
                )
                for p, line, role, g in rows_of(root)
            ]
            baseline = write_baseline(root, rows)
            code, out = self.check(root, baseline)
            self.assertEqual(code, 1, out)
            self.assertIn("binds CwdGuard before the call", out)

    def test_stub_reason_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            baseline = write_baseline(root, rows_of(root), reason="n/a")
            code, out = self.check(root, baseline)
            self.assertEqual(code, 1, out)
            self.assertEqual(out.count("no reason worth reading"), 5)

    def test_writer_leaves_the_reason_column_empty(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            generated = root / "generated.tsv"
            code, out = run("--root", str(root), "--write-baseline", str(generated))
            self.assertEqual(code, 0, out)
            self.assertIn("5 row(s)", out)
            body = [
                line
                for line in generated.read_text(encoding="utf-8").splitlines()
                if line and not line.startswith("#")
            ]
            self.assertEqual([row.split("\t")[4] for row in body], [""] * 5)
            code, out = self.check(root, generated)
            self.assertEqual(code, 1, out)
            self.assertEqual(
                out.count("no reason worth reading"),
                5,
                "the writer cannot know why a change is safe, and a check that accepted "
                "its own unfilled output would pass on an empty explanation column",
            )


    def test_real_test_only_file_is_not_resolved_by_its_directory_name(self) -> None:
        # `git/safety_tests/gate.rs` is brought in by `#[cfg(test)] #[path = ...]` in
        # `git/safety.rs` and has no attribute of its own. Turn off the directory-name
        # shortcut and the module graph alone still has to call it test-only; without
        # that half of the rule its hazard site would be graded as production code,
        # where no restore is required at all.
        original = guard.test_by_location
        guard.test_by_location = lambda path, root: False
        try:
            rows = {(str(r["path"]), int(r["line"])): r for r in guard.sites(REPO)}
        finally:
            guard.test_by_location = original
        gate = [key for key in rows if key[0].endswith("git/safety_tests/gate.rs")]
        self.assertGreaterEqual(len(gate), 2, "the hazard site and its RestoreCwd::drop")
        self.assertTrue(
            all(rows[key]["role"] == "test" for key in gate),
            f"{gate} must be test sites by the #[path] declaration alone",
        )


    def test_real_test_only_file_is_not_resolved_by_its_directory_name(self) -> None:
        # `git/safety_tests/gate.rs` is brought in by `#[cfg(test)] #[path = ...]` in
        # `git/safety.rs` and carries no attribute of its own. Turn off the
        # directory-name shortcut and the module graph alone still has to call it
        # test-only; without that half of the rule, its hazard site would be graded as
        # production code, where no restore is required at all.
        original = guard.test_by_location
        guard.test_by_location = lambda path, root: False
        try:
            rows = {(str(r["path"]), int(r["line"])): r for r in guard.sites(REPO)}
        finally:
            guard.test_by_location = original
        gate = [key for key in rows if key[0].endswith("git/safety_tests/gate.rs")]
        self.assertGreaterEqual(len(gate), 2, "the hazard site and its RestoreCwd::drop")
        self.assertTrue(
            all(rows[key]["role"] == "test" for key in gate),
            f"{gate} must be test sites by the #[path] declaration alone",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
