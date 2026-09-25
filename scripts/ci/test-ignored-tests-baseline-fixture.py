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


class BareIgnoreBaselineTests(unittest.TestCase):
    def test_legacy_entry_is_accepted_and_new_entry_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'Cargo.toml').write_text('[package]\nname = "fixture-crate"\n')
            source = root / 'case.rs'
            source.write_text('#[ignore]\nfn existing() {}\n#[ignore]\nfn added() {}\n')
            rows = module.scan(source)
            baseline = f'fixture-crate\t{source.relative_to(root).as_posix()}\texisting\n'
            self.assertEqual(module.new_bare_ignores(rows[:1], baseline, root), [])
            self.assertEqual(module.new_bare_ignores(rows, baseline, root), [rows[1]])

            baseline_path = root / 'baseline.tsv'
            baseline_path.write_text(baseline, encoding='utf-8')
            result = subprocess.run(
                [sys.executable, str(SCRIPT), '--check-baseline', str(baseline_path), '--root', str(root)],
                capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 1)
            self.assertIn('new bare #[ignore]', result.stderr)

    def test_removed_bare_ignore_is_reported_as_stale_baseline(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'Cargo.toml').write_text('[package]\nname = "fixture-crate"\n')
            source = root / 'case.rs'
            source.write_text('// no ignored test remains\n')
            baseline_path = root / 'baseline.tsv'
            baseline_path.write_text('fixture-crate\tcase.rs\tremoved_case\n', encoding='utf-8')
            result = subprocess.run(
                [sys.executable, str(SCRIPT), '--check-baseline', str(baseline_path), '--root', str(root)],
                capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 1)
            self.assertIn('stale baseline entry', result.stderr)

    def test_reasoned_attributes_are_not_bare_debt(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'Cargo.toml').write_text('[package]\nname = "fixture-crate"\n')
            source = root / 'case.rs'
            source.write_text('#[ignore = "needs PTY; review 2026-10"]\nfn ignored() {}\n')
            rows = module.scan(source)
            self.assertEqual(module.new_bare_ignores(rows, '', root), [])


if __name__ == '__main__':
    unittest.main()
