"""Negative/positive fixtures for scripts/ci/check-script-portability.py.

The gate is only worth having if a violation still makes it fail, so each
non-portable rule family is injected into a throwaway tree and asserted to exit
1. The clean-tree case is asserted too, otherwise a scanner that stopped
matching would look identical to a repository that got fixed.
"""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name('check-script-portability.py')


def run(root: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(SCRIPT), '--root', str(root)],
        capture_output=True,
        text=True,
    )


class ScriptPortabilityTests(unittest.TestCase):
    def tree(self, root: Path, body: str) -> None:
        scripts = root / 'scripts'
        scripts.mkdir(parents=True, exist_ok=True)
        (scripts / 'case.sh').write_text(
            f'#!/usr/bin/env bash\nset -euo pipefail\n{body}\n', encoding='utf-8'
        )

    def test_clean_script_passes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.tree(root, 'while IFS= read -r line; do files+=("$line"); done < <(git ls-files)\n'
                            'echo "cpus $(getconf _NPROCESSORS_ONLN)"\n'
                            # the portable BSD in-place form must stay allowed
                            "sed -i '' 's/a/b/' file")
            result = run(root)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('OK', result.stdout)

    def test_every_rule_family_is_detected(self):
        cases = {
            'bash4-mapfile': 'mapfile -t files < <(git ls-files)',
            'bash4-readarray': 'readarray -t files',
            'bash4-assoc-array': 'declare -A seen',
            'bash4-nameref': 'declare -n ref=some_var',
            'bash4-case-fold': 'key="CHAOS_${platform^^}"',
            'bash4-case-fold-lower': 'key="${platform,,}"',
            'bash4-epoch': 'start=$EPOCHSECONDS',
            'bash4-globstar': 'shopt -s globstar',
            'bash4-wait-n': 'wait -n',
            'gnu-nproc': 'jobs="$(nproc)"',
            'gnu-readlink-f': 'path="$(readlink -f "$file")"',
            'gnu-realpath': 'path="$(realpath "$file")"',
            'gnu-date-d': 'date -d "2 hours ago" +%s',
            'gnu-stat-c': 'stat -c %s "$file"',
            'gnu-sed-inplace': 'sed -i "s/a/b/" "$file"',
            'gnu-grep-pcre': 'grep -P "x" "$file"',
            'gnu-grep-include': 'grep -R --include=*.rs needle .',
            'gnu-find-printf': 'find . -name "*.rs" -printf "%p\\n"',
            'gnu-xargs-run': 'printf "%s\\n" "$@" | xargs -r true',
            'gnu-install-d': 'install -D build/bin out/bin',
            'gnu-cp-reflink': 'cp --reflink=auto a b',
            'gnu-base64-wrap': 'base64 -w0 file',
            'gnu-timeout': 'timeout 30 cargo build',
            'gnu-tac': 'tac file',
            'gnu-wc-L': 'wc -L file',
        }
        for expected, body in cases.items():
            with self.subTest(rule=expected), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.tree(root, body)
                result = run(root)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                if expected == 'bash4-case-fold-lower':
                    self.assertIn('bash4-case-fold', result.stderr)
                else:
                    self.assertIn(expected, result.stderr)

    def test_comment_mentioning_a_rule_is_not_a_finding(self):
        # scripts/ci/*.sh carry comments explaining why a construct was removed;
        # those must not fail the gate, or the fix gets muted instead of kept.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.tree(root, '# `mapfile` is bash 4; macOS still ships 3.2, nproc is GNU only.\ntrue')
            result = run(root)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_ignore_pragma_suppresses_one_line_only(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.tree(root, 'mapfile -t ok  # portability-check:ignore\nmapfile -t bad')
            result = run(root)
            self.assertEqual(result.returncode, 1)
            self.assertIn('mapfile -t bad', result.stderr)
            self.assertNotIn('mapfile -t ok', result.stderr)

    def test_empty_tree_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'scripts').mkdir()
            result = run(root)
            self.assertEqual(result.returncode, 1)
            self.assertIn('no shell scripts', result.stderr)


if __name__ == '__main__':
    unittest.main()
