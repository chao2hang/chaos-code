#!/usr/bin/env python3
"""Fixtures for `check-lab-coverage.py`.

Every case here is one decision the real repository does not exercise on its own, and the set is
built around the two ways this rule can be satisfied for the wrong reason: a workflow that
mentions a lab in prose about why it is not wired, and a ledger row that reads well while pointing
at evidence the commit does not carry.

Dates are computed against the day the fixture runs rather than written as literals, because the
budget compares the ledger to today and a fixture holding a fixed date would go red or green for
reasons of arithmetic alone. The boundary case sits at both sides of the budget for the same
reason: it is what pins `>` rather than `>=`.

The shallow case exists because `actions/checkout` fetches one commit, so a genuine older
`last_sha` cannot be resolved in CI. Skipping resolution is correct there and nowhere else, so the
fixture builds a real shallow clone instead of asserting the exception.

Against the real repository these fixtures ask only the structural question -- every shipped lab
has an answer -- and leave the date budget to the gate itself. A fixture that went red because
thirty days passed would tell a reader nothing the gate does not already say, on the same day.

    python3 scripts/ci/test-check-lab-coverage.py
"""

import datetime as dt
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).with_name('check-lab-coverage.py')
REPO = HERE.parents[2]
GIT = ['git', '-c', 'user.name=fixture', '-c', 'user.email=fixture@example.invalid',
       '-c', 'commit.gpgsign=false']
LAB = 'scripts/alpha-in-docker.sh'
TRANSCRIPT = 'docs/verification/alpha-lab.log'
NO_JOBS = 'name: CI\non: push\njobs: {}\n'
RUNS_WORKFLOW = """\
name: CI
on: push
jobs:
  labs:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: the lab
        run: bash scripts/alpha-in-docker.sh
"""
HEADER = '\t'.join(['script', 'category', 'transcript', 'last_run', 'last_sha', 'verdict',
                    'reason'])
# A row has to name a commit the tree can resolve, so the fixtures write this token and
# `write_ledger` swaps in the fixture's own HEAD. An invented hex string would make the sha rule
# untestable in either direction: the gate is right to refuse it, and a fixture full of refused
# rows could not show anything else going wrong.
HEAD = '<head>'


def days_ago(days: int) -> str:
    return (dt.date.today() - dt.timedelta(days=days)).isoformat()


def gate(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(HERE), *[str(a) for a in args]],
                          capture_output=True, text=True)


def git(root: Path, *args: str) -> str:
    proc = subprocess.run([*GIT, '-C', str(root), *args], capture_output=True, text=True)
    if proc.returncode != 0:
        raise AssertionError(f'git {list(args)} failed: {proc.stderr}')
    return proc.stdout.strip()


def head(root: Path) -> str:
    return git(root, 'rev-parse', 'HEAD')[:12]


def row(script: str = LAB, *, category: str = 'consumes-published-artifact',
        transcript: str = TRANSCRIPT, last_run: str | None = None,
        sha: str = HEAD, verdict: str = 'green',
        reason: str = 'installs a released artifact') -> str:
    return '\t'.join([script, category, transcript, last_run or days_ago(0), sha, verdict,
                      reason])


def write_ledger(root: Path, rows: str, *, header: str = HEADER) -> None:
    (root / 'scripts' / 'ci').mkdir(parents=True, exist_ok=True)
    (root / 'scripts' / 'ci' / 'docker-labs.tsv').write_text(
        '# what CI cannot judge, and when it was last run\n'
        + header.replace(HEAD, head(root)) + '\n' + rows.replace(HEAD, head(root)),
        encoding='utf-8')


def commit_all(root: Path, message: str) -> str:
    git(root, 'add', '-A')
    git(root, 'commit', '-q', '-m', message)
    return git(root, 'rev-parse', 'HEAD')


def build(
    root: Path,
    *,
    labs: tuple[str, ...] = ('alpha-in-docker.sh',),
    workflow: str | None = None,
    rows: str | None = None,
    transcript: bool = True,
    transcript_tracked: bool = True,
) -> Path:
    """A committed tree holding the named labs, at most one workflow, and at most one ledger."""
    (root / 'scripts').mkdir(parents=True, exist_ok=True)
    (root / 'docs' / 'verification').mkdir(parents=True, exist_ok=True)
    for name in labs:
        (root / 'scripts' / name).write_text('#!/usr/bin/env bash\nexit 0\n', encoding='utf-8')
    (root / 'README.md').write_text('# fixture\n', encoding='utf-8')
    if workflow is not None:
        (root / '.github' / 'workflows').mkdir(parents=True, exist_ok=True)
        (root / '.github' / 'workflows' / 'ci.yml').write_text(workflow, encoding='utf-8')
    if transcript:
        (root / TRANSCRIPT).write_text('all 3 check(s) passed\n', encoding='utf-8')
    git(root, 'init', '-q', '-b', 'main')
    commit_all(root, 'fixture')
    if rows is not None:
        # Two commits: a ledger row vouches for a run that already happened, so the commit it
        # names has to exist before the row that names it does.
        write_ledger(root, rows)
        commit_all(root, 'the ledger')
    if not transcript_tracked:
        git(root, 'rm', '-q', '--cached', '--', TRANSCRIPT)
        git(root, 'commit', '-q', '--amend', '-m', 'fixture without the transcript')
    return root


def check(root: Path, *extra: str) -> subprocess.CompletedProcess[str]:
    return gate('--root', root, *extra)


class CoverageTests(unittest.TestCase):
    def setUp(self) -> None:
        self._dir = tempfile.TemporaryDirectory(prefix='lab-coverage-')
        self.root = Path(self._dir.name)
        self.addCleanup(self._dir.cleanup)

    def test_lab_run_by_a_workflow_needs_no_row(self) -> None:
        build(self.root, workflow=RUNS_WORKFLOW)
        proc = check(self.root)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn('1 run by CI', proc.stdout)

    def test_a_comment_mentioning_the_lab_is_not_an_invocation(self) -> None:
        # Both shapes have to be inert: a step commented out at its own indentation, and a note at
        # column 0, which is where a workflow explains why a lab is deliberately not wired. The
        # trailing-comment rule already cuts the indented one (a `#` preceded by whitespace), so
        # only the column-0 line keeps the whole-line rule honest.
        workflow = RUNS_WORKFLOW.replace(
            '      - name: the lab',
            '      # bash scripts/alpha-in-docker.sh  # wired later\n      - name: the lab')
        workflow = ('# scripts/alpha-in-docker.sh is deliberately not run here\n' + workflow)
        workflow = workflow.replace('        run: bash scripts/alpha-in-docker.sh',
                                    '        run: echo nothing')
        build(self.root, workflow=workflow)
        proc = check(self.root)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn('no workflow runs it', proc.stdout)

    def test_a_commented_out_command_at_end_of_a_line_is_not_an_invocation(self) -> None:
        workflow = RUNS_WORKFLOW.replace('        run: bash scripts/alpha-in-docker.sh',
                                         '        run: true  # bash scripts/alpha-in-docker.sh')
        build(self.root, workflow=workflow)
        self.assertEqual(check(self.root).returncode, 1)

    def test_unaccounted_lab_is_named(self) -> None:
        build(self.root, workflow=NO_JOBS)
        proc = check(self.root)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn(LAB, proc.stdout)
        self.assertIn('carries no row', proc.stdout)

    def test_a_current_row_is_enough(self) -> None:
        build(self.root, workflow=NO_JOBS, rows=row() + '\n')
        proc = check(self.root)
        self.assertEqual(proc.returncode, 0, proc.stdout)
        self.assertIn('1 with a current ledger row', proc.stdout)

    def test_the_budget_boundary_is_inclusive(self) -> None:
        build(self.root, workflow=NO_JOBS, rows=row(last_run=days_ago(30)) + '\n')
        self.assertEqual(check(self.root).returncode, 0)
        write_ledger(self.root, row(last_run=days_ago(31)) + '\n')
        commit_all(self.root, 'the row aged one day past the budget')
        proc = check(self.root)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn('31 days ago', proc.stdout)
        self.assertIn('past the 30-day budget', proc.stdout)
        self.assertIn('bash ' + LAB, proc.stdout)

    def test_a_future_run_date_is_a_problem(self) -> None:
        build(self.root, workflow=NO_JOBS, rows=row(last_run=days_ago(-1)) + '\n')
        self.assertIn('is in the future', check(self.root).stdout)

    def test_an_unparseable_run_date_is_a_problem(self) -> None:
        build(self.root, workflow=NO_JOBS, rows=row(last_run='last Tuesday') + '\n')
        self.assertIn('not an ISO date', check(self.root).stdout)

    def test_a_row_for_a_lab_ci_now_runs_is_redundant(self) -> None:
        build(self.root, workflow=RUNS_WORKFLOW, rows=row() + '\n')
        proc = check(self.root)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn('the row is redundant', proc.stdout)

    def test_a_row_for_an_untracked_script_is_a_problem(self) -> None:
        build(self.root, workflow=NO_JOBS, rows=row(script='scripts/ghost-in-docker.sh') + '\n')
        proc = check(self.root)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn('is not tracked', proc.stdout)
        # A row is only about a lab the repository carries, so the ghost is not also reported as
        # an unaccounted lab.
        self.assertNotIn('scripts/ghost-in-docker.sh: no workflow runs it', proc.stdout)

    def test_two_rows_for_one_lab_are_a_problem(self) -> None:
        build(self.root, workflow=NO_JOBS,
              rows=row() + '\n' + row(reason='a second opinion') + '\n')
        proc = check(self.root)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn('only one row can be current', proc.stdout)

    def test_an_unknown_category_is_a_problem_and_lists_the_choices(self) -> None:
        build(self.root, workflow=NO_JOBS, rows=row(category='too-slow') + '\n')
        proc = check(self.root)
        self.assertIn("category 'too-slow'", proc.stdout)
        for name in ('consumes-published-artifact', 'needs-external-service',
                     'already-done-in-ci'):
            self.assertIn(name, proc.stdout)

    def test_an_untracked_transcript_is_a_problem(self) -> None:
        # The log exists on this machine and in no commit: that is the difference between evidence
        # for a run and a file somebody left behind.
        build(self.root, workflow=NO_JOBS, rows=row() + '\n', transcript_tracked=False)
        proc = check(self.root)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn('is not tracked; a log that exists only', proc.stdout)

    def test_a_transcript_outside_docs_verification_is_a_problem(self) -> None:
        build(self.root, workflow=NO_JOBS, rows=row(transcript='notes/alpha.log') + '\n')
        self.assertIn('not under docs/verification/', check(self.root).stdout)

    def test_an_empty_reason_is_a_problem(self) -> None:
        build(self.root, workflow=NO_JOBS, rows=row(reason='  ') + '\n')
        self.assertIn('reason is empty', check(self.root).stdout)

    def test_an_unknown_verdict_is_a_problem(self) -> None:
        build(self.root, workflow=NO_JOBS, rows=row(verdict='mostly') + '\n')
        proc = check(self.root)
        self.assertIn("verdict 'mostly'", proc.stdout)
        self.assertIn('green or red', proc.stdout)

    def test_a_declared_red_run_is_accepted(self) -> None:
        # A lab may be right about a defect CI cannot fix; what is not allowed is for a dated run
        # to imply a pass it did not report.
        build(self.root, workflow=NO_JOBS,
              rows=row(verdict='red', reason='registry holds placeholder names') + '\n')
        self.assertEqual(check(self.root).returncode, 0, check(self.root).stdout)

    def test_a_sha_that_is_not_a_sha_is_a_problem(self) -> None:
        build(self.root, workflow=NO_JOBS, rows=row(sha='nope') + '\n')
        self.assertIn('is not a commit', check(self.root).stdout)

    def test_a_real_older_sha_resolves_and_a_fabricated_one_does_not(self) -> None:
        build(self.root, workflow=NO_JOBS)
        first = git(self.root, 'rev-parse', 'HEAD')
        (self.root / 'README.md').write_text('# second\n', encoding='utf-8')
        write_ledger(self.root, row(sha=first[:7]) + '\n')
        commit_all(self.root, 'ledger naming the first commit')
        self.assertEqual(check(self.root).returncode, 0, check(self.root).stdout)
        write_ledger(self.root, row(sha='ffffffff') + '\n')
        commit_all(self.root, 'ledger naming a sha that never existed')
        self.assertIn('is not a commit', check(self.root).stdout)

    def test_a_shallow_checkout_shape_checks_the_sha_instead(self) -> None:
        source = self.root / 'source'
        source.mkdir()
        build(source, workflow=NO_JOBS)
        first = git(source, 'rev-parse', 'HEAD')
        (source / 'README.md').write_text('# second\n', encoding='utf-8')
        commit_all(source, 'second')
        clone = self.root / 'clone'
        # `file://` is not decoration: git ignores `--depth` in a plain local clone and hardlinks
        # the whole object store instead, which would leave the fixture non-shallow and the
        # assertion below a tautology about a checkout the gate never sees.
        subprocess.run(['git', 'clone', '-q', '--depth', '1', f'file://{source}',
                        str(clone)], check=True, capture_output=True, text=True)
        self.assertEqual(git(clone, 'rev-parse', '--is-shallow-repository'), 'true')
        # The named commit is genuinely absent from this checkout and the row is still true.
        write_ledger(clone, row(sha=first[:7]) + '\n')
        commit_all(clone, 'ledger naming a commit this clone cannot see')
        proc = check(clone)
        self.assertEqual(proc.returncode, 0, proc.stdout)
        self.assertIn('shape-checked only', proc.stdout)
        # Non-hex is refused while shallow too, so the skip cannot widen into "shallow checkouts
        # are unchecked".
        write_ledger(clone, row(sha='zzzzzzz') + '\n')
        commit_all(clone, 'ledger naming non-hex')
        self.assertIn('is not a commit', check(clone).stdout)

    def test_a_misaligned_ledger_row_is_a_problem(self) -> None:
        build(self.root, workflow=NO_JOBS)
        write_ledger(self.root, LAB + '\tconsumes-published-artifact\n')
        commit_all(self.root, 'a row with two fields')
        self.assertIn('field(s), expected 7', check(self.root).stdout)

    def test_a_ledger_without_a_header_is_a_problem(self) -> None:
        build(self.root, workflow=NO_JOBS)
        write_ledger(self.root, row() + '\n', header=row())
        commit_all(self.root, 'no header')
        proc = check(self.root)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn('the header must be the tab-separated columns', proc.stdout)

    def test_an_untracked_ledger_excuses_nothing(self) -> None:
        build(self.root, workflow=NO_JOBS)
        write_ledger(self.root, row() + '\n')
        proc = check(self.root)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn('a file the repository does not track', proc.stdout)

    def test_a_second_lab_needs_its_own_answer(self) -> None:
        build(self.root, labs=('alpha-in-docker.sh', 'beta-in-docker.sh'),
              workflow=RUNS_WORKFLOW,
              rows=row(script='scripts/beta-in-docker.sh') + '\n')
        proc = check(self.root)
        self.assertEqual(proc.returncode, 0, proc.stdout)
        self.assertIn('2 lab(s): 1 run by CI, 1 with a current ledger row', proc.stdout)

    def test_a_new_lab_is_red_without_any_edit_to_the_gate(self) -> None:
        build(self.root, workflow=RUNS_WORKFLOW)
        (self.root / 'scripts' / 'beta-in-docker.sh').write_text('#!/usr/bin/env bash\n',
                                                                encoding='utf-8')
        commit_all(self.root, 'a new lab nobody runs')
        proc = check(self.root)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn('scripts/beta-in-docker.sh', proc.stdout)

    def test_a_lab_in_a_subdirectory_is_swept_in_too(self) -> None:
        # Hiding a lab one directory down must not be a way out of the rule.
        build(self.root, workflow=RUNS_WORKFLOW)
        (self.root / 'scripts' / 'labs').mkdir()
        (self.root / 'scripts' / 'labs' / 'extra-in-docker.sh').write_text(
            '#!/usr/bin/env bash\n', encoding='utf-8')
        commit_all(self.root, 'a lab in a subdirectory')
        proc = check(self.root)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn('scripts/labs/extra-in-docker.sh', proc.stdout)

    def test_a_workflow_naming_a_lab_that_does_not_exist_is_a_problem(self) -> None:
        workflow = RUNS_WORKFLOW + """\
      - name: a lab that was renamed away
        run: bash scripts/gone-in-docker.sh
"""
        build(self.root, workflow=workflow)
        proc = check(self.root)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn('scripts/gone-in-docker.sh, which the repository does not carry',
                      proc.stdout)

    def test_a_tracked_workflow_missing_from_disk_is_reported_not_crashed(self) -> None:
        build(self.root, workflow=RUNS_WORKFLOW)
        (self.root / '.github' / 'workflows' / 'ci.yml').unlink()
        proc = check(self.root)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn('tracked, but not on disk', proc.stdout)

    def test_a_tree_with_no_labs_reports_that_rather_than_crashing(self) -> None:
        build(self.root, labs=(), workflow=NO_JOBS)
        proc = check(self.root)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn('nothing to account for', proc.stdout)

    def test_list_names_every_lab_with_its_verdict(self) -> None:
        build(self.root, labs=('alpha-in-docker.sh', 'beta-in-docker.sh'),
              workflow=RUNS_WORKFLOW, rows=row(script='scripts/beta-in-docker.sh',
                                               verdict='red') + '\n')
        proc = check(self.root, '--list')
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn('alpha-in-docker.sh\trun by CI\t.github/workflows/ci.yml', proc.stdout)
        # The row names the commit the lab ran at, which is the one before the row itself.
        self.assertIn(f'beta-in-docker.sh\tledgered\t{days_ago(0)}\t'
                      f'{git(self.root, "rev-parse", "HEAD~1")[:12]}\tred\t'
                      'consumes-published-artifact', proc.stdout)

    def test_a_directory_named_like_a_lab_is_not_a_lab(self) -> None:
        build(self.root, workflow=RUNS_WORKFLOW)
        (self.root / 'scripts' / 'nested-in-docker.sh').mkdir()
        (self.root / 'scripts' / 'nested-in-docker.sh' / 'inside.sh').write_text(
            '#!/usr/bin/env bash\n', encoding='utf-8')
        commit_all(self.root, 'a directory with a lab-shaped name')
        self.assertEqual(check(self.root).returncode, 0, check(self.root).stdout)

    def test_every_lab_in_the_real_repository_has_an_answer(self) -> None:
        """The structural half of the drift this gate exists for, asked of the shipped ledger."""
        proc = check(REPO, '--list')
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        lines = [line for line in proc.stdout.splitlines() if line]
        self.assertGreaterEqual(len(lines), 5, proc.stdout)
        for line in lines:
            self.assertNotIn('\tunaccounted', line, f'lab with no answer: {line}')
            self.assertRegex(line, r'\t(run by CI|ledgered)\t')

    def test_the_real_ledger_rows_point_at_tracked_transcripts(self) -> None:
        listing = set(subprocess.run(['git', '-C', str(REPO), 'ls-files'],
                                     capture_output=True, text=True).stdout.splitlines())
        rows = [line for line in (REPO / 'scripts' / 'ci' / 'docker-labs.tsv').read_text(
            encoding='utf-8').splitlines()
            if line.strip() and not line.startswith('#') and not line.startswith('script\t')]
        self.assertGreaterEqual(len(rows), 1)
        for line in rows:
            fields = line.split('\t')
            self.assertEqual(len(fields), 7, line)
            self.assertIn(fields[0], listing, f'untracked lab named by a row: {fields[0]}')
            self.assertIn(fields[2], listing, f'untracked transcript: {fields[2]}')


if __name__ == '__main__':
    unittest.main(verbosity=2)
