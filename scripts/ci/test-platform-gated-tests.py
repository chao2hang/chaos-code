#!/usr/bin/env python3
"""Fixtures for `platform-gated-tests.py`.

The guard exists because a test behind `#[cfg(unix)]` disappears from another platform's build
without any runner reporting it, and the pass that turned the first Windows leg green used exactly
that: eleven persistent-shell assertions were gated rather than fixed. So these cases are the ways
a gate can stop meaning anything, and each has to stay red:

  * the platform sets themselves -- `cfg(unix)`, `cfg(target_os = "macos")`, `cfg(not(unix))`,
    `cfg(all(feature = "enforce", unix))`, and the two inheritance shapes (an enclosing
    `#[cfg(unix)] mod`, a `#![cfg(unix)]` file), which is where gating hides;
  * a `cfg_attr(<platform>, ignore)` row, which runs on the platforms the condition excludes --
    the opposite of a gate, and the shape no other inventory in the repository sees;
  * a gated test with no ledger row, and the same after `--write-baseline` has run, so deleting a
    row can never turn the failure into a pass;
  * a site whose cfg was rewritten to something narrower, and a test that gained a POSIX call while
    its row still claimed `none` -- both must report the row as stale rather than keep the old row.
    The line number must **not** do that: it is a navigation hint, and a gate that cries on every
    edit above a gated test is a gate that gets muted;
  * a reason column emptied, stubbed, or left as the import marker once the unreviewed budget is
    spent, including the ledger re-written by the guard's own writer;
  * the platform-blind file count in both directions: it grows when a second file is gated
    unix-only and clears when an ordinary test is added to the blind file, which is what proves the
    metric measures coverage rather than the presence of an attribute;
  * the `assumptions` column on the same three axes: a gate hiding a POSIX call is not named, a gate
    hiding nothing is, a POSIX word that appears only in a comment does not excuse the gate, the
    Windows direction works the other way, and the budget fails with the names rather than a count;
  * `#[cfg(unix)]` written in a module doc comment, a line comment and a string literal, which must
    not count -- and the same file made to count by turning one mention into live code, so a clean
    run proves the blanking works rather than that the scan finds nothing;
  * a test attribute and its `fn` on one line, which is a real spelling and must still be found.

One case runs the guard over this repository with its shipped ledger, so a source change that
outruns the ledger fails here as well as in CI, and one re-derives the `none` rows from the sources
instead of trusting the shipped column. The budgets those cases are run at are parsed out of the
two call sites that enforce them, one case checks the two call sites say the same thing, and one
checks each budget is one above a violation rather than far above the debt.

    python3 scripts/ci/test-platform-gated-tests.py
"""

from __future__ import annotations

import importlib.util
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("platform-gated-tests.py")
REPO = SCRIPT.parents[2]
LEDGER = SCRIPT.with_name("platform-gated-tests.tsv")
_spec = importlib.util.spec_from_file_location("platform_gated_tests", SCRIPT)
guard = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(guard)

HEADER = guard.HEADER
FUNCTION_COL = HEADER.index("function")
REASON_COL = HEADER.index("reason")
ALL = "linux+macos+other-unix+windows+other"
UNIX = "linux+macos+other-unix"
REVIEWED = "Only builds with ptrace; the container this gate runs in has no CAP_SYS_PTRACE."

# The four budgets are read from the call sites that enforce them instead of being restated here.
# They were restated, and on 2026-10-04 the ledger was lowered from 1108 to 1106 rows and the two
# call sites were updated while this file kept the old pair: the case meant to catch the ledger
# outrunning the caps then failed for holding the stale ones. Raising a budget is still a review --
# the gate itself fails the build at the cap -- and `the_two_call_sites_pass_the_same_four_numbers`
# is what keeps the two halves of the wiring from parting company.
BUDGET_FLAGS = (
    "--max-unreviewed",
    "--max-blind-windows",
    "--max-blind-macos",
    "--max-assumption-free",
)
CALL_SITES = (".github/workflows/ci.yml", "scripts/verify-in-docker.sh")
INVOCATION = "platform-gated-tests.py --quiet"


def call_site(rel: str, root: Path = REPO) -> str:
    """The gate's own command in `rel`, with backslash continuations joined.

    Anchored on the invocation rather than searched for file-wide, so a comment or a
    docstring that quotes `--max-unreviewed` cannot become the source of the number.
    """
    text = (root / rel).read_text(encoding="utf-8")
    at = text.find(INVOCATION)
    if at < 0:
        raise AssertionError(f"{rel} does not invoke {INVOCATION}")
    lines: list[str] = []
    for line in text[at:].splitlines():
        stripped = line.strip()
        lines.append(stripped.rstrip("\\"))
        if not stripped.endswith("\\"):
            break
    return " ".join(lines)


def wired_budgets(rel: str, root: Path = REPO) -> dict[str, str]:
    """The four `--max-*` values passed by the call site in `rel`, keyed by flag."""
    site = call_site(rel, root)
    out: dict[str, str] = {}
    for flag in BUDGET_FLAGS:
        found = re.search(re.escape(flag) + r" +(\d+)(?![0-9])", site)
        if found is None:
            raise AssertionError(f"{rel} runs the gate without {flag}: {site}")
        out[flag] = found.group(1)
    return out


BUDGETS: dict[str, dict[str, str]] = {rel: wired_budgets(rel) for rel in CALL_SITES}
LIVE = BUDGETS[CALL_SITES[0]]
LIVE_MAX_BLIND_WINDOWS = LIVE["--max-blind-windows"]
LIVE_MAX_BLIND_MACOS = LIVE["--max-blind-macos"]
LIVE_MAX_UNREVIEWED = LIVE["--max-unreviewed"]
LIVE_MAX_ASSUMPTION_FREE = LIVE["--max-assumption-free"]

MIXED = '''\
//! Notes. A doc comment mentioning `#[cfg(unix)]` and `#[test]` must not count as gating.

#[cfg(test)]
mod prose {
    #[test]
    fn quotes_a_gate() {
        let snippet = "#[cfg(unix)]\\n#[test]\\nfn quoted() {}";
        assert!(snippet.contains("cfg"));
    }
}

#[cfg(unix)]
#[test]
fn unix_only() {}

#[test]
fn runs_everywhere() {}

#[cfg(target_os = "macos")]
#[test]
fn mac_only() {
    assert!(true);
}

#[cfg(not(unix))]
#[test]
fn non_unix_only() {}

#[cfg(all(feature = "enforce", unix))]
#[test]
fn enforce_and_unix() {}

#[cfg_attr(unix, ignore = "flaky where pty job control differs")]
#[test]
fn conditionally_ignored() {}

#[cfg(unix)] #[test] fn one_line_spelling() {}

#[cfg(unix)]
mod inherited {
    #[test]
    fn inherits_the_module_gate() {}

    #[cfg(target_os = "linux")]
    #[test]
    fn narrows_further_than_the_module() {}
}
'''

SECOND_UNIX_TEST = '''
#[cfg(unix)]
#[test]
fn second_unix_only_site() {}
'''

ORDINARY_TEST = '''
#[test]
fn an_ordinary_test_this_file_also_runs() {}
'''

PROSE_ONLY = '''\
//! Notes about gating.

pub fn helper() -> u32 {
    // #[cfg(unix)]
    4
}

#[test]
fn live() {
    assert_eq!(helper(), 4);
}
'''

PROSE_LIVE = '''\
//! Notes about gating.

pub fn helper() -> u32 {
    // #[cfg(unix)]
    4
}

#[cfg(unix)]
#[test]
fn live() {
    assert_eq!(helper(), 4);
}
'''

WHOLE_FILE = """\
#![cfg(unix)]

#[test]
fn the_whole_file_is_gated() {}
"""

WHOLE_FILE_MOVED = """\
#![cfg(unix)]
// one more line than the ledger expects

#[test]
fn the_whole_file_is_gated() {}
"""

UNIX_ONLY_FILE = "#[cfg(unix)]\n#[test]\nfn blind_one() {}\n"

# Three shapes of the same gated test, which is what makes the `assumptions` column testable: one
# that really needs POSIX, one that could run anywhere, and one whose only mention of POSIX is a
# comment. A column that cannot tell these apart cannot name the tests worth un-gating.
ASSUMPTION_FIXTURE = '''\
#[cfg(unix)]
#[test]
fn really_needs_posix() {
    std::os::unix::fs::symlink("a", "b").unwrap();
}

#[cfg(unix)]
#[test]
fn could_run_anywhere() {
    assert_eq!(2 + 2, 4);
}

#[cfg(unix)]
#[test]
fn mentions_posix_only_in_a_comment() {
    // std::os::unix::fs::symlink is not what this exercises any more
    assert_eq!(2 + 2, 4);
}

#[cfg(windows)]
#[test]
fn windows_only_with_a_windows_call() {
    std::process::Command::new("cmd.exe").arg("/c").arg("ver").status().unwrap();
}

#[cfg(windows)]
#[test]
fn windows_only_with_nothing_windows() {
    assert_eq!(2 + 2, 4);
}
'''

POSIX_CALL_ADDED = '''\
#[cfg(unix)]
#[test]
fn another_portable_gate() {
    std::fs::set_permissions("f", std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    assert_eq!(2 + 2, 4);
}
'''

PORTABLE_ONLY = '''\
#[cfg(unix)]
#[test]
fn another_portable_gate() {
    assert_eq!(2 + 2, 4);
}
'''


def write(root: Path, rel: str, text: str) -> Path:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


def run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), *args], capture_output=True, text=True, check=False
    )


def table(root: Path) -> dict[str, dict[str, str]]:
    """Ledger rows of a fixture tree, keyed by function name."""
    out = run("--root", str(root), "--quiet", "--table")
    assert out.returncode == 0, out.stderr
    keyed: dict[str, dict[str, str]] = {}
    for line in out.stdout.split("\n"):
        if not line or line.startswith("kind\t"):
            continue
        cols = line.split("\t")
        if len(cols) != len(HEADER):
            continue
        keyed[cols[FUNCTION_COL]] = dict(zip(HEADER, cols))
    return keyed


BLIND_HEADING = "files with gated tests and no test that runs on the platform:"


def blind_count(root: Path, platform: str) -> int:
    """The count from the blind-files section only.

    The summary prints a padded platform column twice, once for "would compile on" and once for
    the blind counts, and the first section ends in "N of M" rather than a bare number. Reading the
    wrong section would silently return the compile count, so the section heading is what selects.
    """
    out = run("--root", str(root))
    assert out.returncode == 0, out.stderr
    lines = out.stdout.split("\n")
    start = lines.index(BLIND_HEADING)
    for line in lines[start + 1 :]:
        if line.startswith(f"  {platform:<11}"):
            return int(line.split()[-1])
    raise AssertionError(f"no blind count for {platform} in:\n{out.stdout}")


def is_row(line: str) -> bool:
    return "\t" in line and not line.startswith("#") and line != "\t".join(HEADER)


def set_reason(ledger: Path, function: str, reason: str) -> None:
    lines = ledger.read_text(encoding="utf-8").split("\n")
    out = []
    for line in lines:
        cols = line.split("\t")
        if is_row(line) and len(cols) == len(HEADER) and cols[FUNCTION_COL] == function:
            cols[REASON_COL] = reason
            line = "\t".join(cols)
        out.append(line)
    ledger.write_text("\n".join(out), encoding="utf-8")


def review_all(ledger: Path) -> None:
    lines = ledger.read_text(encoding="utf-8").split("\n")
    out = []
    for line in lines:
        if is_row(line):
            cols = line.split("\t")
            cols[REASON_COL] = REVIEWED
            line = "\t".join(cols)
        out.append(line)
    ledger.write_text("\n".join(out), encoding="utf-8")


class ScanSemantics(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        write(self.root, "crates/demo/Cargo.toml", '[package]\nname = "demo"\n')
        write(self.root, "crates/demo/src/lib.rs", MIXED)
        write(self.root, "crates/demo/src/whole_file.rs", WHOLE_FILE)
        write(self.root, "crates/demo/src/prose_only.rs", PROSE_ONLY)
        self.addCleanup(self.tmp.cleanup)

    def test_platform_sets_are_the_ones_rust_would_compile(self) -> None:
        rows = table(self.root)
        self.assertEqual(rows["unix_only"]["runs_on"], UNIX)
        self.assertEqual(rows["mac_only"]["runs_on"], "macos")
        self.assertEqual(rows["non_unix_only"]["runs_on"], "windows+other")

    def test_an_ungated_test_belongs_to_no_ledger_row(self) -> None:
        rows = table(self.root)
        self.assertNotIn("runs_everywhere", rows)
        self.assertNotIn("quotes_a_gate", rows)

    def test_feature_and_platform_gating_are_kept_apart(self) -> None:
        rows = table(self.root)
        self.assertEqual(rows["enforce_and_unix"]["runs_on"], UNIX)
        self.assertEqual(rows["enforce_and_unix"]["extra_cfg"], 'feature = "enforce"')

    def test_enclosing_module_gate_is_inherited(self) -> None:
        rows = table(self.root)
        self.assertEqual(rows["inherits_the_module_gate"]["runs_on"], UNIX)
        # The narrower of two gates wins: `cfg(unix)` outside, `cfg(target_os = "linux")` inside.
        self.assertEqual(rows["narrows_further_than_the_module"]["runs_on"], "linux")

    def test_file_level_inner_cfg_gates_every_test_in_the_file(self) -> None:
        rows = table(self.root)
        self.assertEqual(rows["the_whole_file_is_gated"]["runs_on"], UNIX)
        self.assertEqual(rows["the_whole_file_is_gated"]["crate"], "demo")

    def test_attribute_and_fn_on_one_line_are_still_found(self) -> None:
        rows = table(self.root)
        self.assertEqual(rows["one_line_spelling"]["runs_on"], UNIX)

    def test_conditional_ignore_runs_where_the_condition_is_false(self) -> None:
        rows = table(self.root)
        self.assertEqual(rows["conditionally_ignored"]["kind"], "cfg-attr-ignore")
        # `cfg_attr(unix, ignore)` compiles everywhere and is skipped on unix, so the debt is on
        # the three unix platforms and the row must not read as a unix-only test.
        self.assertEqual(rows["conditionally_ignored"]["runs_on"], "windows+other")
        self.assertEqual(rows["unix_only"]["kind"], "cfg-gate")

    def test_gating_written_only_in_prose_is_not_a_gate(self) -> None:
        self.assertNotIn("live", table(self.root))
        write(self.root, "crates/demo/src/prose_only.rs", PROSE_LIVE)
        self.assertEqual(table(self.root)["live"]["runs_on"], UNIX)

    def test_a_gate_in_a_string_literal_is_not_a_gate(self) -> None:
        self.assertNotIn("quoted", table(self.root))


class LedgerDrift(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        write(self.root, "crates/demo/Cargo.toml", '[package]\nname = "demo"\n')
        write(self.root, "crates/demo/src/lib.rs", MIXED)
        write(self.root, "crates/demo/src/whole_file.rs", WHOLE_FILE)
        write(self.root, "crates/demo/src/prose_only.rs", PROSE_ONLY)
        write(self.root, "crates/demo/src/blind.rs", UNIX_ONLY_FILE)
        self.ledger = self.root / "ledger.tsv"
        out = run("--root", str(self.root), "--write-baseline", str(self.ledger))
        self.assertEqual(out.returncode, 0, out.stderr)
        review_all(self.ledger)
        self.addCleanup(self.tmp.cleanup)

    def check(self, *extra: str) -> subprocess.CompletedProcess[str]:
        return run("--root", str(self.root), "--quiet", "--check-baseline", str(self.ledger), *extra)

    def test_round_trip_of_the_writers_own_output(self) -> None:
        out = self.check()
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)

    def test_a_new_gated_test_needs_a_row(self) -> None:
        path = self.root / "crates/demo/src/lib.rs"
        path.write_text(path.read_text(encoding="utf-8") + SECOND_UNIX_TEST, encoding="utf-8")
        out = self.check()
        self.assertEqual(out.returncode, 1)
        self.assertIn("unlisted platform-gated test", out.stderr)
        self.assertIn("second_unix_only_site", out.stderr)

    def test_deleting_a_row_does_not_erase_the_debt(self) -> None:
        lines = [
            line
            for line in self.ledger.read_text(encoding="utf-8").split("\n")
            if "narrows_further_than_the_module" not in line
        ]
        self.ledger.write_text("\n".join(lines), encoding="utf-8")
        out = self.check()
        self.assertEqual(out.returncode, 1)
        self.assertIn("unlisted platform-gated test", out.stderr)

    def test_a_moved_line_does_not_demand_a_ledger_rewrite(self) -> None:
        # The line column is a navigation hint, not part of a row's identity. Without this, every
        # edit made above a gated test would force a ledger rewrite whose diff says nothing, and a
        # gate that cries on every edit is a gate that gets muted.
        write(self.root, "crates/demo/src/whole_file.rs", WHOLE_FILE_MOVED)
        out = self.check()
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)

    def test_rewriting_the_cfg_narrows_the_row_and_is_caught(self) -> None:
        path = self.root / "crates/demo/src/lib.rs"
        text = path.read_text(encoding="utf-8").replace(
            '#[cfg(target_os = "macos")]\n#[test]\nfn mac_only',
            '#[cfg(all(target_os = "macos", target_pointer_width = "64"))]\n#[test]\nfn mac_only',
        )
        self.assertNotEqual(text, path.read_text(encoding="utf-8"))
        path.write_text(text, encoding="utf-8")
        out = self.check()
        self.assertEqual(out.returncode, 1)
        self.assertIn("stale baseline row", out.stderr)
        self.assertIn("mac_only", out.stderr)

    def test_reason_column_must_say_something(self) -> None:
        set_reason(self.ledger, "unix_only", "n/a")
        out = self.check()
        self.assertEqual(out.returncode, 1)
        self.assertIn("reason is 3 chars", out.stderr)

    def test_the_import_marker_is_a_budget_not_a_free_pass(self) -> None:
        set_reason(self.ledger, "mac_only", guard.UNREVIEWED)
        self.assertEqual(self.check("--max-unreviewed", "1").returncode, 0)
        spent = self.check("--max-unreviewed", "0")
        self.assertEqual(spent.returncode, 1)
        self.assertIn("over the budget of 0", spent.stderr)

    def test_the_writer_cannot_launder_a_reviewed_ledger(self) -> None:
        # Re-running --write-baseline resets every reason to the marker. With a budget that was
        # affordable while the rows were reviewed, that has to fail rather than pass the tree back
        # into unreviewed debt.
        self.assertEqual(self.check("--max-unreviewed", "1").returncode, 0)
        out = run("--root", str(self.root), "--write-baseline", str(self.ledger))
        self.assertEqual(out.returncode, 0, out.stderr)
        spent = self.check("--max-unreviewed", "1")
        self.assertEqual(spent.returncode, 1)
        self.assertIn("import marker", spent.stderr)

    def test_duplicate_rows_are_rejected(self) -> None:
        lines = self.ledger.read_text(encoding="utf-8").split("\n")
        duplicate = next(line for line in lines if line.split("\t")[4:5] == ["unix_only"])
        self.ledger.write_text("\n".join(lines + [duplicate]), encoding="utf-8")
        out = self.check()
        self.assertEqual(out.returncode, 1)
        self.assertIn("duplicate baseline row", out.stderr)

    def test_a_malformed_row_fails_loudly(self) -> None:
        text = self.ledger.read_text(encoding="utf-8").rstrip("\n") + "\nshort\trow\n"
        self.ledger.write_text(text, encoding="utf-8")
        out = self.check()
        self.assertEqual(out.returncode, 1)
        self.assertIn(f"expected {len(HEADER)} columns", out.stderr + str(out))


class BlindFileCount(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        write(self.root, "crates/demo/Cargo.toml", '[package]\nname = "demo"\n')
        write(self.root, "crates/demo/src/one.rs", UNIX_ONLY_FILE)
        self.ledger = self.root / "ledger.tsv"
        run("--root", str(self.root), "--write-baseline", str(self.ledger))
        review_all(self.ledger)
        self.addCleanup(self.tmp.cleanup)

    def test_a_second_gated_file_raises_the_count(self) -> None:
        self.assertEqual(blind_count(self.root, "windows"), 1)
        write(self.root, "crates/demo/src/two.rs", "#[cfg(unix)]\n#[test]\nfn blind_two() {}\n")
        self.assertEqual(blind_count(self.root, "windows"), 2)
        out = run(
            "--root", str(self.root), "--quiet", "--check-baseline", str(self.ledger),
            "--max-blind-windows", "1",
        )
        self.assertEqual(out.returncode, 1)
        self.assertIn("2 files have platform-gated tests", out.stderr)

    def test_an_ordinary_test_in_the_file_clears_the_blindness(self) -> None:
        write(self.root, "crates/demo/src/one.rs", UNIX_ONLY_FILE + ORDINARY_TEST)
        self.assertEqual(blind_count(self.root, "windows"), 0)
        out = run(
            "--root", str(self.root), "--quiet", "--check-baseline", str(self.ledger),
            "--max-blind-windows", "1",
        )
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)

    def test_the_macos_cap_is_a_separate_switch(self) -> None:
        write(
            self.root,
            "crates/demo/src/mac.rs",
            '#[cfg(target_os = "linux")]\n#[test]\nfn linux_only_site() {}\n',
        )
        self.assertEqual(blind_count(self.root, "macos"), 1)
        out = run(
            "--root", str(self.root), "--quiet", "--check-baseline", str(self.ledger),
            "--max-blind-macos", "0",
        )
        self.assertEqual(out.returncode, 1)
        self.assertIn("nothing that runs on macOS", out.stderr)


class PlatformAssumptions(unittest.TestCase):
    """The `assumptions` column: which gated tests could actually be made to run elsewhere."""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        write(self.root, "crates/demo/Cargo.toml", '[package]\nname = "demo"\n')
        write(self.root, "crates/demo/src/assumptions.rs", ASSUMPTION_FIXTURE)
        write(self.root, "crates/demo/src/portable.rs", PORTABLE_ONLY)
        self.ledger = self.root / "ledger.tsv"
        out = run("--root", str(self.root), "--write-baseline", str(self.ledger))
        self.assertEqual(out.returncode, 0, out.stderr)
        review_all(self.ledger)
        self.addCleanup(self.tmp.cleanup)

    def check(self, *extra: str) -> subprocess.CompletedProcess[str]:
        return run("--root", str(self.root), "--quiet", "--check-baseline", str(self.ledger), *extra)

    def test_a_gate_hiding_a_posix_call_is_not_named(self) -> None:
        rows = table(self.root)
        self.assertIn("std-os-unix", rows["really_needs_posix"]["assumptions"])
        self.assertNotEqual(rows["really_needs_posix"]["assumptions"], "none")

    def test_a_gate_hiding_nothing_is_named(self) -> None:
        rows = table(self.root)
        self.assertEqual(rows["could_run_anywhere"]["assumptions"], "none")
        self.assertEqual(rows["windows_only_with_nothing_windows"]["assumptions"], "none")

    def test_a_windows_gate_hiding_a_windows_call_is_not_named(self) -> None:
        rows = table(self.root)
        self.assertIn("windows-shell", rows["windows_only_with_a_windows_call"]["assumptions"])

    def test_a_posix_word_in_a_comment_does_not_excuse_the_gate(self) -> None:
        rows = table(self.root)
        self.assertEqual(rows["mentions_posix_only_in_a_comment"]["assumptions"], "none")

    def test_the_budget_counts_named_rows_and_says_which(self) -> None:
        # Four rows in this fixture have no assumption in them; the budget has to be able to hold
        # that number and the failure has to name them, because the list is the point.
        self.assertEqual(self.check("--max-assumption-free", "4").returncode, 0)
        out = self.check("--max-assumption-free", "3")
        self.assertEqual(out.returncode, 1)
        self.assertIn("over the budget of 3", out.stderr)
        self.assertIn("could_run_anywhere", out.stderr)

    def test_the_named_list_can_be_printed_without_a_budget(self) -> None:
        out = run("--root", str(self.root), "--quiet", "--list-assumption-free")
        self.assertEqual(out.returncode, 0, out.stderr)
        listed = out.stdout
        self.assertIn("could_run_anywhere", listed)
        self.assertIn("windows_only_with_nothing_windows", listed)
        self.assertNotIn("really_needs_posix", listed)
        self.assertNotIn("windows_only_with_a_windows_call", listed)

    def test_growing_a_posix_assumption_outruns_the_row(self) -> None:
        # The column is part of the row's identity: a test that gains a POSIX call while its ledger
        # row still says `none` must be re-derived, not accepted with the old claim.
        write(self.root, "crates/demo/src/portable.rs", PORTABLE_ONLY)
        out = run("--root", str(self.root), "--write-baseline", str(self.ledger))
        self.assertEqual(out.returncode, 0, out.stderr)
        review_all(self.ledger)
        self.assertEqual(table(self.root)["another_portable_gate"]["assumptions"], "none")
        self.assertEqual(self.check().returncode, 0)
        write(self.root, "crates/demo/src/portable.rs", POSIX_CALL_ADDED)
        out = self.check()
        self.assertEqual(out.returncode, 1)
        self.assertIn("stale baseline row", out.stderr)
        self.assertIn("assumptions=", out.stderr)
        self.assertIn("std-os-unix", out.stderr)


class BudgetParsing(unittest.TestCase):
    """The reader that pulls the four budgets out of the call sites.

    Fixtures, because the thing to pin down is where the number is allowed to come from: a
    file-wide search would happily read a comment that quotes a flag, which is how a guard ends up
    asserting a number nothing enforces.
    """

    def parse(self, text: str) -> dict[str, str]:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "ci.yml").write_text(text, encoding="utf-8")
            return wired_budgets("ci.yml", root)

    def test_a_prose_mention_of_a_flag_is_not_the_budget(self) -> None:
        prose = (
            "# The budgets were `--max-unreviewed 9999 --max-blind-windows 8 --max-blind-macos 9\n"
            "# --max-assumption-free 7` before the ledger was imported.\n"
            "          python3 scripts/ci/platform-gated-tests.py --quiet \\\n"
            "            --check-baseline scripts/ci/platform-gated-tests.tsv \\\n"
            "            --max-unreviewed 1106 --max-blind-windows 74 --max-blind-macos 11 \\\n"
            "            --max-assumption-free 441\n"
            "          python3 scripts/ci/next-gate.py --max-unreviewed 3\n"
        )
        self.assertEqual(
            self.parse(prose),
            {
                "--max-unreviewed": "1106",
                "--max-blind-windows": "74",
                "--max-blind-macos": "11",
                "--max-assumption-free": "441",
            },
        )

    def test_the_window_stops_at_the_command(self) -> None:
        """A later step in the same job must not be able to supply a flag either."""
        prose = (
            "          python3 scripts/ci/platform-gated-tests.py --quiet \\\n"
            "            --max-unreviewed 1 --max-blind-windows 2 --max-blind-macos 3 \\\n"
            "            --max-assumption-free 4\n"
            "          python3 scripts/ci/other.py --max-unreviewed 9\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "ci.yml").write_text(prose, encoding="utf-8")
            site = call_site("ci.yml", root)
            self.assertNotIn("other.py", site)
            self.assertEqual(wired_budgets("ci.yml", root)["--max-unreviewed"], "1")

    def test_a_file_that_never_runs_the_gate_is_an_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "ci.yml").write_text("python3 scripts/ci/other.py\n", encoding="utf-8")
            with self.assertRaises(AssertionError) as caught:
                wired_budgets("ci.yml", root)
        self.assertIn(INVOCATION, str(caught.exception))

    def test_a_call_site_missing_one_budget_is_an_error(self) -> None:
        prose = (
            "python3 scripts/ci/platform-gated-tests.py --quiet \\\n"
            "  --max-unreviewed 1 --max-blind-windows 2 --max-assumption-free 4\n"
        )
        with self.assertRaises(AssertionError) as caught:
            self.parse(prose)
        self.assertIn("--max-blind-macos", str(caught.exception))


class LiveRepository(unittest.TestCase):
    """The shipped ledger against the shipped sources, at the caps CI enforces."""

    def unreviewed_rows(self) -> int:
        text = LEDGER.read_text(encoding="utf-8")
        return sum(1 for line in text.split("\n") if line.endswith("\t" + guard.UNREVIEWED))

    def test_the_shipped_ledger_carries_unreviewed_rows_for_the_budget_to_mean_anything(self) -> None:
        self.assertGreater(self.unreviewed_rows(), 100)
        self.assertEqual(self.unreviewed_rows(), int(LIVE_MAX_UNREVIEWED))

    def test_the_shipped_ledger_matches_the_shipped_sources(self) -> None:
        out = run(
            "--quiet",
            "--check-baseline",
            str(LEDGER),
            "--max-unreviewed",
            LIVE_MAX_UNREVIEWED,
            "--max-blind-windows",
            LIVE_MAX_BLIND_WINDOWS,
            "--max-blind-macos",
            LIVE_MAX_BLIND_MACOS,
            "--max-assumption-free",
            LIVE_MAX_ASSUMPTION_FREE,
        )
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)

    def test_the_two_call_sites_pass_the_same_four_numbers(self) -> None:
        """ci.yml and verify-in-docker.sh each pass the budgets, and nothing compared them.

        `check-guard-wiring.py` proves both name the gate; it says nothing about the numbers, so
        the local entry point could hold one set and CI another, and the leg nobody runs locally
        would be the one enforcing the real cap.
        """
        first, second = (BUDGETS[rel] for rel in CALL_SITES)
        self.assertEqual(first, second, {rel: BUDGETS[rel] for rel in CALL_SITES})

    def test_every_shipped_budget_is_exactly_at_the_measured_debt(self) -> None:
        """Each cap is one above a violation, so none of the four has gone slack.

        One run with all four caps lowered together: the gate reports every budget it exceeded, so
        this asks for all four messages and proves each number is load-bearing on today's tree. A
        cap set far above the debt would pass the check above and police nothing.
        """
        lowered = []
        for flag in BUDGET_FLAGS:
            lowered += [flag, str(int(LIVE[flag]) - 1)]
        out = run(
            "--quiet",
            "--check-baseline",
            str(LEDGER),
            *lowered,
        )
        self.assertEqual(out.returncode, 1, out.stdout + out.stderr)
        text = out.stdout + out.stderr
        for flag, phrase in (
            ("--max-unreviewed", "rows still carry the import marker"),
            ("--max-blind-windows", "nothing that runs on Windows"),
            ("--max-blind-macos", "nothing that runs on macOS"),
            ("--max-assumption-free", "candidates to un-gate"),
        ):
            with self.subTest(flag=flag):
                self.assertIn(phrase, text)
        for flag in BUDGET_FLAGS:
            with self.subTest(flag=flag):
                self.assertIn(f"over the budget of {int(LIVE[flag]) - 1}", text)

    def test_the_shipped_ledger_names_gates_with_nothing_to_gate(self) -> None:
        """The third thing the debt note asked for: name the gated tests with no POSIX assumption.

        A count would satisfy the wording and prove nothing, so this checks the real list: that the
        shipped ledger has `none` rows, that they are spread over more than one crate, and that a
        named row is a row whose body really carries no marker (re-derived here, not trusted).
        """
        rows = guard.read_baseline(LEDGER)
        named = [r for r in rows if r["assumptions"] == "none" and r["kind"] != "cfg-attr-ignore"]
        self.assertGreater(len(named), 100, "the ledger names no gates worth un-gating")
        self.assertEqual(len(named), int(LIVE_MAX_ASSUMPTION_FREE))
        self.assertGreater(len({r["crate"] for r in named}), 5)
        # The row's marker set is what the guard computes from the source today. A row whose column
        # was hand-edited to `none` would disagree with the fresh scan and fail the ledger check.
        fresh = {
            (r["file"], r["function"]): r["assumptions"]
            for r in guard.ledger_rows(guard.scan(REPO))
        }
        for row in named[:25]:
            with self.subTest(row=f"{row['file']}:{row['function']}"):
                self.assertEqual(fresh[(row["file"], row["function"])], "none")

    def test_every_shipped_row_names_a_platform_subset_and_a_crate(self) -> None:
        for row in guard.read_baseline(LEDGER):
            with self.subTest(row=f"{row['file']}:{row['line']}"):
                self.assertTrue(row["runs_on"], row)
                self.assertTrue(row["crate"], row)
                self.assertIn(row["kind"], ("cfg-gate", "cfg-attr-ignore", "cfg-gate-and-ignore"))
                self.assertNotIn("linux+macos+other-unix+windows+other", row["runs_on"])


class EvaluatorUnits(unittest.TestCase):
    def test_cfg_expressions_map_to_platform_sets(self) -> None:
        cases = {
            "unix": {"linux", "macos", "other-unix"},
            "windows": {"windows"},
            "not(unix)": {"windows", "other"},
            'not(target_os = "windows")': {"linux", "macos", "other-unix", "other"},
            'any(target_os = "linux", target_os = "macos")': {"linux", "macos"},
            'all(unix, not(target_os = "macos"))': {"linux", "other-unix"},
            'all(feature = "enforce", unix)': {"linux", "macos", "other-unix"},
            'target_family = "unix"': {"linux", "macos", "other-unix"},
            'target_os = "ios"': {"macos"},
            'feature = "x"': {"linux", "macos", "other-unix", "windows", "other"},
        }
        for expr, want in cases.items():
            with self.subTest(expr=expr):
                self.assertEqual(set(guard.eval_cfg(expr)[0]), want)

    def test_an_unknown_target_os_is_reported_instead_of_selecting_nothing(self) -> None:
        platforms, extra = guard.eval_cfg('target_os = "haiku-os"')
        self.assertEqual(set(platforms), set(guard.ALL_PLATFORMS))
        self.assertEqual(extra, ['target_os = "haiku-os"'])


if __name__ == "__main__":
    unittest.main(verbosity=2)
