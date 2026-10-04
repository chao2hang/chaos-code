#!/usr/bin/env python3
"""Fixtures for `check-workflow-toolchain.py`.

The rule is short enough to state in one breath -- a job that runs a tool has to
install it first -- which is exactly why it needs fixtures: every way of getting
it wrong reads as a simplification. Reading a step's comments as its script makes
a job that merely *mentions* the guard look like it runs it; ignoring step order
makes a job that installs ripgrep after the guard look fixed; matching `npm` as a
substring makes `grep -q 'npm' package.json` look like a Node program.

The first case is the one that matters most: it runs the checker over this
repository's own workflows, and it is red for a day whenever a job gains a step
whose binary nobody installed -- the shape that made `docs-l10n` fail on every
push on 2026-10-02 while every local gate stayed green.

    python3 scripts/ci/test-check-workflow-toolchain.py
"""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name('check-workflow-toolchain.py')
REPO = SCRIPT.parents[2]

GUARD_STEP = """\
      - name: guard self-tests
        run: |
          set -euo pipefail
          python3 scripts/l10n-guard-selftest.py
          bash scripts/l10n-guard.sh --before HEAD --after HEAD --report "$RUNNER_TEMP/l10n"
"""

APT_RG = """\
      - name: Install ripgrep
        run: |
          sudo apt-get update
          sudo apt-get install -y --no-install-recommends ripgrep
"""

TARBALL_RG = """\
      - name: Provide ripgrep for the search tools
        run: |
          curl -fsSL -o "$tools_dir/ripgrep.tar.gz" \\
            "https://github.com/BurntSushi/ripgrep/releases/download/14.1.1/ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz"
          echo "$tools_dir" >> "$GITHUB_PATH"
"""

SETUP_NODE = """\
      - uses: actions/setup-node@v4
        with:
          node-version: 20
"""

NODE_STEP = """\
      - name: Frontend checks
        run: |
          npm ci
          node --check scripts/npm/publish-npm.sh.js
"""


def workflow(job_body: str, job_name: str = 'checks') -> str:
    return (
        'name: CI\n'
        'on: push\n'
        'jobs:\n'
        f'  {job_name}:\n'
        '    runs-on: ubuntu-latest\n'
        '    steps:\n'
        '      - uses: actions/checkout@v4\n'
        f'{job_body}'
    )


def run_checker(root: Path, *files: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(SCRIPT), *files],
        cwd=root,
        capture_output=True,
        text=True,
    )


class WorkflowToolchainTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def write(self, text: str, name: str = 'ci.yml') -> str:
        path = self.root / name
        path.write_text(text, encoding='utf-8')
        return name

    # ---- the real repository -----------------------------------------------

    def test_real_workflows_provision_everything_they_run(self) -> None:
        # No file names: discovery in the real checkout is what has to cover every
        # workflow the repository has, including any added after this test was written.
        shipped = sorted((REPO / '.github' / 'workflows').glob('*.yml'))
        self.assertGreaterEqual(len(shipped), 3, 'discovery found nothing to check')
        result = subprocess.run([sys.executable, str(SCRIPT)], cwd=REPO,
                                capture_output=True, text=True)
        self.assertEqual(
            result.returncode,
            0,
            f'real workflows are missing a tool:\n{result.stderr}',
        )
        self.assertIn(f'{len(shipped)} workflow file(s)', result.stdout)

    # ---- ripgrep ------------------------------------------------------------

    def test_guard_step_without_ripgrep_names_the_job_and_the_step(self) -> None:
        path = self.write(workflow(GUARD_STEP, job_name='docs-l10n'))
        result = run_checker(self.root, path)
        self.assertEqual(result.returncode, 1)
        self.assertIn('[docs-l10n]', result.stderr)
        self.assertIn('guard self-tests', result.stderr)
        self.assertIn('ripgrep', result.stderr)

    def test_apt_install_before_the_guard_counts(self) -> None:
        path = self.write(workflow(APT_RG + GUARD_STEP))
        result = run_checker(self.root, path)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_tarball_install_before_the_guard_counts(self) -> None:
        path = self.write(workflow(TARBALL_RG + GUARD_STEP))
        result = run_checker(self.root, path)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_install_after_the_guard_does_not_count(self) -> None:
        path = self.write(workflow(GUARD_STEP + APT_RG))
        result = run_checker(self.root, path)
        self.assertEqual(result.returncode, 1)
        self.assertIn('no earlier step', result.stderr)

    def test_install_earlier_in_the_same_script_counts(self) -> None:
        step = (
            '      - name: guard with its own dependency\n'
            '        run: |\n'
            '          sudo apt-get install -y --no-install-recommends ripgrep\n'
            '          bash scripts/l10n-guard.sh --before HEAD --after HEAD\n'
        )
        result = run_checker(self.root, self.write(workflow(step)))
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_install_later_in_the_same_script_does_not_count(self) -> None:
        step = (
            '      - name: guard then regret\n'
            '        run: |\n'
            '          bash scripts/l10n-guard.sh --before HEAD --after HEAD\n'
            '          sudo apt-get install -y --no-install-recommends ripgrep\n'
        )
        result = run_checker(self.root, self.write(workflow(step)))
        self.assertEqual(result.returncode, 1)

    # ---- comments and names are not scripts ---------------------------------

    def test_comment_mentioning_the_guard_is_not_a_need(self) -> None:
        step = (
            '      - name: unrelated\n'
            '        run: |\n'
            '          # scripts/l10n-guard.sh would be the thing to run here one day\n'
            '          python3 scripts/ci/something-else.py\n'
        )
        result = run_checker(self.root, self.write(workflow(step)))
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_trailing_comment_is_not_a_need(self) -> None:
        step = (
            '      - name: unrelated\n'
            '        run: python3 scripts/ci/something-else.py  # see scripts/l10n-guard.sh\n'
        )
        result = run_checker(self.root, self.write(workflow(step)))
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_step_name_mentioning_the_guard_is_not_a_need(self) -> None:
        # The name has to carry the exact string the trigger matches: a name
        # reading `l10n-guard.sh` would pass even if names were scanned as
        # scripts, because the trigger is the path `scripts/l10n-guard.sh`.
        step = (
            '      - name: Re-run scripts/l10n-guard.sh from a cached report\n'
            '        run: python3 scripts/ci/print_report.py\n'
        )
        result = run_checker(self.root, self.write(workflow(step)))
        self.assertEqual(result.returncode, 0, result.stderr)

    # ---- node ---------------------------------------------------------------

    def test_node_step_with_setup_node_counts(self) -> None:
        path = self.write(workflow(SETUP_NODE + NODE_STEP, job_name='gui'))
        result = run_checker(self.root, path)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_node_step_without_setup_node_is_named(self) -> None:
        path = self.write(workflow(NODE_STEP, job_name='gui'))
        result = run_checker(self.root, path)
        self.assertEqual(result.returncode, 1)
        self.assertIn('node', result.stderr)
        self.assertIn('actions/setup-node', result.stderr)

    def test_the_string_npm_in_a_path_is_not_a_node_program(self) -> None:
        step = (
            '      - name: require release + ci workflows\n'
            '        run: |\n'
            "          grep -q 'xai-grok-pager-bin' .github/workflows/release.yml\n"
            "          grep -q 'chaos-code' crates/codegen/xai-grok-pager/npm/chaos/package.json\n"
        )
        result = run_checker(self.root, self.write(workflow(step, job_name='workflows-present')))
        self.assertEqual(result.returncode, 0, result.stderr)

    # ---- fail closed --------------------------------------------------------

    def test_named_missing_workflow_fails(self) -> None:
        result = run_checker(self.root, 'nope.yml')
        self.assertEqual(result.returncode, 1)
        self.assertIn('does not exist', result.stderr)

    def test_a_third_workflow_is_checked_without_anyone_naming_it(self) -> None:
        # The hole the hard-coded pair left: a workflow file the gates were never told
        # about installed nothing and needed nothing as far as any of them was
        # concerned. Discovery means the guard job below is refused from `.github/
        # workflows/` without appearing in a list.
        directory = self.root / '.github' / 'workflows'
        directory.mkdir(parents=True)
        (directory / 'ci.yml').write_text(
            workflow(APT_RG + GUARD_STEP, job_name='docs-l10n'), encoding='utf-8')
        (directory / 'docker-labs.yml').write_text(
            workflow(GUARD_STEP, job_name='labs'), encoding='utf-8')
        result = subprocess.run([sys.executable, str(SCRIPT)], cwd=self.root,
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn('docker-labs.yml', result.stderr)
        self.assertIn('ripgrep', result.stderr)

    def test_no_workflows_at_all_refuses_to_pass(self) -> None:
        result = subprocess.run(
            [sys.executable, str(SCRIPT)],
            cwd=self.root,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn('no workflow files', result.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2)
