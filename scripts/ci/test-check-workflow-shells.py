#!/usr/bin/env python3
"""Fixtures for `check-workflow-shells.py`.

The gate decides one thing: whether a step could execute under `pwsh`. The cases below are the
ways a step can be arranged so that the answer is not what the author meant, plus the two ways
the gate itself can go blind -- a workflow file it was never told to read, and a matrix written
in the form it does not recognise. Both of those were real: `DEFAULT_WORKFLOWS` named two files
while the repository grew a third, and only the inline `os: [a, b]` form of a matrix was read, so
rewriting the same matrix as a block would have taken the job out of the check's sight.

    python3 scripts/ci/test-check-workflow-shells.py
"""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).with_name('check-workflow-shells.py')
REPO = HERE.parents[2]
WORKFLOWS = REPO / '.github' / 'workflows'

STEP = """\
      - name: stamp the build
        run: date +%Y%m%d
"""
PINNED = STEP.replace('        run:', '        shell: bash\n        run:')
GUARDED = STEP.replace('        run:', "        if: runner.os == 'Linux'\n        run:")
SH = STEP.replace('        run:', '        shell: sh\n        run:')


def workflow(steps: str, runs_on: str, matrix: str = '') -> str:
    return (
        'name: CI\n'
        'on: push\n'
        'jobs:\n'
        '  build:\n'
        f'    runs-on: {runs_on}\n'
        + (f'    strategy:\n      matrix:\n{matrix}\n' if matrix else '')
        + '    steps:\n'
        + '      - uses: actions/checkout@v4\n'
        + steps
    )


WINDOWS_INLINE = '        os: [macos-14, windows-latest]'
WINDOWS_BLOCK = '        os:\n          - macos-14\n          - windows-latest'
LINUX_BLOCK = '        os:\n          - macos-14\n          - ubuntu-latest'


class ShellsTests(unittest.TestCase):
    def setUp(self) -> None:
        self._dir = tempfile.TemporaryDirectory(prefix='workflow-shells-')
        self.root = Path(self._dir.name)
        self.addCleanup(self._dir.cleanup)

    def run_gate(self, *files: str,
                 cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run([sys.executable, str(HERE), *files], cwd=cwd or self.root,
                              capture_output=True, text=True)

    def write(self, text: str, name: str = 'ci.yml',
              directory: Path | None = None) -> str:
        target = directory or self.root
        target.mkdir(parents=True, exist_ok=True)
        (target / name).write_text(text, encoding='utf-8')
        return name

    def assert_flags(self, proc: subprocess.CompletedProcess[str], needle: str) -> None:
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn(needle, proc.stderr)

    # ---- the rule itself --------------------------------------------------

    def test_an_unpinned_run_on_a_windows_runner_is_refused(self) -> None:
        proc = self.run_gate(self.write(workflow(STEP, 'windows-latest')))
        self.assert_flags(proc, 'would execute under pwsh')
        # The report has to name where, not just that something is wrong.
        self.assertIn('ci.yml:8', proc.stderr)
        self.assertIn("job 'build'", proc.stderr)
        self.assertIn("'stamp the build'", proc.stderr)

    def test_pinning_bash_satisfies_the_rule(self) -> None:
        proc = self.run_gate(self.write(workflow(PINNED, 'windows-latest')))
        self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_an_if_naming_runner_os_satisfies_the_rule(self) -> None:
        proc = self.run_gate(self.write(workflow(GUARDED, 'windows-latest')))
        self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_sh_is_not_bash(self) -> None:
        # `shell: sh` would run the step, but under a shell that is not the one every
        # other step in these files assumes; accepting it is how the rule erodes.
        proc = self.run_gate(self.write(workflow(SH, 'windows-latest')))
        self.assert_flags(proc, 'pwsh')

    def test_a_linux_only_job_is_left_alone(self) -> None:
        proc = self.run_gate(self.write(workflow(STEP, 'ubuntu-latest')))
        self.assertEqual(proc.returncode, 0, proc.stderr)

    # ---- which jobs are Windows-capable -----------------------------------

    def test_an_inline_matrix_naming_windows_makes_the_job_capable(self) -> None:
        proc = self.run_gate(self.write(
            workflow(STEP, '${{ matrix.os }}', WINDOWS_INLINE)))
        self.assert_flags(proc, 'pwsh')

    def test_a_block_matrix_naming_windows_makes_the_job_capable(self) -> None:
        # The same matrix written as a block. Reading only the inline form meant a
        # reformat -- no change in what runs -- moved the job out of the check.
        proc = self.run_gate(self.write(
            workflow(STEP, '${{ matrix.os }}', WINDOWS_BLOCK)))
        self.assert_flags(proc, 'pwsh')

    def test_a_block_matrix_without_windows_is_left_alone(self) -> None:
        proc = self.run_gate(self.write(
            workflow(STEP, '${{ matrix.os }}', LINUX_BLOCK)))
        self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_a_commented_out_step_is_not_a_step(self) -> None:
        commented = ''.join('# ' + line for line in STEP.splitlines(keepends=True))
        proc = self.run_gate(self.write(workflow(commented, 'windows-latest')))
        self.assertEqual(proc.returncode, 0, proc.stderr)

    # ---- which files get read ---------------------------------------------

    def test_a_third_workflow_is_checked_without_anyone_naming_it(self) -> None:
        directory = self.root / '.github' / 'workflows'
        self.write(workflow(PINNED, 'ubuntu-latest'), 'ci.yml', directory)
        self.write(workflow(STEP, 'windows-latest'), 'docker-labs.yml', directory)
        proc = self.run_gate()
        self.assert_flags(proc, 'docker-labs.yml:8')

    def test_no_workflows_at_all_refuses_to_pass(self) -> None:
        (self.root / '.github' / 'workflows').mkdir(parents=True)
        proc = self.run_gate()
        self.assert_flags(proc, 'no workflow files')

    def test_a_named_missing_workflow_is_an_error(self) -> None:
        proc = self.run_gate('nope.yml')
        self.assert_flags(proc, 'missing workflow')

    # ---- the shipped repository -------------------------------------------

    def test_every_shipped_workflow_pins_its_shells(self) -> None:
        # Run from the real checkout with no arguments, which is what the gate sees in
        # CI: the point is that discovery, not a name in this file, brings each workflow
        # under the rule.
        shipped = sorted(p.name for p in WORKFLOWS.glob('*.yml'))
        self.assertGreaterEqual(len(shipped), 3, 'discovery found nothing to check')
        proc = self.run_gate(cwd=REPO)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        for name in shipped:
            self.assertIn(f'.github/workflows/{name}: OK', proc.stdout)


if __name__ == '__main__':
    unittest.main(verbosity=2)
