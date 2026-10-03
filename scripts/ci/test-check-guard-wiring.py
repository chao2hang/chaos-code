#!/usr/bin/env python3
"""Fixtures for `check-guard-wiring.py`.

The reachability rule is the kind of thing that reads obviously correct and then
reports "OK" for the wrong reason, so each case here pins one decision the real
repository does not exercise on its own: comments are not invocations, docs are
not callers, a helper reached through a script CI runs does count, a dangling
call site is an error, and an exemption for a deleted file is an error.

The second half covers the local mirror. `verify-in-docker.sh` runs part of these
guards and the rest stay CI-only, so a guard has to be in one of the two sets --
otherwise a line can leave the `gates` array and the local run quietly gets
weaker with nothing to say so. The list of CI-only guards is data in
`scripts/ci/docker-entry-ci-only.tsv`, and it rots in two directions: a row for a
deleted guard, and a row for a guard the entry point runs anyway.

The last class covers the third rule, the one the first two left open: both call
sites run a guard, but with what arguments. Four budgets for
`platform-gated-tests.py` were written at both, one lowering reached one of them,
and the repository stayed green with the two call sites asking for different
numbers. So a value both places pass has to be the same value, and that rule needs
its own fixtures -- which call sites count, which do not, and what happens when the
number is not where a line-by-line reader would look for it.

    python3 scripts/ci/test-check-guard-wiring.py
"""

import importlib.util
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


def build(
    root: Path,
    *,
    extra_ci: str = '',
    helper_in_lab: bool = True,
    ci_only: tuple[str, ...] = ('helper.py',),
) -> Path:
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
    # helper.py runs in CI through the lab and is deliberately not in the local
    # entry point, so it needs a row; the mirror cases below rewrite this file.
    rows = ''.join(f'{name}\tneeds a Windows runner\n' for name in ci_only)
    (root / 'scripts' / 'ci' / 'docker-entry-ci-only.tsv').write_text(
        '# what the local container cannot run, and why\n' + rows, encoding='utf-8'
    )
    # The checker's own allow-list names ignored-tests.sh, so a tree without it
    # would fail for exemption rot before it could say anything about the case
    # under test. The exemption-rot case below deletes this file again.
    (root / 'scripts' / 'ci' / 'ignored-tests.sh').write_text('#!/usr/bin/env bash\n', encoding='utf-8')
    return root


def run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(SCRIPT), *[str(a) for a in args]], capture_output=True, text=True)


def add_step(root: Path, *lines: str, name: str = 'the budget', workflow: str = 'ci.yml') -> None:
    """Append a step to a workflow. Two lines means a backslash continuation.

    The real ci.yml keeps `platform-gated-tests.py` on one line and its four
    budgets on the next four, so a fixture that only ever writes single-line
    commands would leave the joining untested.
    """
    run_line = ' \\\n          '.join(lines)
    path = root / '.github' / 'workflows' / workflow
    path.write_text(
        path.read_text(encoding='utf-8') + f'      - name: {name}\n        run: {run_line}\n', encoding='utf-8'
    )


def add_workflow(root: Path, name: str, *steps: str) -> Path:
    """A second workflow, for the case where two legs legitimately differ."""
    body = ''.join(f'      - name: step {index}\n        run: {step}\n' for index, step in enumerate(steps, 1))
    path = root / '.github' / 'workflows' / name
    header = f'name: {name}\non: push\njobs:\n  leg:\n    runs-on: ubuntu-latest\n    steps:\n'
    path.write_text(header + body, 'utf-8')
    return path


def add_gate(root: Path, command: str) -> None:
    """Append one entry to the `gates` array of the local entry point."""
    verify = root / 'scripts' / 'verify-in-docker.sh'
    verify.write_text(
        verify.read_text(encoding='utf-8').replace('gates=(\n', f'gates=(\n  "{command}"\n', 1), encoding='utf-8'
    )


def set_budget(root: Path, workflow_value: str, local_value: str) -> None:
    """Write the same flag at both call sites with the two given values."""
    add_step(root, 'python3 scripts/ci/wired.py --max-unreviewed ' + workflow_value)
    add_gate(root, 'budget: python3 scripts/ci/wired.py --max-unreviewed ' + local_value)
    return root


class RealRepositoryTests(unittest.TestCase):
    def test_every_gate_in_the_real_repo_is_reachable(self):
        result = run('--root', REPO)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('check-guard-wiring: OK', result.stdout)

    def test_the_real_classification_is_the_one_the_lists_claim(self):
        # Reads the shipped verdict out of the checker instead of restating it, so
        # a guard that starts or stops being mirrored has to move in the TSV --
        # not in a test that would otherwise agree with whatever the repo does.
        result = run('--root', REPO, '--list-mirror')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        state = {}
        for line in result.stdout.splitlines():
            if line.startswith(('mirrored', 'ci-only', 'exempt')):
                state[line.split()[1]] = line.split()[0]
        gates = sorted(p.name for p in (REPO / 'scripts' / 'ci').iterdir() if p.suffix in {'.py', '.sh', '.mjs'})
        self.assertEqual(sorted(state), gates, 'every guard must land in exactly one class')
        # `publish-npm.sh` publishes; the reason it is not in the ci-only list is
        # that its own harness runs it, which is what the transitive hop is for.
        self.assertEqual(state.get('publish-npm.sh'), 'mirrored')
        self.assertEqual(state.get('test-publish-npm.sh'), 'mirrored')
        # Reached through that harness rather than by a gate line of their own.
        # Neither rewrites the tree on the paths the harness takes -- the publisher
        # stops at its missing-binary check and the stamper waits for --version.
        self.assertEqual(state.get('local-publish-host.sh'), 'mirrored')
        self.assertEqual(state.get('stamp-npm-version.mjs'), 'mirrored')
        for name in ('check-powershell-syntax.py', 'release-integrity-serve.py'):
            self.assertEqual(state.get(name), 'ci-only', f'{name} needs something the image lacks')
        # The mirror check needs a built chaos-engine, so it rides the `--full` gate
        # list right after `cargo test` rather than the quick one. Counting it as
        # ci-only would claim the local entry point never looks at the shipped
        # TypeScript protocol mirror, which stopped being true when --full grew
        # a workspace build.
        self.assertEqual(state.get('check-gui-protocol.sh'), 'mirrored')
        self.assertNotIn('check-versions.sh', [n for n, c in state.items() if c != 'mirrored'])
        self.assertEqual(state.get('panic-site-census.py'), 'mirrored')

    def test_the_real_repo_actually_compares_the_four_budgets(self):
        # The rule has to have teeth on this repository, not only on fixtures. If
        # the reader stopped matching the real call sites, every synthetic case
        # could still pass while the actual budgets went unwatched -- so assert
        # they are genuinely seen at both, and that the one flag deliberately
        # passed on one side only genuinely stays out of the comparison.
        spec = importlib.util.spec_from_file_location('check_guard_wiring', SCRIPT)
        self.assertIsNotNone(spec)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        seen = {
            str(path.relative_to(REPO)): flags
            for path, flags in module.flags_by_gate(REPO, {g.name for g in module.gate_files(REPO)}).items()
        }
        workflow = seen.get('.github/workflows/ci.yml', {})
        local = seen.get('scripts/verify-in-docker.sh', {})
        compared = {
            (gate, flag)
            for gate in set(workflow) & set(local)
            for flag in set(workflow[gate]) & set(local[gate])
        }
        for flag in ('--max-unreviewed', '--max-blind-windows', '--max-blind-macos', '--max-assumption-free'):
            self.assertIn(('platform-gated-tests.py', flag), compared, f'{flag} is no longer compared')
        # ci.yml alone passes this one, and that asymmetry is intentional; if it
        # ever reached the comparison the check would demand a flag the container
        # has no way to satisfy.
        self.assertNotIn(('test-installer-asset-names.py', '--require'), compared)


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


    def test_a_guard_the_mirror_skips_and_nobody_recorded_is_named(self):
        # The hole this closes: someone deletes a line from the `gates` array (or
        # adds a guard to CI only), and the local run quietly covers less than it
        # did. Nothing but this rule notices.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory), ci_only=())
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('helper.py runs in CI but not in', result.stdout)

    def test_a_list_row_for_a_deleted_guard_is_an_error(self):
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory), ci_only=('helper.py', 'retired.py'))
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('retired.py no longer exists', result.stdout)

    def test_a_list_row_for_a_mirrored_guard_is_an_error(self):
        # wired.py is in the gates array, so a ci-only row for it would understate
        # what the local run covers -- the number on the OK line would be a lie.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory), ci_only=('helper.py', 'wired.py'))
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('wired.py is listed as CI-only', result.stdout)

    def test_a_list_row_without_a_reason_is_rejected(self):
        # A row whose tab is missing must not be skipped: silently dropping it
        # would report the guard as un-mirrored and send the reader to the wrong
        # file. The row number has to appear.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory), ci_only=())
            rows = '# what the local container cannot run, and why\nhelper.py needs a Windows runner\n'
            (root / 'scripts' / 'ci' / 'docker-entry-ci-only.tsv').write_text(rows, encoding='utf-8')
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('docker-entry-ci-only.tsv:2', result.stdout + result.stderr)

    def test_a_data_file_naming_a_guard_is_not_an_invocation(self):
        # Baselines and lists live under scripts/ci/ too (`*.tsv`, `*.txt`), and a
        # row naming a guard is a record, not a call. `reachable()` has always had
        # to say so; the mirror walk inherits the same rule, or the CI-only list
        # would mark the guards it exempts as run by the entry point. The row is
        # written as a path on purpose -- the other rule (names must appear in a
        # path position) would otherwise mask this one, and a mutation that lets
        # data files call guards would survive it.
        with tempfile.TemporaryDirectory() as directory:
            # A CI step reads the data file, so the walk does reach it -- whether
            # it then treats its rows as calls is the thing under test.
            root = build(
                Path(directory),
                extra_ci='      - name: baseline\n        run: cat scripts/ci/notes.tsv\n',
                helper_in_lab=False,
                ci_only=('helper.py',),
            )
            (root / 'scripts' / 'ci' / 'notes.tsv').write_text(
                'scripts/ci/helper.py\tmeasured once, in passing\n', encoding='utf-8'
            )
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('helper.py is not invoked', result.stdout)

    def test_a_guard_reached_through_a_mirrored_guard_counts_as_mirrored(self):
        # `test-publish-npm.sh` runs `publish-npm.sh`; the publisher itself should
        # not need a row, because the harness that exercises it already runs.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            (root / 'scripts' / 'ci' / 'harness.sh').write_text(
                '#!/usr/bin/env bash\npython3 "$(dirname "$0")/deep.py"\n', encoding='utf-8'
            )
            (root / 'scripts' / 'ci' / 'deep.py').write_text('#!/usr/bin/env python3\n', encoding='utf-8')
            verify = root / 'scripts' / 'verify-in-docker.sh'
            verify.write_text(
                verify.read_text(encoding='utf-8').replace(
                    'gates=(', 'gates=(\n  "harness: bash scripts/ci/harness.sh"'
                ),
                encoding='utf-8',
            )
            result = run('--root', root, '--list-mirror')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn('mirrored  deep.py', result.stdout)


class FlagAgreementTests(unittest.TestCase):
    """The third rule: where both call sites run a guard, they must ask the same thing."""

    def test_the_same_value_at_both_call_sites_is_accepted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            set_budget(root, '1106', '1106')
            result = run('--root', root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_a_value_raised_at_one_call_site_only_is_named(self):
        # The incident itself: 1106 in the workflow, 1108 still in the entry
        # point. Nothing else in the pipeline can see the pair.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            set_budget(root, '1106', '1108')
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("wired.py --max-unreviewed is passed as '1106'", result.stdout)
            self.assertIn("but '1108' by scripts/verify-in-docker.sh", result.stdout)

    def test_a_flag_only_one_call_site_passes_is_a_choice(self):
        # `--require` is passed by the Windows leg and by no Linux run, on
        # purpose. Comparing flags one side never mentions would demand the
        # container satisfy an argument it cannot.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            set_budget(root, '1106', '1106')
            add_step(root, 'python3 scripts/ci/wired.py --require', name='windows only')
            result = run('--root', root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_a_budget_written_on_a_continued_line_still_counts(self):
        # ci.yml keeps the command on one line and the budgets on the lines
        # below it. Read line by line, the workflow would appear to pass no
        # budget at all, and a budget counts as compared only where the numbers
        # are actually found.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            add_step(root, 'python3 scripts/ci/wired.py --quiet', '--max-unreviewed 1106')
            add_gate(root, 'budget: python3 scripts/ci/wired.py --quiet --max-unreviewed 1108')
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('wired.py --max-unreviewed', result.stdout)

    def test_a_flag_after_a_shell_connective_belongs_to_the_next_command(self):
        # The entry point keeps `test-x.py && x.py --budget N` on one line. The
        # budget belongs to `wired.py`; attributing it to the test file instead
        # compares the wrong pair and hides the drift it exists to catch.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            (root / 'scripts' / 'ci' / 'test-wired.py').write_text('#!/usr/bin/env python3\n', encoding='utf-8')
            add_step(root, 'python3 scripts/ci/test-wired.py --require', name='the test')
            add_step(root, 'python3 scripts/ci/wired.py --max-unreviewed 1106')
            add_gate(
                root,
                'budget: python3 scripts/ci/test-wired.py --require'
                ' && python3 scripts/ci/wired.py --max-unreviewed 1108',
            )
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('wired.py --max-unreviewed', result.stdout)
            # One problem, not two: `--require` is passed the same way at both and
            # must not be swept into the report by the command it sits in front of.
            self.assertIn('1 problem(s)', result.stdout)

    def test_a_baseline_only_one_call_site_reads_is_named(self):
        # The rule is about values, not about numbers: a `--check-baseline` that
        # points at a different file on each side is the same class of drift, and
        # the two documents would then be maintained one at a time.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            for name in ('a.tsv', 'b.tsv'):
                (root / 'scripts' / 'ci' / name).write_text('name\tvalue\n', encoding='utf-8')
            add_step(root, 'python3 scripts/ci/wired.py --check-baseline scripts/ci/a.tsv')
            add_gate(root, 'budget: python3 scripts/ci/wired.py --check-baseline scripts/ci/b.tsv')
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("is passed as 'scripts/ci/a.tsv'", result.stdout)
            self.assertIn("but 'scripts/ci/b.tsv' by scripts/verify-in-docker.sh", result.stdout)

    def test_a_commented_out_number_is_not_a_call_site(self):
        # Both roots carry old numbers in comments explaining how a budget got
        # here. If prose counted, the check would report a disagreement between
        # two sentences -- and would then be ignored like any other noisy gate.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            set_budget(root, '1106', '1106')
            commented = 'python3 scripts/ci/wired.py --max-unreviewed {n} {note}'
            ci = root / '.github' / 'workflows' / 'ci.yml'
            ci.write_text(
                ci.read_text(encoding='utf-8')
                + '      # ' + commented.format(n='9999', note='was refused') + '\n',
                encoding='utf-8',
            )
            verify = root / 'scripts' / 'verify-in-docker.sh'
            verify.write_text(
                verify.read_text(encoding='utf-8')
                + '# ' + commented.format(n='8888', note='the old debt') + '\n',
                encoding='utf-8',
            )
            result = run('--root', root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_a_second_workflow_has_to_agree_with_the_entry_point_too(self):
        # Every workflow that runs a mirrored guard is compared with the local
        # entry point, not just the merge leg: a release leg that quietly loosens
        # a cap is the same drift, one file over. ci.yml agrees here, so exactly
        # one line is reported -- two would mean workflows were compared against
        # each other, which would make a legitimate second leg impossible.
        with tempfile.TemporaryDirectory() as directory:
            root = build(Path(directory))
            set_budget(root, '1106', '1106')
            add_workflow(root, 'release.yml', 'python3 scripts/ci/wired.py --max-unreviewed 4000')
            result = run('--root', root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("is passed as '4000' by .github/workflows/release.yml", result.stdout)
            self.assertIn('1 problem(s)', result.stdout)
            self.assertNotIn('by .github/workflows/ci.yml', result.stdout)


if __name__ == '__main__':
    unittest.main(verbosity=2)
