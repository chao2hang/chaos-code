import csv
import importlib.util
import io
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name('ignored-tests.py')
spec = importlib.util.spec_from_file_location('ignored_tests', SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class IgnoredTestsParserTests(unittest.TestCase):
    def scan(self, source):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'Cargo.toml').write_text('[package]\nname = "fixture-crate"\n')
            path = root / 'test.rs'
            path.write_text(source)
            return module.scan(path)

    def test_trailing_comment_is_not_reason(self):
        rows = self.scan('#[ignore] // reason-like text\n#[test]\nfn skipped() {}\n')
        self.assertEqual(rows[0][3], 'NO_REASON')
        self.assertEqual(rows[0][2], 1)

    def test_multiline_reason_and_escaped_quote(self):
        rows = self.scan('#[ignore = "first line\\n\\\"quoted\\\""\n]\n#[test]\nfn skipped() {}\n')
        self.assertEqual(rows[0][3], 'first line\n"quoted"')

    def test_csv_round_trip_quotes_commas_and_newlines(self):
        stream = io.StringIO(newline='')
        csv.writer(stream, lineterminator='\n').writerow(['crate', 'path,with,commas', 1, 'quote " and\nnewline', 'fn'])
        parsed = next(csv.reader(io.StringIO(stream.getvalue(), newline='')))
        self.assertEqual(parsed, ['crate', 'path,with,commas', '1', 'quote " and\nnewline', 'fn'])

    def test_ignores_textual_examples_in_doc_comments(self):
        self.assertEqual(self.scan('/// Example: `#[ignore]` is disabled.\n#[test]\nfn active() {}\n'), [])

    def test_empty_tree_emits_header_only(self):
        with tempfile.TemporaryDirectory() as directory:
            result = __import__('subprocess').run(['python3', str(SCRIPT), '--csv', '--root', directory], capture_output=True, text=True, check=True)
            self.assertEqual(list(csv.reader(result.stdout.splitlines())), [['crate', 'file', 'line', 'reason', 'nearest_fn']])

    def test_baseline_accepts_existing_bare_ignores_and_rejects_new(self):
        rows = self.scan('#[ignore]\n#[test]\nfn old_case() {}\n')
        baseline = 'fixture-crate\ttest.rs\told_case\n'
        root = Path(rows[0][1]).parent
        self.assertEqual(module.new_bare_ignores(rows, baseline, root), [])
        self.assertEqual(module.new_bare_ignores(rows, '', root), rows)

    def test_baseline_uses_multiset_for_duplicate_function_keys(self):
        rows = self.scan('#[ignore]\nfn duplicate() {}\n#[ignore]\nfn duplicate() {}\n')
        baseline = 'fixture-crate\ttest.rs\tduplicate\n'
        root = Path(rows[0][1]).parent
        self.assertEqual(module.new_bare_ignores(rows, baseline, root), [rows[1]])

    def test_reasoned_ignore_does_not_need_baseline_entry(self):
        rows = self.scan('#[ignore = "needs network; review 2026-10"]\nfn reasoned() {}\n')
        self.assertEqual(module.new_bare_ignores(rows, ''), [])


if __name__ == '__main__':
    unittest.main()
