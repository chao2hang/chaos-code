import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name('classify-open-todos.py')


class OpenTodoClassificationTests(unittest.TestCase):
    def test_emits_each_state_with_section_and_exact_totals(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'TODO.md').write_text(
                '# Scope A\n- [ ] needs work\n- [~] partial work\n## Scope B\n- [x] done\n- [ ] another open\n',
                encoding='utf-8',
            )
            checker = root / 'scripts/ci/classify-open-todos.py'
            checker.parent.mkdir(parents=True)
            checker.write_text(SCRIPT.read_text(encoding='utf-8').replace(
                "ROOT = Path(__file__).resolve().parents[2]", f"ROOT = Path(r'{root.as_posix()}')"
            ), encoding='utf-8')
            result = subprocess.run([sys.executable, str(checker)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('6\tunchecked\t## Scope B\t- [ ] another open', result.stdout)
            self.assertIn('TOTAL\tunchecked=2\tpartial=1\trows=3', result.stdout)

    def test_classification_document_counts_match_real_todo(self):
        result = subprocess.run([sys.executable, str(SCRIPT)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)

        groups = {
            name: {'unchecked': 0, 'partial': 0}
            for name in ('M-1', 'M0', 'M1', 'M2', 'M3', 'M4', 'M5', 'Maintenance items / §8')
        }
        for row in result.stdout.splitlines():
            columns = row.split('\t', 3)
            if len(columns) < 4 or not columns[0].isdigit():
                continue
            status, section = columns[1], columns[2]
            if section.startswith('### M-1.'):
                group = 'M-1'
            elif section.startswith('### M0.'):
                group = 'M0'
            elif section.startswith('### M1.'):
                group = 'M1'
            elif section.startswith('### M2.'):
                group = 'M2'
            elif section.startswith('### M3.') or section.startswith('## M3.3'):
                group = 'M3'
            elif section.startswith('### M4.'):
                group = 'M4'
            elif section.startswith('### M5.'):
                group = 'M5'
            elif section.startswith('## MT-') or section.startswith('### 8.'):
                group = 'Maintenance items / §8'
            else:
                self.fail(f'Open TODO row is not assigned to a classification group: {section}')
            groups[group][status] += 1

        classification_path = SCRIPT.parents[2] / 'docs/architecture/todo-open-item-classification.md'
        classification = classification_path.read_text(encoding='utf-8')
        documented = {}
        for line in classification.splitlines():
            if not line.startswith('| '):
                continue
            columns = [column.strip() for column in line.strip('|').split('|')]
            if len(columns) == 3 and columns[0] in groups:
                documented[columns[0]] = {'unchecked': int(columns[1]), 'partial': int(columns[2])}

        self.assertEqual(documented, groups)
        self.assertEqual(
            sum(counts['unchecked'] + counts['partial'] for counts in documented.values()),
            sum(1 for line in result.stdout.splitlines() if line.split('\t', 1)[0].isdigit()),
        )


if __name__ == '__main__':
    unittest.main()
