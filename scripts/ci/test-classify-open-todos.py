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


if __name__ == '__main__':
    unittest.main()
