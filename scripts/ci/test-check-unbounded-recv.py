#!/usr/bin/env python3
"""Fixtures for `check-unbounded-recv.py`.

The check exists because one `.recv().await.expect(..)` in one `#[tokio::test]` cost a whole CI
job: the run sat silent from the moment the wait began until `timeout-minutes` killed the job,
and the report said `cancelled` with no failing test to point at. These cases are the ways that
idiom hides, plus the ways the check itself could go quietly useless:

  * `rx.recv_bounded("event").await` passes, and the run says it read the tree;
  * `.recv().await.expect("phrase")` fails, naming the file, the line and the phrase;
  * so does `.recv().await.unwrap()`, which carries no phrase of its own;
  * a chain rustfmt broke across four lines is still found, at the line the `recv()` sits on;
  * the same call inside a plain `impl` method is production code and passes;
  * a site under `tests/`, in a `src/.../*_tests.rs` file, and inside a `#[cfg(test)] mod` is
    each found -- and the `*_tests.rs` case is what proves the suffix is matched as a *suffix*:
    the first version of this guard matched it as a whole name and silently dropped 5 of 232
    sites;
  * `.recv().await.expect(..)` inside `#[cfg(any())]` does not count, because no build contains
    it and so no build can hang on it -- the dropped module is nested inside the `#[cfg(test)]`
    mod, so the never-cfg subtraction is the only mechanism that can exclude it, and a sibling
    test outside it still has to be reported;
  * the idiom written inside a raw string, a char literal or a comment does not count, and the
    same file is then made to count by turning one mention into live code, so a green run proves
    the blanking worked rather than that nothing was read;
  * `third_party/` and `target/` are not scanned -- the first is vendored code, the second would
    otherwise decide the gate's runtime on a built tree;
  * the ratchet refuses a crate above its row and a crate below it, because a ceiling nobody
    tightens is a ledger nobody reads;
  * `--write-baseline` records what it counted, and a later run against it sees the same number;
  * a root with no Rust sources exits 2 rather than reporting a clean tree;
  * the shipped tree is clean, and every one of the 232 sites that batch removed stays gone.

    python3 scripts/ci/test-check-unbounded-recv.py
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-unbounded-recv.py")
REPO = SCRIPT.parents[2]
SRC = "crates/codegen/widget/src/lib.rs"

# One production method and three test waits, in the shapes the 232 converted sites actually
# came in. Line numbers in the assertions are read back out of this text, so an edit here cannot
# silently point an assertion at the wrong call.
SVC = '''\
use tokio::sync::mpsc;

pub struct Service;

impl Service {
    pub async fn drain(&mut self, rx: &mut mpsc::Receiver<u32>) -> u32 {
        rx.recv().await.expect("a request from the actor")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reads_one_event() {
        let (_tx, mut rx) = mpsc::channel(1);
        let v = rx.recv().await.expect("an event");
        assert_eq!(v, 1);
    }

    #[tokio::test]
    async fn reads_one_event_unwrapped() {
        let (_tx, mut rx) = mpsc::channel(1);
        let v = rx.recv().await.unwrap();
        assert_eq!(v, 1);
    }

    #[tokio::test]
    async fn broken_across_lines() {
        let (_tx, mut rx) = mpsc::channel(1);
        let v = rx
            .recv()
            .await
            .unwrap();
        assert_eq!(v, 1);
    }
}
'''

# The same three tests the way the conversion writes them, with the production method left
# exactly as it was -- an unbounded `recv()` in an actor is the design, not the debt.
BOUNDED = '''\
use tokio::sync::mpsc;
use xai_grok_test_support::recv_wait::RecvBounded;

pub struct Service;

impl Service {
    pub async fn drain(&mut self, rx: &mut mpsc::Receiver<u32>) -> u32 {
        rx.recv().await.expect("a request from the actor")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xai_grok_test_support::recv_wait::RecvBounded;

    #[tokio::test]
    async fn reads_one_event() {
        let (_tx, mut rx) = mpsc::channel(1);
        let v = rx.recv_bounded("an event").await;
        assert_eq!(v, 1);
    }

    #[tokio::test]
    async fn reads_one_event_unwrapped() {
        let (_tx, mut rx) = mpsc::channel(1);
        let v = rx.recv_bounded("v").await;
        assert_eq!(v, 1);
    }

    #[tokio::test]
    async fn broken_across_lines() {
        let (_tx, mut rx) = mpsc::channel(1);
        let v = rx.recv_bounded("v").await;
        assert_eq!(v, 1);
    }
}
'''

PRODUCTION_ONLY = """\
use tokio::sync::mpsc;

pub struct Service;

impl Service {
    pub async fn drain(&mut self, rx: &mut mpsc::Receiver<u32>) -> u32 {
        rx.recv().await.expect("a request from the actor")
    }
}
"""

# The broken chain, and the line its `.recv()` sits on: the guard reports a site where a reader
# looking for it would look, not where the combinator ended up.
BROKEN = "            .recv()\n            .await\n            .unwrap();"
BROKEN_LINE = SVC[: SVC.index(BROKEN)].count("\n") + 1


def line_of(body: str, needle: str, nth: int = 1) -> int:
    """1-based line holding the nth occurrence of `needle`."""
    at = -1
    for _ in range(nth):
        at = body.index(needle, at + 1)
    return body.count("\n", 0, at) + 1


def make_tree(tmp: Path, files: dict[str, str], crates: tuple[str, ...] = ("widget",)) -> Path:
    """A workspace holding `crates`, with `files` written at their relative paths."""
    for crate in crates:
        manifest = tmp / "crates" / "codegen" / crate / "Cargo.toml"
        manifest.parent.mkdir(parents=True, exist_ok=True)
        manifest.write_text(f'[package]\nname = "{crate}"\nversion = "0.1.0"\n')
    for rel, body in files.items():
        path = tmp / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body, encoding="utf-8")
    return tmp


def run(repo: Path, baseline: Path | None = None, *extra: str) -> tuple[int, str]:
    argv = [sys.executable, str(SCRIPT), "--root", str(repo)]
    argv += ["--baseline", str(baseline)] if baseline is not None else []
    argv += list(extra)
    proc = subprocess.run(argv, capture_output=True, text=True)
    return proc.returncode, proc.stdout + proc.stderr


def fixture_baseline(tmp: Path, rows: str = "TOTAL\t0\n") -> Path:
    path = tmp / "baseline.tsv"
    path.write_text("# fixture baseline\n" + rows, encoding="utf-8")
    return path


class UnboundedRecv(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="unbounded-recv-")
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def test_bounded_waits_pass_and_the_tree_was_read(self) -> None:
        repo = make_tree(self.tmp, {SRC: BOUNDED})
        code, out = run(repo, fixture_baseline(self.tmp))
        self.assertEqual(code, 0, out)
        self.assertIn("0 site(s), every crate at or under its recorded ceiling", out)

    def test_expect_in_a_test_fails_with_file_line_and_phrase(self) -> None:
        repo = make_tree(self.tmp, {SRC: SVC})
        code, out = run(repo, fixture_baseline(self.tmp))
        self.assertEqual(code, 1, out)
        self.assertIn(f"{SRC}:{line_of(SVC, 'an event')}: ", out)
        self.assertIn('rx.recv().await.expect("an event");', out)
        self.assertIn("the test cannot fail, only the CI job can", out)

    def test_unwrap_in_a_test_fails_too(self) -> None:
        repo = make_tree(self.tmp, {SRC: SVC})
        code, out = run(repo, fixture_baseline(self.tmp))
        self.assertEqual(code, 1, out)
        self.assertIn(f"{SRC}:{line_of(SVC, 'rx.recv().await.unwrap()')}: ", out)

    def test_a_chain_broken_across_lines_is_found_at_the_recv_line(self) -> None:
        repo = make_tree(self.tmp, {SRC: SVC})
        code, out = run(repo, fixture_baseline(self.tmp))
        self.assertEqual(code, 1, out)
        self.assertEqual(out.count(f"{SRC}:{BROKEN_LINE}: "), 1, out)

    def test_the_same_call_in_production_is_not_reported(self) -> None:
        repo = make_tree(self.tmp, {SRC: PRODUCTION_ONLY})
        code, out = run(repo, fixture_baseline(self.tmp))
        self.assertEqual(code, 0, out)
        self.assertIn("rx.recv().await.expect(", PRODUCTION_ONLY)

    def test_a_site_under_tests_dir_counts(self) -> None:
        rel = "crates/codegen/widget/tests/it.rs"
        body = (
            "use tokio::sync::mpsc;\n"
            "\n"
            "#[tokio::test]\n"
            "async fn reads() {\n"
            "    let (_tx, mut rx) = mpsc::channel(1);\n"
            "    rx.recv().await.unwrap();\n"
            "}\n"
        )
        repo = make_tree(self.tmp, {rel: body})
        code, out = run(repo, fixture_baseline(self.tmp))
        self.assertEqual(code, 1, out)
        self.assertIn(f"{rel}:{line_of(body, 'rx.recv().await.unwrap()')}: ", out)

    def test_a_site_in_an_included_tests_file_outside_any_cfg_item_counts(self) -> None:
        # `src/backend_tests.rs` and its neighbours are reached by a `#[cfg(test)] mod
        # backend_tests;` declaration, so nothing inside them carries a cfg of their own. A
        # helper at the top of such a file is test code, and reading the `_tests.rs` suffix as a
        # suffix is what finds it: matching it as a whole file name silently dropped 5 of the
        # 232 sites this guard's first version counted.
        rel = "crates/codegen/widget/src/backend_tests.rs"
        body = (
            "use tokio::sync::mpsc;\n"
            "\n"
            "pub async fn admit(rx: &mut mpsc::Receiver<u32>) -> u32 {\n"
            "    rx.recv().await.expect(\"an admitted message\")\n"
            "}\n"
        )
        repo = make_tree(self.tmp, {rel: body})
        code, out = run(repo, fixture_baseline(self.tmp))
        self.assertEqual(code, 1, out)
        self.assertIn(f"{rel}:{line_of(body, 'an admitted message')}: ", out)

    def test_a_module_dropped_by_a_false_cfg_is_not_a_hang_risk(self) -> None:
        # The dropped module sits *inside* `#[cfg(test)] mod tests`, so the test-cfg range covers
        # it on its own and the never-cfg subtraction is the only thing keeping it out. Parking
        # it at the top level instead satisfied the same assertion through two mechanisms at once,
        # and the fixture stayed green with that subtraction deleted.
        body = (
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    #[cfg(any())]\n"
            "    mod parked_tests {\n"
            "        #[tokio::test]\n"
            "        async fn never_compiled() {\n"
            "            let (_tx, mut rx) = mpsc::channel(1);\n"
            "            rx.recv().await.unwrap();\n"
            "        }\n"
            "    }\n"
            "}\n"
        )
        repo = make_tree(self.tmp, {SRC: body})
        code, out = run(repo, fixture_baseline(self.tmp))
        self.assertEqual(code, 0, out)

    def test_a_never_cfg_module_hides_its_own_sites_but_nobody_elses(self) -> None:
        body = (
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    #[cfg(any())]\n"
            "    mod parked_tests {\n"
            "        #[tokio::test]\n"
            "        async fn never_compiled() {\n"
            "            let (_tx, mut rx) = mpsc::channel(1);\n"
            "            rx.recv().await.unwrap();\n"
            "        }\n"
            "    }\n"
            "\n"
            "    #[tokio::test]\n"
            "    async fn live() {\n"
            "        let (_tx, mut rx) = mpsc::channel(1);\n"
            "        rx.recv().await.expect(\"a live event\");\n"
            "    }\n"
            "}\n"
        )
        repo = make_tree(self.tmp, {SRC: body})
        code, out = run(repo, fixture_baseline(self.tmp))
        self.assertEqual(code, 1, out)
        self.assertIn(f"{SRC}:{line_of(body, 'a live event')}: ", out)
        self.assertNotIn(f"{SRC}:{line_of(body, 'rx.recv().await.unwrap()')}: ", out)

    def test_strings_and_comments_are_not_sites(self) -> None:
        body = "\n".join(
            [
                "//! The idiom `rx.recv().await.expect(\"x\")` is what this guard bans.",
                "#[cfg(test)]",
                "mod fixture {",
                "    #[tokio::test]",
                "    async fn note() {",
                "        // rx.recv().await.unwrap();",
                '        let snippet = r#"rx.recv().await.expect("inside a raw string")"#;',
                "        let quote = '\"';",
                "        use_it(quote, snippet);",
                "    }",
                "}",
                "",
            ]
        )
        repo = make_tree(self.tmp, {SRC: body})
        code, out = run(repo, fixture_baseline(self.tmp))
        self.assertEqual(code, 0, out)

        # Same file, one mention turned into live code: proves the blanking above worked rather
        # than the scanner having read nothing.
        live = body.replace(
            "        use_it(quote, snippet);",
            "        use_it(quote, snippet);\n"
            "        let mut rx = tokio::sync::mpsc::channel(1).1;\n"
            "        rx.recv().await.unwrap();",
        )
        repo2 = make_tree(self.tmp / "live", {SRC: live})
        code, out = run(repo2, fixture_baseline(self.tmp / "live"))
        self.assertEqual(code, 1, out)
        self.assertIn(f"{SRC}:{line_of(live, '        rx.recv().await.unwrap();')}: ", out)
        self.assertEqual(out.count("rx.recv().await.unwrap()"), 1, out)

    def test_third_party_and_target_are_not_scanned(self) -> None:
        vendored = (
            "#[tokio::test]\n"
            "async fn upstream_hang() {\n"
            "    rx.recv().await.unwrap();\n"
            "}\n"
        )
        repo = make_tree(
            self.tmp,
            {
                SRC: BOUNDED,
                "third_party/vendored/src/lib.rs": vendored,
                "target/debug/build/widget-0123456789ab/out/generated.rs": vendored,
            },
        )
        code, out = run(repo, fixture_baseline(self.tmp))
        self.assertEqual(code, 0, out)

    def test_ratchet_refuses_a_crate_above_its_row(self) -> None:
        repo = make_tree(self.tmp, {SRC: SVC})
        baseline = fixture_baseline(self.tmp, "widget\t1\nTOTAL\t1\n")
        code, out = run(repo, baseline)
        self.assertEqual(code, 1, out)
        self.assertIn("1 crate(s) above baseline", out)
        self.assertIn("baseline baseline.tsv allows 1", out)
        self.assertEqual(out.count(f"{SRC}:"), 3, out)

    def test_ratchet_refuses_a_crate_below_its_row(self) -> None:
        # A converted site that leaves its ceiling behind is the ratchet going stale, which is
        # how a baseline turns into a number nobody believes. It also proves the scan read the
        # file: a scanner that found nothing would report 0 against 0 and pass.
        repo = make_tree(self.tmp, {SRC: BOUNDED})
        baseline = fixture_baseline(self.tmp, "widget\t3\nTOTAL\t3\n")
        code, out = run(repo, baseline)
        self.assertEqual(code, 1, out)
        self.assertIn("widget (3 -> 0)", out)

    def test_write_baseline_then_the_same_count_back(self) -> None:
        repo = make_tree(self.tmp, {SRC: SVC})
        baseline = self.tmp / "written.tsv"
        code, out = run(repo, baseline, "--write-baseline")
        self.assertEqual(code, 0, out)
        self.assertIn("wrote", out)
        rows = baseline.read_text(encoding="utf-8")
        self.assertIn("widget\t3\n", rows)
        self.assertIn("TOTAL\t3\n", rows)
        code, out = run(repo, baseline)
        self.assertEqual(code, 0, out)
        # And once the sites are converted the same ceiling has to be lowered, not quietly
        # satisfied by a check that only ever looks for an increase.
        make_tree(self.tmp, {SRC: BOUNDED})
        code, out = run(repo, baseline)
        self.assertEqual(code, 1, out)
        self.assertIn("widget (3 -> 0)", out)

    def test_rows_are_per_crate(self) -> None:
        gadget = (
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    #[tokio::test]\n"
            "    async fn reads() {\n"
            "        rx.recv().await.unwrap();\n"
            "    }\n"
            "}\n"
        )
        repo = make_tree(
            self.tmp,
            {SRC: SVC, "crates/codegen/gadget/src/lib.rs": gadget},
            crates=("widget", "gadget"),
        )
        baseline = fixture_baseline(self.tmp, "gadget\t0\nwidget\t2\nTOTAL\t2\n")
        code, out = run(repo, baseline)
        self.assertEqual(code, 1, out)
        self.assertIn("2 crate(s) above baseline", out)
        self.assertIn(f"crates/codegen/gadget/src/lib.rs:{line_of(gadget, 'rx.recv()')}: ", out)

    def test_no_rust_sources_is_an_error_not_a_clean_tree(self) -> None:
        repo = make_tree(self.tmp, {})
        code, out = run(repo, fixture_baseline(self.tmp))
        self.assertEqual(code, 2, out)
        self.assertIn("refusing to report a clean tree", out)

    def test_a_malformed_baseline_row_is_an_error(self) -> None:
        repo = make_tree(self.tmp, {SRC: SVC})
        baseline = fixture_baseline(self.tmp, "widget 3\n")
        code, out = run(repo, baseline)
        self.assertEqual(code, 1, out)
        self.assertIn("malformed row", out)

    def test_verbose_reports_the_emptied_tree(self) -> None:
        repo = make_tree(self.tmp, {SRC: BOUNDED})
        code, out = run(repo, fixture_baseline(self.tmp), "--verbose")
        self.assertEqual(code, 0, out)
        self.assertIn("0 test-side `.recv().await.expect/unwrap` site(s)", out)

    def test_shipped_tree_is_clean(self) -> None:
        code, out = run(REPO)
        self.assertEqual(code, 0, out)


if __name__ == "__main__":
    unittest.main(verbosity=2)
