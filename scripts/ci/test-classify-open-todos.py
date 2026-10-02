#!/usr/bin/env python3
"""Fixtures for `classify-open-todos.py`.

The document that script checks,
`docs/architecture/todo-open-item-classification.md`, sat 13 rows off -- 149 claimed
against 136 real, with its `M4` row reading 24/3 while the prose in the same file said
5/13 -- and nothing failed, because the comparison was only ever run by hand. These cases
make it provable: the real document has to pass, a one-number edit of it has to fail and
name the group, and a TODO row under a heading no milestone group owns has to be refused
rather than quietly left out of every count.

    python3 scripts/ci/test-classify-open-todos.py
"""

import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name('classify-open-todos.py')
REPO = SCRIPT.parents[2]
DOC = REPO / 'docs' / 'architecture' / 'todo-open-item-classification.md'

FIXTURE_TODO = """# Fixture

### M2.1 something
- [ ] two-box row
- [~] half done row
- [x] finished row, must not be counted

### M5.9 release
- [ ] packaging

## MT-1 npm packages
- [~] a maintenance row

### 8.1 housekeeping
- [ ] another maintenance row
"""

GROUP_LINES = (
    'M-1\t0\t0',
    'M0\t0\t0',
    'M1\t0\t0',
    'M2\t1\t1',
    'M3\t0\t0',
    'M4\t0\t0',
    'M5\t1\t0',
    'Maintenance items / §8\t1\t1',
)


def run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ['python3', str(SCRIPT), *[str(arg) for arg in args]], capture_output=True, text=True
    )


def write(path: Path, text: str) -> Path:
    path.write_text(text, encoding='utf-8')
    return path


def real_groups() -> dict[str, tuple[int, int]]:
    """`(unchecked, partial)` per group straight out of TODO.md, via the subject script.

    The document-side and the TODO-side numbers are independent facts; a case that
    derived one from the other would keep passing while the guard compared them.
    """
    result = run('--groups')
    if result.returncode != 0:
        raise AssertionError(result.stdout + result.stderr)
    groups = {}
    for line in result.stdout.splitlines():
        group, unchecked, partial = line.split('\t')
        groups[group] = (int(unchecked), int(partial))
    return groups


class InventoryTests(unittest.TestCase):
    def test_lists_every_open_row_with_line_section_and_totals(self):
        with tempfile.TemporaryDirectory() as directory:
            todo = write(Path(directory) / 'TODO.md', FIXTURE_TODO)
            result = run('--todo', todo)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('4\tunchecked\t### M2.1 something\t- [ ] two-box row', result.stdout)
            self.assertIn('12\tpartial\t## MT-1 npm packages\t- [~] a maintenance row', result.stdout)
            self.assertNotIn('finished row', result.stdout, '- [x] rows are not open work')
            self.assertEqual(result.stdout.splitlines()[-1], 'TOTAL\tunchecked=3\tpartial=2\trows=5')

    def test_groups_map_headings_to_milestone_buckets(self):
        with tempfile.TemporaryDirectory() as directory:
            todo = write(Path(directory) / 'TODO.md', FIXTURE_TODO)
            result = run('--todo', todo, '--groups')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.splitlines(), list(GROUP_LINES))

    def test_heading_no_group_owns_is_refused_not_dropped(self):
        # The failure this prevents: a new milestone section whose rows are counted by
        # TODO.md's own `TOTAL` but by nobody's group, so the document looks complete.
        with tempfile.TemporaryDirectory() as directory:
            todo = write(
                Path(directory) / 'TODO.md',
                '### M2.1 something\n- [ ] counted\n\n### M6.1 unowned milestone\n- [ ] invisible\n',
            )
            for mode in ([], ['--groups']):
                result = run('--todo', todo, *mode)
                self.assertNotEqual(result.returncode, 0, f'mode {mode} accepted an unowned row')
                self.assertIn('### M6.1 unowned milestone', result.stderr + result.stdout)
                self.assertIn('line 5', result.stderr + result.stdout)


class DocumentGuardTests(unittest.TestCase):
    def test_the_real_document_matches_the_real_todo(self):
        result = run('--check-doc', DOC)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('matches TODO.md', result.stdout)

    def test_one_wrong_number_fails_and_names_the_group(self):
        # The numbers are read out of the document and out of TODO.md rather than
        # hard-coded, so this case keeps proving something after a legitimate recount.
        # Transposing one row's two columns is the shape of the real bug: a well-formed,
        # plausible, wrong table.
        text = DOC.read_text(encoding='utf-8')
        real_line = next(line for line in text.splitlines() if line.startswith('| M4 |'))
        doc_unchecked, doc_partial = (int(cell) for cell in real_line.strip('|').split('|')[1:3])
        todo_unchecked, todo_partial = real_groups()['M4']
        stale_line = f'| M4 | {doc_partial} | {doc_unchecked} |'
        self.assertNotEqual(stale_line, real_line, 'a symmetric row cannot prove anything')
        with tempfile.TemporaryDirectory() as directory:
            doc = write(Path(directory) / 'classification.md', text.replace(real_line, stale_line, 1))
            result = run('--check-doc', doc)
        self.assertNotEqual(result.returncode, 0)
        out = result.stdout + result.stderr
        self.assertIn('does not match TODO.md', out)
        self.assertIn(f'M4 unchecked: document says {doc_partial}, TODO.md has {todo_unchecked}', out)
        self.assertIn('recompute with', out)

    def test_a_missing_group_row_is_refused(self):
        text = DOC.read_text(encoding='utf-8')
        lines = [line for line in text.splitlines() if not line.startswith('| M0 |')]
        self.assertLess(len(lines), len(text.splitlines()), 'no M0 row to remove')
        with tempfile.TemporaryDirectory() as directory:
            doc = write(Path(directory) / 'classification.md', '\n'.join(lines))
            result = run('--check-doc', doc)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('no row for M0', result.stdout + result.stderr)

    def test_a_duplicated_group_row_is_refused(self):
        text = DOC.read_text(encoding='utf-8')
        real_line = next(line for line in text.splitlines() if line.startswith('| M1 |'))
        edited = text.replace(real_line, f'{real_line}\n{real_line}', 1)
        self.assertNotEqual(edited, text)
        with tempfile.TemporaryDirectory() as directory:
            doc = write(Path(directory) / 'classification.md', edited)
            result = run('--check-doc', doc)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('appears twice', result.stdout + result.stderr)

    def test_a_non_numeric_count_is_refused(self):
        text = DOC.read_text(encoding='utf-8')
        real_line = next(line for line in text.splitlines() if line.startswith('| M3 |'))
        edited = text.replace(real_line, '| M3 | six | 11 |', 1)
        self.assertNotEqual(edited, text)
        with tempfile.TemporaryDirectory() as directory:
            doc = write(Path(directory) / 'classification.md', edited)
            result = run('--check-doc', doc)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('non-numeric', result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2)
