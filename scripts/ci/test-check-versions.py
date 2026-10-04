#!/usr/bin/env python3
"""Fixtures for `scripts/ci/check-versions.sh`.

The gate keeps the Cargo package version and the npm package versions from drifting
apart, and check 5 of it compares the `optionalDependencies` set in `npm/chaos/package.json`
against the platform package directories on disk, so a platform added on disk but never
declared (or a pin dropped) fails the build with both lists printed.

On 2026-10-04 it was found unable to report the one breakage it is most specific about.
With `optionalDependencies` deleted from that file the gate exited 1, left stdout at its
first informational line and wrote nothing to stderr. The cause was not the comparison:
`declared_names` was built by `grep -v '^$'` inside a bare `$(...)` assignment, and `grep`
exits 1 when no line is left to print, which under `set -euo pipefail` ends the script one
statement before the comparison that would have named the missing set. Check 4 upstream of
it had already accepted everything silently, since an empty declared set means the loop
never runs. So the gate died holding the only report of the breakage. The fix is
`sed '/^$/d'`, which has no "no lines matched" status.

Every failing case here therefore asserts on the *report*, not on the exit code. The exit
code was already 1 while the gate was broken; only the words were missing.

    python3 scripts/ci/test-check-versions.py
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
GATE = HERE / "check-versions.sh"
REL_MAIN_PKG = "crates/codegen/xai-grok-pager/npm/chaos/package.json"
REL_CARGO_TOML = "crates/codegen/xai-grok-pager-bin/Cargo.toml"
REL_NPM = "crates/codegen/xai-grok-pager/npm"
CARGO_SRC = REPO / REL_CARGO_TOML
NPM_SRC = REPO / REL_NPM


def read_json(tree: Path, rel: str) -> dict:
    return json.loads((tree / rel).read_text(encoding="utf-8"))


def write_json(tree: Path, rel: str, data: dict) -> None:
    (tree / rel).write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def declared_names(tree: Path, rel: str = REL_MAIN_PKG) -> list:
    deps = read_json(tree, rel).get("optionalDependencies") or {}
    return sorted(deps)


@unittest.skipUnless(shutil.which("node"), "check-versions.sh parses package.json with node")
@unittest.skipIf(os.name == "nt", "the gate under test is a bash script")
class CheckVersionsReportTests(unittest.TestCase):
    """Each mutation is applied to a copy of the tree; the repository is never touched."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tree = Path(self._tmp.name) / "repo"
        (self.tree / "scripts/ci").mkdir(parents=True)
        shutil.copy(GATE, self.tree / "scripts/ci/check-versions.sh")
        (self.tree / "crates/codegen/xai-grok-pager-bin").mkdir(parents=True)
        shutil.copy(CARGO_SRC, self.tree / REL_CARGO_TOML)
        shutil.copytree(NPM_SRC, self.tree / REL_NPM)

    def run_gate(self) -> subprocess.CompletedProcess:
        return subprocess.run(
            ["bash", "scripts/ci/check-versions.sh"],
            cwd=self.tree,
            capture_output=True,
            text=True,
        )

    def assert_reports(self, proc: subprocess.CompletedProcess, *fragments: str) -> None:
        """Failed, and said why in stderr: exit 1 in silence is the defect under test."""
        self.assertNotEqual(proc.returncode, 0, f"expected a failure\nstdout: {proc.stdout}")
        self.assertTrue(
            proc.stderr.strip(),
            f"gate failed without a report\nstdout: {proc.stdout}\nstderr: {proc.stderr}",
        )
        for fragment in fragments:
            self.assertIn(fragment, proc.stderr)

    def test_faithful_copy_passes(self):
        # Guards the fixture itself: were the copied tree not a faithful one, every
        # "reports the breakage" assertion below could be satisfied by an artifact of setup.
        proc = self.run_gate()
        self.assertEqual(proc.returncode, 0, f"stdout: {proc.stdout}\nstderr: {proc.stderr}")
        self.assertIn("agree on", proc.stdout)

    def test_optional_dependencies_removed_reports_the_package_set(self):
        # The 2026-10-04 defect. Before the fix: exit 1, empty stderr, no mention of
        # either list. The declared side is empty, so the report has to say so in words.
        main = read_json(self.tree, REL_MAIN_PKG)
        del main["optionalDependencies"]
        write_json(self.tree, REL_MAIN_PKG, main)
        proc = self.run_gate()
        self.assert_reports(proc, "MISMATCH platform package set", "(none declared)")
        for name in ("chaos-code-darwin-arm64", "chaos-code-linux-x64"):
            self.assertIn(name, proc.stderr)

    def test_optional_dependencies_empty_object_reports_too(self):
        # `{}` reaches the same code path by a different edit, and `release.yml` reads
        # this field to stamp the platform packages, so an emptied map ships nothing.
        main = read_json(self.tree, REL_MAIN_PKG)
        main["optionalDependencies"] = {}
        write_json(self.tree, REL_MAIN_PKG, main)
        self.assert_reports(self.run_gate(), "MISMATCH platform package set", "(none declared)")

    def test_one_dropped_pin_names_the_package(self):
        # Non-empty on both sides: this path never had the bug, and if it stopped
        # reporting, the two cases above would be proving a fix that bought nothing.
        names = declared_names(self.tree)
        dropped = names[0]
        main = read_json(self.tree, REL_MAIN_PKG)
        del main["optionalDependencies"][dropped]
        write_json(self.tree, REL_MAIN_PKG, main)
        proc = self.run_gate()
        self.assert_reports(proc, "MISMATCH platform package set", dropped)
        self.assertNotIn(dropped, proc.stderr.split("package.json names on disk:")[0])

    def test_platform_package_version_drift_is_named(self):
        # The gate's headline purpose, kept asserted so the pipefail work cannot be the
        # only thing this file pins.
        platform_pkg = sorted(
            str(p.relative_to(self.tree))
            for p in (self.tree / REL_NPM).glob("chaos-*/package.json")
        )[0]
        data = read_json(self.tree, platform_pkg)
        data["version"] = "0.0.0-drifted"
        write_json(self.tree, platform_pkg, data)
        self.assert_reports(self.run_gate(), "MISMATCH", platform_pkg, "0.0.0-drifted")

    def test_cargo_version_drift_is_named(self):
        self.drift_cargo_version("9.9.9-drifted")
        proc = self.run_gate()
        self.assert_reports(proc, "MISMATCH", "9.9.9-drifted")

    def drift_cargo_version(self, new: str) -> None:
        # The same section-scoped read the gate performs, so the drift lands on the
        # package's own version and not on a `[dependencies]` entry.
        cargo = self.tree / REL_CARGO_TOML
        lines = cargo.read_text(encoding="utf-8").splitlines(keepends=True)
        in_package = False
        for index, line in enumerate(lines):
            if line.startswith("["):
                in_package = line.strip() == "[package]"
            elif in_package and line.startswith("version"):
                lines[index] = f'version = "{new}"\n'
                cargo.write_text("".join(lines), encoding="utf-8")
                return
        self.fail("fixture could not find the [package] version to drift")


if __name__ == "__main__":
    unittest.main(verbosity=2)
