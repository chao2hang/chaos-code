#!/usr/bin/env python3
"""Fixtures for `check-guard-wiring.py`.

The reachability rule is the kind of thing that reads obviously correct and then
reports "OK" for the wrong reason, so each case here pins one decision the real
repository does not exercise on its own: comments are not invocations, docs are
not callers, a helper reached through a script CI runs does count, a dangling
call site is an error, and an exemption for a deleted file is an error.

    python3 scripts/ci/test-check-guard-wiring.py
"""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name('check-guard-wiring.py')
REPO = SCRIPT.parents[2]

CI_YML = """\
name: CI
on: push
jobs:
  rust:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: wired guard
        run: python3 scripts/ci/wired.py
      - name: the lab
        run: ./scripts/lab.sh
"""

VERIFY = """\
#!/usr/bin/env bash
gates=(
  "wired: python3 scripts/ci/wired.py"
)
"""


def build(root: Path, *, extra_ci: str = '', helper_in_lab: bool = True) -> Path:
    (root / '.github' / 'workflows').mkdir(parents=True)
    (root / 'scripts' / 'ci').mkdir(parents=True)
    (root / '.github' / 'workflows' / 'ci.yml').write_text(CI_YML + extra_ci, encoding='utf-8')
    (root / 'scripts' / 'verify-in-docker.sh').write_text(VERIFY, encoding='utf-8')
    (root / 'scripts' / 'ci' / 'wired.py').write_text('#!/usr/bin/env python3\n', encoding='utf-8')
    lab = '#!/usr/bin/env bash\n'
    if helper_in_lab:
        lab += 'python3 "$(dirname "$0")/ci/helper.py"\n'
    (root / 'scripts' / 'lab.sh').write_text(lab, encoding='utf-8')
    (root / 'scripts' / 'ci' / 'helper.py').write_text('#!/usr/bin/env python3\n', encoding='utf-8')
    # The checker's own allow-list names ignored-tests.sh, so a tree without it
    # would fail for exemption rot before it could say anything about the case
    # under test. The exemption-rot case below deletes this file again.
    (root / 'scripts' / 'ci' / 'ignored-tests.sh').write_text('#!/usr/bin/env bash\n', encoding='utf-8')
    return root


def run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(SCRIPT), *[str(a) for a in args]], capture_output=True, text=True)


class RealRepositoryTests(unittest.TestCase):
    def test_every_gate_in_the_real_repo_is_reachable(self):
        result = run('--root', REPO)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('check-guard-wiring: OK', result.stdout)


class SyntheticTreeTests(unittest.TestCase):
    def test_an_unwired_gate_is_named(self):
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            (root / 'scripts' / 'ci' / 'orphan.py').write_text('#!/usr/bin/env python3\n', encoding='utf-8')
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('orphan.py is not invoked', result.stdout)

    def test_a_gate_reached_through_a_script_ci_runs_counts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            result = run('--root', root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_a_comment_mention_is_not_an_invocation(self):
        # The failure this prevents: `install.sh`'s header comment names a test
        # file. If prose counted, the check would pass on documentation alone.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory), helper_in_lab=False)
            ci = root / '.github' / 'workflows' / 'ci.yml'
            ci.write_text(
                ci.read_text(encoding='utf-8') + '      # see scripts/ci/helper.py for the details\n',
                encoding='utf-8',
            )
            lab = root / 'scripts' / 'lab.sh'
            lab.write_text('#!/usr/bin/env bash\n# helper.py used to run here\n', encoding='utf-8')
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('helper.py is not invoked', result.stdout)

    def test_a_docstring_mention_is_not_an_invocation(self):
        # The case the checker failed its first real run on: its own docstring
        # explains four gates by name, so reading docstrings as code let the file
        # certify itself green out of its own commentary.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory), helper_in_lab=False)
            (root / 'scripts' / 'ci' / 'wired.py').write_text(
                '#!/usr/bin/env python3\n"""Explain helper.py, the other half of this check."""\n',
                encoding='utf-8',
            )
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('helper.py is not invoked', result.stdout)

    def test_a_documentation_mention_is_not_an_invocation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory), helper_in_lab=False)
            docs = root / 'docs'
            docs.mkdir()
            (docs / 'audit.md').write_text('Run `python3 scripts/ci/helper.py`.\n', encoding='utf-8')
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('helper.py is not invoked', result.stdout)

    def test_a_dangling_call_site_is_named(self):
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            ci = root / '.github' / 'workflows' / 'ci.yml'
            ci.write_text(
                ci.read_text(encoding='utf-8') + '      - name: gone\n        run: python3 scripts/ci/removed.py\n',
                encoding='utf-8',
            )
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('scripts/ci/removed.py, which does not exist', result.stdout)

    def test_an_exemption_for_a_deleted_file_is_an_error(self):
        # EXEMPT names ignored-tests.sh; once that alias is deleted the entry has
        # to go with it, or the allow-list silently keeps exempting nothing.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            (root / 'scripts' / 'ci' / 'ignored-tests.sh').unlink()
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('ignored-tests.sh no longer exists', result.stdout)

    def test_a_real_gate_wired_only_in_the_docker_entry_point_counts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            gate = root / 'scripts' / 'ci' / 'docker-only.py'
            gate.write_text('#!/usr/bin/env python3\n', encoding='utf-8')
            verify = root / 'scripts' / 'verify-in-docker.sh'
            verify.write_text(
                verify.read_text(encoding='utf-8').replace(
                    'gates=(', 'gates=(\n  "docker only: python3 scripts/ci/docker-only.py"'
                ),
                encoding='utf-8',
            )
            self.assertIn('docker-only.py', verify.read_text(encoding='utf-8'))
            self.assertEqual(run('--root', root).returncode, 0)


if __name__ == '__main__':
    unittest.main(verbosity=2)
