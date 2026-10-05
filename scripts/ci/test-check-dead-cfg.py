#!/usr/bin/env python3
"""Fixtures for `check-dead-cfg.py`.

The guard exists because a `#[cfg(any())]` over 23 tests sat in `auth_method.rs` for ten weeks,
invisible to `cargo test`, to the `#[ignore]` ledger and to the census's production count at the
same time. So these cases are not "does it spot the word `any`" -- each one pins a decision the
folder has to get right, and each is paired with the shape that must stay silent, because a
scanner that reported everything would pass a one-sided test too.

Decisions pinned here, in the order the cases appear:

- the fold itself: `any()` is FALSE, `all()` is TRUE, `not()` flips, `all` falls to its FALSE part
  and `any` to its TRUE part, and a leaf is UNKNOWN because the guard cannot know the feature
  tree, the target or whether `test` is on;
- a leaf carries its value: `all(target_os = "linux", not(target_os = "linux"))` is a
  contradiction and is refused, while `all(target_os = "linux", not(target_os = "macos"))` -- a
  real shape in this repository -- is not, and `all(feature = "a", feature = "b")` is two leaves
  a build may well enable together;
- never-true gates are refused in every spelling: the item attribute, the file-level `#![cfg(..)]`
  and the `cfg!(..)` macro, and in nested form through `all`/`any`/`not`;
- TRUE is not refused. `#[cfg(all())]` and `any(f, not(f))` compile what they look like they
  compile; refusing them would train people to delete the gate rather than the dead text;
- `cfg_attr` folds its condition only: the attribute list after the comma is not a predicate, so
  `cfg_attr(any(), deny(..))` is a finding and `cfg_attr(unix, allow(..))` is silent;
- comments and literals are not code: the same dead predicate is written in a line comment, a
  block comment, a nested block comment, a doc comment, a `r#".."#` literal and a plain string --
  all silent -- and then the identical file gets one real attribute, which must report exactly
  that one. Without the second half, "nothing found" cannot be told apart from "nothing scanned";
- a function *named* `cfg` is not a predicate. `fn cfg()` and its `&cfg()` call sites in
  `xai-grok-compaction/src/code_compaction/compact.rs` matched a looser pattern 10 times and each
  match became a bogus finding, which is how the pattern got anchored to `#[cfg(` and `cfg!(`;
- text that cannot be folded is reported, not skipped: an unclosed predicate and a predicate with
  trailing tokens both have to come back non-zero, because quietly skipping what the scanner
  cannot parse is the same hole the guard was written to close;
- the scan roots are `crates/` and `bin/`, and `target/` inside a crate is build output;
- a tree with no Rust sources exits 2 with a complaint, never 0;
- one case runs the guard on this repository, so it cannot be green in fixtures and red on the
  tree it was written for.

    python3 scripts/ci/test-check-dead-cfg.py
"""

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-dead-cfg.py")
REPO = SCRIPT.parents[2]
_spec = importlib.util.spec_from_file_location("check_dead_cfg", SCRIPT)
guard = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(guard)


def run(root: Path, *extra: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--root", str(root), *extra],
        capture_output=True,
        text=True,
    )


def write(root: Path, rel: str, text: str) -> Path:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


class FoldTests(unittest.TestCase):
    """The three-valued fold, asserted one predicate at a time."""

    def state(self, args: str) -> str:
        return guard.fold(args).state

    def test_the_constants_fold_the_way_rustc_evaluates_them(self) -> None:
        self.assertEqual(self.state("any()"), guard.FALSE)
        self.assertEqual(self.state("all()"), guard.TRUE)
        self.assertEqual(self.state("not(any())"), guard.TRUE)
        self.assertEqual(self.state("not(all())"), guard.FALSE)

    def test_leaves_are_undecided_because_the_build_is_unknown(self) -> None:
        for args in ("unix", "test", 'feature = "gated"', 'target_os = "linux"', "not(unix)"):
            self.assertEqual(self.state(args), guard.UNKNOWN, args)

    def test_all_falls_to_its_false_part_and_any_to_its_true_part(self) -> None:
        self.assertEqual(self.state('all(unix, any())'), guard.FALSE)
        self.assertEqual(self.state('any(unix, all())'), guard.TRUE)
        self.assertEqual(self.state('any(any(), any())'), guard.FALSE)
        self.assertEqual(self.state('all(all(), all())'), guard.TRUE)
        self.assertEqual(self.state('any(any(), all())'), guard.TRUE)
        self.assertEqual(self.state('all(any(), all())'), guard.FALSE)
        self.assertEqual(self.state("all(unix, test)"), guard.UNKNOWN)
        self.assertEqual(self.state("any(unix, any())"), guard.UNKNOWN)

    def test_a_leaf_and_its_own_negation_contradict_only_under_all(self) -> None:
        self.assertEqual(self.state('all(feature = "a", not(feature = "a"))'), guard.FALSE)
        self.assertEqual(self.state('any(feature = "a", not(feature = "a"))'), guard.TRUE)
        self.assertEqual(self.state('all(target_os = "linux", not(target_os = "linux"))'), guard.FALSE)
        # Two different values of the same key are two leaves, not a contradiction: a Linux build
        # satisfies `not(target_os = "macos")` while it is a Linux build.
        self.assertEqual(self.state('all(target_os = "linux", not(target_os = "macos"))'), guard.UNKNOWN)
        self.assertEqual(self.state('all(target_os = "linux", target_os = "macos")'), guard.UNKNOWN)
        self.assertEqual(self.state('all(feature = "a", feature = "b")'), guard.UNKNOWN)

    def test_nesting_carries_the_fold_through_not(self) -> None:
        self.assertEqual(self.state("all(any(), not(all()))"), guard.FALSE)
        self.assertEqual(self.state("not(any(all(), any()))"), guard.FALSE)
        self.assertEqual(self.state("any(all(unix, not(unix)), not(any()))"), guard.TRUE)

    def test_a_predicate_the_tokenizer_cannot_read_is_an_error_not_a_guess(self) -> None:
        for args in ("all(unix))x", "feature =", "(unix"):
            with self.assertRaises(ValueError, msg=args):
                guard.fold(args)


class ScanTests(unittest.TestCase):
    """CLI behaviour over a generated tree."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)

    def demo(self, body: str, rel: str = "crates/demo/src/lib.rs") -> Path:
        return write(self.root, rel, body)

    def test_a_dead_gate_is_named_with_file_and_line(self) -> None:
        self.demo(
            "#[cfg(test)]\nmod live {\n    #[test]\n    fn one() {}\n}\n"
            "#[cfg(any())]\nmod dead {\n    #[test]\n    fn two() {}\n}\n"
        )
        result = run(self.root)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("crates/demo/src/lib.rs:6", result.stdout)
        self.assertIn("cfg(any())", result.stdout)
        self.assertNotIn("lib.rs:1:", result.stdout)

    def test_the_identically_shaped_live_gate_is_silent(self) -> None:
        self.demo("#[cfg(test)]\nmod live {\n    #[test]\n    fn one() {}\n}\n")
        result = run(self.root)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("no cfg predicate that can never hold", result.stdout)

    def test_the_dead_gate_is_refused_in_every_spelling(self) -> None:
        cases = [
            ("item attribute", "#[cfg(any())]\nfn gone() {}\n"),
            ("file inner attribute", "#![cfg(any())]\nfn gone() {}\n"),
            ("macro in an expression", "fn pick() -> u8 {\n    if cfg!(any()) { 1 } else { 0 }\n}\n"),
            ("nested under all", '#[cfg(all(feature = "x", any()))]\nfn gone() {}\n'),
            ("nested under not", "#[cfg(not(all()))]\nfn gone() {}\n"),
            ("contradiction", '#[cfg(all(target_os = "linux", not(target_os = "linux")))]\nfn gone() {}\n'),
            (
                "cfg_attr condition",
                "#[cfg_attr(any(), allow(dead_code))]\nfn gone() {}\n",
            ),
        ]
        for what, body in cases:
            with self.subTest(what=what):
                with tempfile.TemporaryDirectory() as each:
                    root = Path(each)
                    write(root, "crates/demo/src/lib.rs", body)
                    result = run(root)
                    self.assertEqual(result.returncode, 1, f"{what}: {result.stdout}")
                    self.assertTrue(
                        any(
                            "lib.rs:" in line and "can never hold" in line
                            for line in result.stdout.splitlines()
                        ),
                        f"{what}: no finding line in {result.stdout}",
                    )

    def test_a_live_gate_of_the_same_shape_is_silent(self) -> None:
        cases = [
            ("empty all is needless but true", "#[cfg(all())]\nfn ships() {}\n"),
            ("tautology", '#[cfg(any(feature = "x", not(feature = "x")))]\nfn ships() {}\n'),
            ("two values of one key", '#[cfg(all(target_os = "linux", not(target_os = "macos")))]\nfn ships() {}\n'),
            ("test gate", "#[cfg(test)]\nmod tests {}\n"),
            ("cfg_attr condition is live", '#[cfg_attr(unix, allow(dead_code))]\nfn ships() {}\n'),
            ("cfg_attr with a real condition", '#[cfg_attr(feature = "x", deny(warnings))]\nfn ships() {}\n'),
            ("macro is live", "fn pick() -> u8 {\n    if cfg!(unix) { 1 } else { 0 }\n}\n"),
        ]
        for what, body in cases:
            with self.subTest(what=what):
                with tempfile.TemporaryDirectory() as each:
                    root = Path(each)
                    write(root, "crates/demo/src/lib.rs", body)
                    result = run(root)
                    self.assertEqual(result.returncode, 0, f"{what}: {result.stdout}")

    def test_written_prose_and_literals_are_not_predicates(self) -> None:
        noisy = (
            "// #[cfg(any())] in a line comment\n"
            "/* #[cfg(any())] in a block comment /* nested */ */\n"
            "/// `#[cfg(any())]` in a doc comment\n"
            'const GEN: &str = r#"#[cfg(any())]"#;\n'
            'const PLAIN: &str = "#[cfg(any())] and (unbalanced";\n'
            "fn quote() -> char {\n    '(' \n}\n"
        )
        self.demo(noisy)
        quiet = run(self.root)
        self.assertEqual(quiet.returncode, 0, quiet.stdout + quiet.stderr)

        # Same file, one real attribute: the clean run above was blanking, not blindness.
        self.demo(noisy + "#[cfg(any())]\nfn gone() {}\n")
        loud = run(self.root)
        self.assertEqual(loud.returncode, 1, loud.stdout + loud.stderr)
        self.assertEqual(loud.stdout.count("cfg(any()) can never hold"), 1, loud.stdout)

    def test_a_function_named_cfg_is_not_a_predicate(self) -> None:
        # The shape that made a looser pattern report 10 findings in xai-grok-compaction.
        self.demo(
            "pub struct Cfg;\nfn cfg() -> Cfg {\n    Cfg\n}\n"
            "fn use_it() -> Cfg {\n    let _ = cfg();\n    cfg()\n}\n"
        )
        result = run(self.root)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_an_unreadable_predicate_is_reported_not_skipped(self) -> None:
        for what, body in [
            ("never closes", "#[cfg(any(]\nfn gone() {}\n"),
            ("stray parenthesis", "#[cfg((unix))]\nfn gone() {}\n"),
            ("key with no value", "#[cfg(feature =)]\nfn gone() {}\n"),
            ("no predicate at all", "#[cfg()]\nfn gone() {}\n"),
        ]:
            with self.subTest(what=what):
                with tempfile.TemporaryDirectory() as each:
                    root = Path(each)
                    write(root, "crates/demo/src/lib.rs", body)
                    result = run(root)
                    self.assertEqual(result.returncode, 1, f"{what}: {result.stdout}")

    def test_bin_is_scanned_and_build_output_is_not(self) -> None:
        write(self.root, "bin/tool.rs", "#[cfg(any())]\nfn gone() {}\n")
        result = run(self.root)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("bin/tool.rs", result.stdout)

        write(self.root, "crates/demo/target/generated.rs", "#[cfg(any())]\nfn gone() {}\n")
        write(self.root, "bin/tool.rs", "fn ships() {}\n")
        result = run(self.root)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_a_tree_with_nothing_to_scan_is_an_error_not_a_pass(self) -> None:
        write(self.root, "notes/readme.md", "#[cfg(any())]\n")
        result = run(self.root)
        self.assertEqual(result.returncode, 2)
        self.assertIn("nothing was checked", result.stderr)

    def test_verbose_reports_what_was_examined(self) -> None:
        self.demo('#[cfg(feature = "maybe")]\nfn maybe() {}\n')
        result = run(self.root, "--verbose")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('cfg(feature = "maybe") -> unknown', result.stdout)
        self.assertIn("predicates examined across 1 files", result.stdout)

    def test_this_repository_is_clean(self) -> None:
        result = run(REPO)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("files, no cfg predicate that can never hold", result.stdout)


if __name__ == "__main__":
    unittest.main(verbosity=2)
