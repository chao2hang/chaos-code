#!/usr/bin/env python3
"""Fixtures for `ignored-tests.py --require-reasons`.

CI never passes `--ignored`, so an ignored test without a reason is debt nobody
can see. These fixtures inject the violations: a gate that cannot fail on the
bad case is indistinguishable from a repo that happens to be clean.
"""

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name('ignored-tests.py')
spec = importlib.util.spec_from_file_location('ignored_tests', SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def run(root):
    return subprocess.run(
        [sys.executable, str(SCRIPT), '--require-reasons', '--root', str(root)],
        capture_output=True,
        text=True,
    )


class RequireReasonsTests(unittest.TestCase):
    def fixture(self, root, body):
        (root / 'Cargo.toml').write_text('[package]\nname = "fixture-crate"\n')
        (root / 'case.rs').write_text(body)

    def test_bare_ignore_is_rejected_by_name_and_line(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, '#[test]\n#[ignore]\nfn hidden() {}\n')
            result = run(root)
            self.assertEqual(result.returncode, 1)
            self.assertIn('case.rs:2', result.stderr)
            self.assertIn('bare #[ignore]', result.stderr)

    def test_empty_reason_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, '#[test]\n#[ignore = ""]\nfn hidden() {}\n')
            self.assertEqual(run(root).returncode, 1)

    def test_reason_and_trailing_comment_are_accepted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(
                root,
                '#[test]\n#[ignore = "needs a built binary; review 2027-01"] // keep\n'
                'fn hidden() {}\n',
            )
            result = run(root)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('carry a reason', result.stdout)

    def test_unreferenced_source_in_a_comment_is_not_counted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, '/// Example: `#[ignore]` marks a skipped test.\n')
            result = run(root)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_root_reports_zero_and_passes(self):
        with tempfile.TemporaryDirectory() as directory:
            result = run(Path(directory) / 'absent')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('all 0 ignored attributes', result.stdout)

    def test_repo_tree_passes_the_gate(self):
        """The real tree is the shipped path, so assert on it directly."""
        repo = Path(__file__).resolve().parents[2]
        result = subprocess.run(
            [sys.executable, str(SCRIPT), '--require-reasons'],
            cwd=repo,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == '__main__':
    unittest.main()
