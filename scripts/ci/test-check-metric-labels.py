#!/usr/bin/env python3
"""Fixtures for `check-metric-labels.py`.

Every case drives the shipped script over a synthetic tree and pins one decision the gate
makes, from both sides where a missing match would otherwise look like a clean tree. The pair
for "a wrong label count is an abort" is therefore not only the call with one value too many
but the same metric called correctly, so a scanner that stopped matching `with_label_values`
altogether cannot pass.

Decisions pinned here, in the order the cases appear:

- the dependency really does unwrap: the version the gate cites is the one the lockfile pins,
  and against the vendored `prometheus` source in `~/.cargo`, when it is readable, the three
  line numbers the gate quotes are the `pub fn with_label_values`, the `unwrap()` that panics
  and the length check. The premise of the whole gate is checked rather than quoted, and a
  citation that drifts away from the pinned version fails here rather than in a docstring.
- arity is counted from entries, not commas, so the one-entry-per-line trailing comma that
  most of this repository's label arrays use is not read as an extra value;
- a `register_histogram_vec!` bucket list is not a label list;
- `Opts::new(name, help)` puts the metric name one argument deeper, and the repository's own
  `register_resource!` is not a Prometheus registration at all;
- names, labels and duplicates are checked on the registration side, and a name built at
  runtime is reported rather than guessed;
- only `register_*!` against the default registry is claimed to be globally unique;
- a hand-built `IntCounterVec::new(Opts::new(..), &[..])` is checked for arity like any other;
- label values carried in a local `let labels = [..]` are counted, and a slice that arrives
  as a parameter is reported as `dynamic-labels` rather than assumed fine;
- an identifier used for two metrics with different label counts is reported as ambiguous
  instead of resolved by guessing the type;
- the four ways to write an exemption are validated, and an exemption whose finding is gone
  fails as stale;
- one case runs the gate on this repository, so it cannot be green in fixtures and red on the
  tree it was written for.

    python3 scripts/ci/test-check-metric-labels.py
"""

import importlib.util
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-metric-labels.py")
REPO = SCRIPT.parents[2]
ALLOWLIST = Path(__file__).with_name("metric-labels-allowlist.tsv")
_spec = importlib.util.spec_from_file_location("check_metric_labels", SCRIPT)
guard = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(guard)

GOOD = """
use prometheus::{register_int_counter_vec, LazyLock, IntCounterVec};

static CALLS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "app_calls_total",
        "Calls by outcome and route",
        &["outcome", "route"]
    )
    .unwrap()
});

fn hit(outcome: &str) {
    CALLS_TOTAL.with_label_values(&[outcome, "/x"]).inc();
}
"""


def run(root: Path, allowlist: Path | None = None) -> tuple[int, str]:
    cmd = [sys.executable, str(SCRIPT), "--root", str(root)]
    if allowlist is not None:
        cmd += ["--allowlist", str(allowlist)]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    return proc.returncode, proc.stdout + proc.stderr


class Tree(unittest.TestCase):
    """Base: a temp directory holding one Rust file plus an empty allowlist."""

    def tree(self, source: str, allowlist_rows: str = "") -> Path:
        root = Path(tempfile.mkdtemp(prefix="metric-labels-fixture-"))
        self.addCleanup(shutil.rmtree, root, True)
        (root / "src").mkdir()
        (root / "src" / "lib.rs").write_text(source, encoding="utf-8")
        ledger = root / "allow.tsv"
        ledger.write_text(allowlist_rows, encoding="utf-8")
        self.allowlist = ledger
        return root

    def assert_green(self, source: str, allowlist_rows: str = "") -> str:
        root = self.tree(source, allowlist_rows)
        rc, out = run(root, self.allowlist)
        self.assertEqual(rc, 0, out)
        return out

    def assert_red(self, source: str, needle: str, allowlist_rows: str = "") -> str:
        root = self.tree(source, allowlist_rows)
        rc, out = run(root, self.allowlist)
        self.assertNotEqual(rc, 0, out)
        self.assertIn(needle, out)
        return out


class Premise(Tree):
    def test_the_lockfile_pins_the_version_the_docstring_quotes(self) -> None:
        """The gate cites `prometheus-0.14.0`; the lockfile has to still say that."""
        doc = (SCRIPT.read_text(encoding="utf-8").split('"""')[1])
        cited = re.findall(r"prometheus-(\d+\.\d+\.\d+)", doc)
        self.assertEqual(len(cited), 1, doc[:400])
        lock = (REPO / "Cargo.lock").read_text(encoding="utf-8")
        pinned = re.findall(
            r'name = "prometheus"\nversion = "([^"]+)"', lock)
        self.assertEqual(pinned, [cited[0]],
                         f"docstring cites prometheus {cited}, lockfile pins {pinned}")

    def test_the_dependency_unwraps_the_lookup(self) -> None:
        """The gate exists because `with_label_values` aborts; check that in the source."""
        candidates = sorted(
            Path.home().glob(".cargo/registry/src/*/prometheus-*/src/vec.rs")
        )
        if not candidates:
            self.skipTest("the prometheus source is not vendored on this host")
        path = candidates[0]
        text = path.read_text(encoding="utf-8")
        self.assertIn("pub fn with_label_values", text)
        self.assertRegex(text, r"fn with_label_values[\s\S]{0,200}?\.unwrap\(\)")
        self.assertIn("InconsistentCardinality", text)
        if path.parent.parent.name != "prometheus-0.14.0":
            self.skipTest(f"cited line numbers are for 0.14.0, found {path.parent.parent.name}")
        lines = text.splitlines()
        # The docstring names these three lines; a citation that drifted is a bug in the doc.
        self.assertTrue(lines[291].lstrip().startswith("pub fn with_label_values"),
                        lines[291])
        self.assertEqual(lines[295].strip(),
                         "self.get_metric_with_label_values(vals).unwrap()")
        self.assertEqual(lines[117].strip(),
                         "if vals.len() != self.desc.variable_labels.len() {")

    def test_this_repository_holds(self) -> None:
        """The gate cannot be green in fixtures and red on the tree it was written for."""
        rc, out = run(REPO, ALLOWLIST)
        self.assertEqual(rc, 0, out[-4000:])
        self.assertIn("metric labels hold", out)

    def test_print_lists_every_metric(self) -> None:
        proc = subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(REPO),
             "--allowlist", str(ALLOWLIST), "--print"],
            capture_output=True, text=True,
        )
        self.assertEqual(proc.returncode, 0, proc.stderr[-2000:])
        self.assertRegex(proc.stdout, r"\d+ metric registrations, \d+ label-value call sites")
        self.assertIn("grok_workspace_startup_stage_duration_seconds", proc.stdout)
        # The registered label list is printed, which is what makes --print a review aid.
        self.assertIn("[stage,outcome]", proc.stdout)


class Arity(Tree):
    def test_correct_call_is_silent(self) -> None:
        self.assertIn("metric labels hold", self.assert_green(GOOD))

    def test_one_value_too_many_is_reported(self) -> None:
        self.assert_red(GOOD.replace('&[outcome, "/x"]', '&[outcome, "/x", "extra"]'),
                        "arity: CALLS_TOTAL.with_label_values passes 3")

    def test_one_value_too_few_is_reported(self) -> None:
        self.assert_red(GOOD.replace('&[outcome, "/x"]', "&[outcome]"),
                        "passes 1 label value(s), the metric declares 2")

    def test_the_report_says_what_the_mismatch_does(self) -> None:
        out = self.assert_red(GOOD.replace('&[outcome, "/x"]', "&[outcome]"),
                             "aborts on a mismatch")
        self.assertIn("prometheus unwraps inside the dependency", out)

    def test_trailing_comma_is_not_an_element(self) -> None:
        """One entry per line is ordinary rustfmt output, not a third label."""
        source = GOOD.replace('&[outcome, "/x"]', '&[\n        outcome,\n        "/x",\n    ]')
        self.assertIn("metric labels hold", self.assert_green(source))

    def test_non_unwrapping_lookup_is_still_checked(self) -> None:
        """`get_metric_with_label_values` returns the error, so a mismatch is a metric that
        never reports rather than an abort; both are reported."""
        source = GOOD.replace("with_label_values", "get_metric_with_label_values")
        out = self.assert_red(source.replace('&[outcome, "/x"]', "&[outcome]"),
                              "returns Err on a mismatch")
        self.assertIn("silently stops reporting", out)

    def test_commented_out_call_is_not_live(self) -> None:
        source = GOOD + "\n// fn gone() { CALLS_TOTAL.with_label_values(&[\"a\"]).inc(); }\n"
        self.assertIn("metric labels hold", self.assert_green(source))

    def test_a_string_that_looks_like_a_call_is_not_one(self) -> None:
        """A doc comment quoting the wrong call is prose; the gate must not read it as code."""
        source = GOOD.replace(
            "fn hit(outcome: &str) {",
            '/// Use `CALLS_TOTAL.with_label_values(&["a"])` sparingly.\n'
            "fn hit(outcome: &str) {",
        )
        self.assertIn("metric labels hold", self.assert_green(source))


class Registration(Tree):
    def test_bucket_list_is_not_a_label_list(self) -> None:
        source = """
use prometheus::{register_histogram_vec, LazyLock, HistogramVec};
static SECS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!("app_secs_seconds", "Seconds", &["route"], vec![0.1, 1.0, 10.0])
        .unwrap()
});
fn observe() { SECS.with_label_values(&["/x"]).observe(0.2); }
"""
        self.assertIn("metric labels hold", self.assert_green(source))

    def test_opts_form_is_parsed(self) -> None:
        source = """
use prometheus::{register_int_counter_vec, LazyLock, IntCounterVec, Opts};
static V: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(Opts::new("app_v_total", "help"), &["reason"]).unwrap()
});
fn hit() { V.with_label_values(&["timeout"]).inc(); }
"""
        self.assertIn("metric labels hold", self.assert_green(source))
        self.assert_red(source.replace('&["timeout"]', '&["timeout", "extra"]'),
                        "arity: V.with_label_values passes 2")

    def test_repo_macros_are_not_prometheus_registrations(self) -> None:
        """This repository has its own `register_resource!`, 30 call sites, 23 of which name
        the same tool family: reading it as a metric would invent 22 duplicate names."""
        source = """
register_resource!("grok_build", ReadFile);
register_resource!("grok_build", ListDir);
"""
        self.assertIn("metric labels hold", self.assert_green(source))

    def test_duplicate_name_is_reported(self) -> None:
        source = """
use prometheus::{register_int_counter, LazyLock, IntCounter};
static A: LazyLock<IntCounter> =
    LazyLock::new(|| register_int_counter!("app_shared_total", "help").unwrap());
static B: LazyLock<IntCounter> =
    LazyLock::new(|| register_int_counter!("app_shared_total", "other").unwrap());
"""
        self.assert_red(source, "duplicate-name: 'app_shared_total' is registered 2 times")

    def test_hand_built_family_is_not_claimed_globally_unique(self) -> None:
        """A test's own `Registry::new()` cannot collide with the process registry."""
        source = """
use prometheus::{register_int_counter, LazyLock, IntCounter, IntCounterVec, Opts, Registry};
static A: LazyLock<IntCounter> =
    LazyLock::new(|| register_int_counter!("grok_test_total", "help").unwrap());
fn in_a_test() {
    let registry = Registry::new();
    let counter = IntCounterVec::new(Opts::new("grok_test_total", "help"), &["reason"]).unwrap();
    registry.register(Box::new(counter.clone())).unwrap();
    counter.with_label_values(&["zdr"]).inc_by(5);
}
"""
        self.assertIn("metric labels hold", self.assert_green(source))

    def test_hand_built_family_still_has_its_arity_checked(self) -> None:
        source = """
use prometheus::{IntCounterVec, Opts};
fn in_a_test() {
    let counter = IntCounterVec::new(Opts::new("grok_test_total", "help"), &["reason"]).unwrap();
    counter.with_label_values(&["a", "b"]).inc_by(5);
}
"""
        self.assert_red(source, "arity: counter.with_label_values passes 2")

    def test_bad_metric_name(self) -> None:
        source = """
use prometheus::{register_int_counter, LazyLock, IntCounter};
static A: LazyLock<IntCounter> =
    LazyLock::new(|| register_int_counter!("app-calls-total", "help").unwrap());
"""
        self.assert_red(source, "bad-name: 'app-calls-total'")

    def test_runtime_metric_name_is_reported_not_guessed(self) -> None:
        source = """
use prometheus::{register_int_counter, LazyLock, IntCounter};
static A: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(format!("app_{}_total", component()), "help").unwrap()
});
"""
        self.assert_red(source, "unreadable-name")

    def test_bad_label_name_and_duplicate_label(self) -> None:
        source = """
use prometheus::{register_int_counter_vec, LazyLock, IntCounterVec};
static A: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!("app_a_total", "help", &["in flight", "route", "route"]).unwrap()
});
"""
        out = self.assert_red(source, "bad-label: app_a_total: label 'in flight'")
        self.assertIn("duplicate-label: app_a_total: label 'route' is declared twice, "
                      "at positions 1 and 2", out)

    def test_a_parenthesis_in_a_help_string_does_not_close_the_arguments(self) -> None:
        """Help text is prose. An unbalanced bracket inside it must not end the argument list,
        which would silently drop the label list and turn every call into a mismatch."""
        source = """
use prometheus::{register_int_counter_vec, LazyLock, IntCounterVec};
static A: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!("app_a_total", "counts (only the first half", &["x"]).unwrap()
});
fn hit() { A.with_label_values(&["x"]).inc(); }
"""
        self.assertIn("metric labels hold", self.assert_green(source))
        self.assert_red(source.replace('A.with_label_values(&["x"])',
                                       'A.with_label_values(&["x", "y"])'),
                        "arity: A.with_label_values passes 2")

    def test_zero_label_vec(self) -> None:
        source = """
use prometheus::{register_int_counter_vec, LazyLock, IntCounterVec};
static A: LazyLock<IntCounterVec> =
    LazyLock::new(|| register_int_counter_vec!("app_a_total", "help", &[]).unwrap());
fn hit() { A.with_label_values(&["unexpected"]).inc(); }
"""
        self.assert_red(source, "passes 1 label value(s), the metric declares 0")


class Resolution(Tree):
    def test_labels_in_a_local_array_are_counted(self) -> None:
        source = GOOD.replace(
            'fn hit(outcome: &str) {\n    CALLS_TOTAL.with_label_values(&[outcome, "/x"]).inc();',
            'fn hit(outcome: &str) {\n'
            '    let labels = [outcome, "/x"];\n'
            "    CALLS_TOTAL.with_label_values(&labels).inc();",
        )
        self.assertIn("metric labels hold", self.assert_green(source))
        short = source.replace("let labels = [outcome, \"/x\"];", 'let labels = [outcome];')
        self.assert_red(short, "arity: CALLS_TOTAL.with_label_values passes 1")

    def test_slice_parameter_is_reported_as_dynamic(self) -> None:
        source = GOOD.replace(
            'fn hit(outcome: &str) {\n    CALLS_TOTAL.with_label_values(&[outcome, "/x"]).inc();',
            'fn hit(labels: &[&str]) {\n    CALLS_TOTAL.with_label_values(labels).inc();',
        )
        self.assert_red(source, "dynamic-labels: CALLS_TOTAL")
        # Recorded with a reason, it is silent.
        row = ("CALLS_TOTAL\tdynamic-labels\tthe label slice is built by the caller, so no "
               "static count exists at this call site\n")
        self.assertIn("metric labels hold", self.assert_green(source, row))

    def test_dynamic_row_does_not_excuse_an_arity_finding(self) -> None:
        """The ledger excuses a call the gate cannot count, never a counted mismatch."""
        source = GOOD.replace('&[outcome, "/x"]', "&[outcome]")
        self.assert_red(source, "arity:",
                        allowlist_rows="CALLS_TOTAL\tdynamic-labels\t"
                                       "placeholder reason long enough to satisfy the gate\n")

    def test_unresolved_receiver_must_be_recorded(self) -> None:
        source = GOOD + "\nfn other() {\n    helper.with_label_values(&[\"a\"]).inc();\n}\n"
        self.assert_red(source, "unresolved-receiver: helper")
        self.assertIn("metric labels hold", self.assert_green(
            source, "helper\tunresolved-receiver\tsame method name on a non-prometheus type "
                    "defined outside this crate\n"))

    def test_shared_identifier_is_not_resolved_by_guessing(self) -> None:
        source = """
use prometheus::{register_int_counter_vec, LazyLock, IntCounterVec};
pub mod a {
    use super::*;
    pub static V: LazyLock<IntCounterVec> =
        LazyLock::new(|| register_int_counter_vec!("app_a_total", "h", &["x"]).unwrap());
}
pub mod b {
    use super::*;
    pub static V: LazyLock<IntCounterVec> =
        LazyLock::new(|| register_int_counter_vec!("app_b_total", "h", &["x", "y"]).unwrap());
}
fn hit() { V.with_label_values(&["x", "y"]).inc(); }
"""
        self.assert_red(source, "ambiguous-receiver: V")

    def test_stale_row_fails(self) -> None:
        """A reviewed exemption cannot outlive the call that needed it."""
        root = self.tree(GOOD, "GONE_TOTAL\tdynamic-labels\tthis call was rewritten and the "
                               "row was never deleted\n")
        rc, out = run(root, self.allowlist)
        self.assertEqual(rc, 1, out)
        self.assertIn("stale", out)


class Ledger(Tree):
    def bad_row(self, row: str, needle: str) -> None:
        root = self.tree(GOOD, row)
        rc, out = run(root, self.allowlist)
        self.assertEqual(rc, 2, out)
        self.assertIn(needle, out)

    def test_unknown_category(self) -> None:
        self.bad_row("X\twhatever\treason that is long enough to pass the length check\n",
                     "unknown category")

    def test_short_reason(self) -> None:
        self.bad_row("X\tdynamic-labels\ttoo short\n", "write at least 20")

    def test_wrong_column_count(self) -> None:
        self.bad_row("X\tdynamic-labels\n", "expected 3 tab-separated columns")

    def test_comments_and_blank_lines_are_not_rows(self) -> None:
        root = self.tree(GOOD, "# review note\n\n")
        rc, out = run(root, self.allowlist)
        self.assertEqual(rc, 0, out)


if __name__ == "__main__":
    unittest.main(verbosity=2)
