#!/usr/bin/env python3
"""Fixtures for `check-load-bearing-features.py`.

The gate exists because of one real defect: `xai-grok-tools` needed
`serde_json/preserve_order`, never declared it, and received it through a
dependency that only exists under `cfg(unix)`. The MCP elicitation schema-order
test was therefore green on Linux and red on the Windows CI leg. No `cfg` in the
code changed, no compiler error appeared, and the difference lived entirely in
the resolved dependency graph.

Each fixture therefore pins a way this gate could have stayed silent while that
defect was present:

- a tree parser that reads no feature nodes: pinned from both sides, a dump that
  must yield the feature and the same dump with the feature line removed, which
  must yield nothing;
- a tree parser that reads nothing because `cargo tree` colourised its output, which
  is what happened on the first CI run of this gate -- the `rust check / clippy /
  test` job exports `CARGO_TERM_COLOR=always`, and a coloured tree has no matchable
  label while its glyph-free root line still matches, so the dependency read as
  present and every feature as lost. Pinned three ways: a real coloured capture
  parsed with and without the feature, the command asserted to pass `--color never`,
  and the end-to-end run against the real graph forced through that same variable so
  a developer shell that happens to leave colour off cannot hide the regression;
- a tree that counts dev-dependency edges, where a test-only dependency hands the
  feature to `cargo test` while `cargo build` still lacks it: pinned on the argv,
  because the shipped binary is what the table's rows are about;
- a `check` that treats an uninstalled target as "nothing to compare": pinned by
  requiring the failure message to name the missing target and to say
  `rustup target add`;
- a `check` that treats a missing dependency as an empty feature set: pinned
  separately from the missing-feature case, because "the crate stopped depending
  on it" is a different finding from "the feature was lost";
- a `check` that reports the first problem and returns: pinned with two failing
  rows in one table;
- a table that could be emptied or reduced to one target: a one-target row is
  rejected outright, since a comparison over a single target cannot fail.

Two fixtures run against the real repository. One parses the committed table; the
other runs the real `cargo tree` for every target the table names, which is what
proves the shipped tree satisfies it rather than merely being parseable.

    python3 scripts/ci/test-check-load-bearing-features.py
"""

import importlib.util
import io
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).with_name("check-load-bearing-features.py")
REPO = SCRIPT.parents[2]
_spec = importlib.util.spec_from_file_location("check_load_bearing_features", SCRIPT)
guard = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(guard)

TABLE = "scripts/ci/load-bearing-features.tsv"
LINUX = "x86_64-unknown-linux-gnu"
WINDOWS = "x86_64-pc-windows-msvc"
MACOS = "aarch64-apple-darwin"
ALL_TARGETS = [LINUX, WINDOWS, MACOS]

# Shape of `cargo tree -p xai-grok-tools -e features --locked --target T -i
# serde_json` as of cargo 1.94: the dependency is the root, its enabled features
# are the depth-1 nodes, and the packages that asked for each feature hang below.
TREE_WITH_FEATURE = """serde_json v1.0.149
├── serde_json feature "alloc"
│   └── schemars v1.0.4
│       └── schemars feature "default"
│           └── xai-grok-tools v0.1.220-alpha.4 (/repo/crates/codegen/xai-grok-tools)
├── serde_json feature "preserve_order"
│   ├── serde_json feature "indexmap"
│   │   └── serde_json feature "preserve_order" (*)
│   └── xai-grok-tools v0.1.220-alpha.4 (/repo/crates/codegen/xai-grok-tools)
├── serde_json feature "raw_value"
│   └── axum v0.8.6 (*)
└── serde_json feature "std"
    └── serde_json feature "default" (*)
"""

TREE_WITHOUT_FEATURE = TREE_WITH_FEATURE.replace(
    """├── serde_json feature "preserve_order"
│   ├── serde_json feature "indexmap"
│   │   └── serde_json feature "preserve_order" (*)
│   └── xai-grok-tools v0.1.220-alpha.4 (/repo/crates/codegen/xai-grok-tools)
""",
    "",
)

TREE_WITHOUT_DEPENDENCY = """error: package `xai-grok-tools` does not contain serde_json
"""

# The four shapes `cargo tree` uses for its glyphs when colour is on, plus the
# repeat marker, captured from
# `CARGO_TERM_COLOR=always cargo tree -p xai-grok-tools -e features --locked
#  --target x86_64-unknown-linux-gnu -i serde_json`.
# That is the environment of the `rust check / clippy / test` job, which sets
# `CARGO_TERM_COLOR=always` for the whole job. Note that the root line carries no
# glyphs and so survives a parser that cannot see past an escape sequence: the
# first CI run of this gate therefore reported the dependency as present while
# finding none of its features, which read as a real lost-feature failure.
BRANCH = "\x1b[2m\x1b[35m├──\x1b[0m "
TRUNK = "\x1b[2m\x1b[35m│\x1b[0m   "
LEAF = "\x1b[2m\x1b[35m└──\x1b[0m "
PAD = "\x1b[2m \x1b[0m   "
REPEAT = " \x1b[33m\x1b[2m(*)\x1b[39m\x1b[22m"
TREE_WITH_COLOUR = (
    "serde_json v1.0.149\n"
    f'{BRANCH}serde_json feature "alloc"\n'
    f"{TRUNK}{LEAF}schemars v1.0.4\n"
    f"{TRUNK}{PAD}{TRUNK}{LEAF}serde_json feature \"indexmap\"\n"
    f'{TRUNK}{PAD}{LEAF}serde_json feature "preserve_order"\n'
    f'{BRANCH}serde_json feature "preserve_order"{REPEAT}\n'
    f'{TRUNK}{PAD}{BRANCH}serde_json feature "raw_value"{REPEAT}\n'
)


def flag_value(argv: list[str], name: str) -> str | None:
    """The value following `name` in an argv, or None when the flag was not passed."""
    return argv[argv.index(name) + 1] if name in argv else None


def row(feature="preserve_order", targets=None, crate="xai-grok-tools", dep="serde_json"):
    return (crate, dep, feature, list(targets or ALL_TARGETS), "why not")


def table_text(*rows: str) -> str:
    header = "# comment\n\n"
    return header + "\n".join(rows)


def good_row_line(targets=",".join(ALL_TARGETS), feature="preserve_order") -> str:
    return "\t".join(
        ["xai-grok-tools", "serde_json", feature, targets, "the form field order"]
    )


class TreeParsing(unittest.TestCase):
    def test_feature_nodes_of_the_named_dependency_are_read(self):
        features, present = guard.parse_tree(TREE_WITH_FEATURE, "serde_json")
        self.assertTrue(present)
        self.assertEqual(
            features, {"alloc", "preserve_order", "raw_value", "std", "indexmap", "default"}
        )

    def test_the_same_dump_without_the_feature_line_yields_nothing(self):
        # Without this half, the passing case above could be satisfied by a parser
        # that never finds any feature node at all.
        features, present = guard.parse_tree(TREE_WITHOUT_FEATURE, "serde_json")
        self.assertTrue(present, "the dependency is still in the dump")
        self.assertNotIn("preserve_order", features)
        self.assertIn("raw_value", features, "the rest of the dump still parses")

    def test_another_packages_features_are_not_attributed_to_the_dependency(self):
        dump = TREE_WITH_FEATURE.replace(
            'serde_json feature "preserve_order"', 'schemars feature "preserve_order"'
        )
        features, present = guard.parse_tree(dump, "serde_json")
        self.assertTrue(present)
        self.assertNotIn("preserve_order", features)

    def test_a_dependency_that_is_not_in_the_tree_is_reported_as_absent(self):
        features, present = guard.parse_tree(TREE_WITHOUT_DEPENDENCY, "serde_json")
        self.assertEqual(features, set())
        self.assertFalse(present)

    def test_repeated_and_marked_subtrees_are_still_counted(self):
        # `(*)` marks a subtree printed earlier; the feature itself is enabled, so
        # dropping these lines would under-report.
        dump = 'serde_json v1.0.149\n├── serde_json feature "preserve_order" (*)\n'
        features, present = guard.parse_tree(dump, "serde_json")
        self.assertTrue(present)
        self.assertEqual(features, {"preserve_order"})

    def test_a_coloured_capture_yields_the_same_features_as_a_plain_one(self):
        # A coloured tree is the same data with different bytes. The first CI run of
        # this gate could not see past the escapes and called a fully-featured
        # dependency featureless.
        features, present = guard.parse_tree(TREE_WITH_COLOUR, "serde_json")
        self.assertTrue(present)
        self.assertEqual(features, {"alloc", "indexmap", "preserve_order", "raw_value"})

    def test_the_coloured_capture_still_says_nothing_about_features_it_lacks(self):
        # The non-vacuity half of the fixture above: colour must not turn the parser
        # into something that reports a feature for every line it cannot read.
        dump = "\n".join(
            line
            for line in TREE_WITH_COLOUR.splitlines()
            if "preserve_order" not in line
        )
        features, present = guard.parse_tree(dump, "serde_json")
        self.assertTrue(present)
        self.assertNotIn("preserve_order", features)
        self.assertIn("raw_value", features, "the rest of the coloured dump still parses")


class CommandShape(unittest.TestCase):
    """The gate reads `cargo tree` as a data source, so its output flags are load-bearing."""

    def _argv(self) -> list[str]:
        captured: dict[str, list[str]] = {}

        def fake_run(argv, **_kwargs):
            argv = list(argv)
            captured["argv"] = argv
            return subprocess.CompletedProcess(argv, 0, stdout="serde_json v1.0.149\n", stderr="")

        with mock.patch.object(guard.subprocess, "run", fake_run):
            features, present = guard.enabled_features(
                "xai-grok-tools", "serde_json", LINUX, REPO
            )
        self.assertTrue(present, "the fixture dump should report the dependency")
        self.assertEqual(features, set())
        return captured["argv"]

    def test_the_tree_is_requested_without_colour(self):
        argv = self._argv()
        self.assertEqual(flag_value(argv, "--color"), "never")

    def test_dev_dependency_edges_are_excluded(self):
        # A dev-dependency can supply the feature to `cargo test` while the shipped
        # binary still lacks it; every row in the table is about runtime behaviour.
        argv = self._argv()
        self.assertEqual(flag_value(argv, "--edges"), "no-dev")

    def test_the_command_still_asks_about_the_rows_own_crate_target_and_dependency(self):
        # Guards against the flags above being "fixed" by dropping the question.
        argv = self._argv()
        self.assertEqual(flag_value(argv, "-p"), "xai-grok-tools")
        self.assertEqual(flag_value(argv, "--target"), LINUX)
        self.assertEqual(flag_value(argv, "-i"), "serde_json")
        self.assertIn("--locked", argv)


class CheckRows(unittest.TestCase):
    def run_check(self, rows, enabled, have=None):
        have = set(have if have is not None else ALL_TARGETS)

        def features_for(crate, dep, target, repo):
            if target not in have:  # pragma: no cover - guarded by check()
                raise AssertionError("check() must not query an unavailable target")
            value = enabled.get(target)
            if value is None:
                return set(), False
            return set(value), True

        return guard.check(Path("/repo"), rows, features_for=features_for,
                           available_targets=lambda: have)

    def test_a_feature_enabled_on_every_target_produces_no_problem(self):
        problems, count = self.run_check([row()], {t: ["preserve_order"] for t in ALL_TARGETS})
        self.assertEqual((problems, count), ([], 1))

    def test_the_failure_names_the_target_that_lost_the_feature_only(self):
        enabled = {t: ["preserve_order"] for t in ALL_TARGETS}
        enabled[WINDOWS] = ["std"]
        problems, _ = self.run_check([row()], enabled)
        self.assertEqual(len(problems), 1)
        self.assertIn(f"without `preserve_order` on {WINDOWS} ", problems[0])
        self.assertIn(
            f"(enabled on: {LINUX}, {MACOS})", problems[0]
        ), "the passing targets are listed so the reader can see the shape"
        self.assertIn("row reason: why not", problems[0], "the table's why has to be printed")

    def test_an_uninstalled_target_fails_rather_than_skipping(self):
        problems, _ = self.run_check([row()], {t: ["preserve_order"] for t in ALL_TARGETS},
                                     have=[LINUX, WINDOWS])
        self.assertEqual(len(problems), 1)
        self.assertIn(MACOS, problems[0])
        self.assertIn(f"rustup target add {MACOS}", problems[0])

    def test_a_dependency_that_vanished_for_one_target_is_reported_as_such(self):
        enabled = {t: ["preserve_order"] for t in ALL_TARGETS}
        del enabled[MACOS]
        problems, _ = self.run_check([row()], enabled)
        self.assertEqual(len(problems), 1)
        self.assertIn("does not depend on", problems[0])
        self.assertIn(MACOS, problems[0])
        # The same target must not also be accused of building the dependency
        # without the feature, which would be false.
        self.assertNotIn("builds `serde_json` without", " ".join(problems))

    def test_a_feature_no_target_enables_is_called_stale(self):
        problems, _ = self.run_check([row()], {t: ["std"] for t in ALL_TARGETS})
        self.assertEqual(len(problems), 1)
        self.assertIn("stale", problems[0])
        # A row nobody satisfies must not also be reported as a per-target skew.
        self.assertNotIn("enabled on:", problems[0])

    def test_every_failing_row_is_reported_not_only_the_first(self):
        rows = [row(feature="preserve_order"), row(feature="raw_value", dep="serde_json")]
        enabled = {t: ["unrelated"] for t in ALL_TARGETS}
        problems, count = self.run_check(rows, enabled)
        self.assertEqual(count, 2)
        self.assertEqual(len(problems), 2, "a loop that returns early hides rows")

    def test_a_cargo_tree_failure_propagates_rather_than_reading_as_green(self):
        failed = subprocess.CompletedProcess([], 101, "", "error: no such package")
        with mock.patch.object(guard.subprocess, "run", return_value=failed):
            with self.assertRaises(guard.ConfigError) as ctx:
                guard.enabled_features("xai-grok-tools", "serde_json", LINUX, Path("/repo"))
        self.assertIn("no such package", str(ctx.exception))


class TableParsing(unittest.TestCase):
    def test_comments_and_blank_lines_are_not_rows(self):
        rows = guard.parse_config(table_text(good_row_line()))
        self.assertEqual(len(rows), 1)
        crate, dep, feature, targets, why = rows[0]
        self.assertEqual((crate, dep, feature), ("xai-grok-tools", "serde_json",
                                                 "preserve_order"))
        self.assertEqual(targets, ALL_TARGETS)
        self.assertEqual(why, "the form field order")

    def test_a_row_naming_one_target_is_rejected(self):
        # A comparison over one target can never fail, so such a row is fake
        # coverage rather than weak coverage.
        with self.assertRaises(guard.ConfigError) as ctx:
            guard.parse_config(table_text(good_row_line(targets=LINUX)))
        self.assertIn("proves nothing", str(ctx.exception))

    def test_a_repeated_target_is_rejected(self):
        with self.assertRaises(guard.ConfigError):
            guard.parse_config(table_text(good_row_line(targets=f"{LINUX},{LINUX}")))

    def test_a_row_with_the_wrong_field_count_is_rejected(self):
        for bad in ["xai-grok-tools\tserde_json\tpreserve_order",
                    "\t".join(["", "serde_json", "f", LINUX, WINDOWS, "why"]),
                    "not tab separated at all"]:
            with self.subTest(bad=bad), self.assertRaises(guard.ConfigError):
                guard.parse_config(bad)

    def test_an_empty_table_is_rejected(self):
        with self.assertRaises(guard.ConfigError):
            guard.parse_config("# nothing but a comment\n\n")

    def test_the_line_number_is_reported_so_a_100_row_table_is_locatable(self):
        text = table_text(good_row_line(), "too\tfew", good_row_line())
        with self.assertRaises(guard.ConfigError) as ctx:
            guard.parse_config(text)
        self.assertIn("line 4", str(ctx.exception))


class CommittedArtifacts(unittest.TestCase):
    def test_the_committed_table_is_well_formed_and_cites_real_crates(self):
        rows = guard.parse_config((REPO / TABLE).read_text())
        self.assertGreaterEqual(len(rows), 1)
        for crate, dep, feature, targets, _why in rows:
            manifest = REPO / "crates" / "codegen" / crate / "Cargo.toml"
            self.assertTrue(manifest.is_file(), f"{crate} is not a crate in this repo")
            for target in targets:
                # A rust target triple: arch, vendor, sysname and ABI, so 3 or 4
                # hyphen-separated fields (`x86_64-unknown-linux-gnu`,
                # `aarch64-apple-darwin`).
                self.assertRegex(target, r"^[a-z0-9_]+(-[a-z0-9_]+){2,}$")

    def test_the_committed_table_pins_the_feature_that_broke_on_windows(self):
        rows = guard.parse_config((REPO / TABLE).read_text())
        matching = [r for r in rows if r[0] == "xai-grok-tools" and r[1] == "serde_json"
                    and r[2] == "preserve_order"]
        self.assertEqual(len(matching), 1, "the defect that motivated this gate must stay pinned")
        self.assertEqual(sorted(matching[0][3]), sorted(ALL_TARGETS))
        self.assertIn("BTreeMap", matching[0][4])

    def test_the_shipped_tree_satisfies_the_committed_table(self):
        # End to end against the real graph. This is the fixture that fails if the
        # Cargo.toml declaration is ever removed, which is the exact regression the
        # gate was written for.
        if not _cargo_and_targets_present():
            self.skipTest("cargo or one of the table's targets is unavailable here")
        # Run under the CI job's environment, not the developer's shell. Without the
        # forced colour this test passed locally while the same call returned nothing
        # parseable on the runner.
        with mock.patch.dict(os.environ, {"CARGO_TERM_COLOR": "always"}):
            problems, count = guard.check(REPO, guard.parse_config((REPO / TABLE).read_text()))
        self.assertEqual(problems, [], f"{count} row(s) checked")


class MainExitCodes(unittest.TestCase):
    def test_a_missing_table_is_exit_two(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(guard.main(["--repo", tmp]), 2)

    def test_a_malformed_table_is_exit_two(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "scripts/ci").mkdir(parents=True)
            (root / TABLE).write_text("one\ttwo\n")
            self.assertEqual(guard.main(["--repo", tmp]), 2)

    def test_an_uninstalled_target_reaches_exit_one(self):
        # Runs the real `main`, including the real `rustup target list`, so the
        # fail-closed path is proven through the entry point rather than around it.
        triple = f"{'z' * 7}-never-built-msvc"
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "scripts/ci").mkdir(parents=True)
            (root / TABLE).write_text(table_text(good_row_line(
                targets=f"{LINUX},{triple}")))
            self.assertEqual(guard.main(["--repo", tmp]), 1)

    def test_exit_zero_path_prints_what_was_checked(self):
        rows = guard.parse_config(table_text(good_row_line()))
        enabled = {t: ["preserve_order"] for t in ALL_TARGETS}
        with mock.patch.object(guard, "check",
                               return_value=([], len(rows))) as checked, \
                mock.patch.object(sys, "stdout", new_callable=io.StringIO) as out:
            code = guard.main(["--repo", str(REPO)])
        checked.assert_called_once()
        self.assertEqual(code, 0)
        self.assertIn("1 load-bearing feature row", out.getvalue())


def _cargo_and_targets_present() -> bool:
    try:
        have = guard.installed_targets()
    except guard.ConfigError:
        return False
    return subprocess.run(["which", "cargo"], capture_output=True).returncode == 0 and all(
        t in have for t in ALL_TARGETS
    )


if __name__ == "__main__":
    unittest.main(verbosity=2)
