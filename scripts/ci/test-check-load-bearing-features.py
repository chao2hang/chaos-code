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
