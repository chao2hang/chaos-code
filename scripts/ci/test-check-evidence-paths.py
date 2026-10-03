#!/usr/bin/env python3
"""Fixtures for `check-evidence-paths.py`.

Each case pins one decision that a "just grep the docs" implementation would get
wrong: the phrase in either language, the hyphenated spelling, the absolute
scratch root, an unresolved `{SCRATCH}/x.log` citation copied out of a plan, the
generated `.tsv` export (which repeats every `TODO.md` row and
therefore rots silently when only the Markdown is rewritten), and the two things
that must *not* be scanned -- captured transcripts, which quote the scratch path
of the command they ran because that is what happened, and `scripts/ci/*.tsv`,
which is guard data.

The exemption is checked in both directions as well: the shipped prompt template
may use the phrase as product vocabulary, and an exemption whose file has gone is
itself a failure, so the list cannot keep a retired waiver.

    python3 scripts/ci/test-check-evidence-paths.py
"""

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name('check-evidence-paths.py')
REPO = SCRIPT.parents[2]
_spec = importlib.util.spec_from_file_location('check_evidence_paths', SCRIPT)
guard = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(guard)

EXEMPT_REL = next(iter(guard.EXEMPT))
EXEMPT_PLANNER = ('crates/codegen/xai-grok-shell/src/session/templates/'
                  'goal_planner_prompt.md')
# The phrase this document is exempted for, kept in the fixture so the test
# proves the exemption suppresses a real hit rather than a file nobody flags.
EXEMPT_TEXT = (
    '# Strategist prompt\n'
    '- `{SCRATCH_ROOT}` — per-goal scratch root with the implementer\'s logs\n'
)
# The planner prompt is exempted for the fifth pattern: it is the document that
# defines the `{SCRATCH}/out.log` spelling, so the exemption has to cover an
# actual placeholder path, not just the word.
EXEMPT_PLANNER_TEXT = (
    '# Planner prompt\n'
    '- Output paths use the literal `{SCRATCH}` placeholder (e.g. '
    '`{SCRATCH}/out.log`), never a hardcoded path.\n'
)


def write(root: Path, rel: str, text: str) -> Path:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding='utf-8')
    return path


def build(root: Path, *, with_exempt: bool = True) -> Path:
    """A repository-shaped tree: docs, a captured transcript, guard data."""
    write(root, 'docs/verification/evidence.log',
          '$ python3 /tmp/grok-goal-abc123/implementer/mutate1.sh\n'
          'mutation survived\n')
    write(root, 'scripts/ci/baseline.tsv',
          'row\tquoted from a note about the goal scratch\n')
    write(root, 'TODO.md',
          '- [x] 修复完成，回归通过。输出见 `docs/verification/evidence.log`。\n')
    write(root, 'docs/architecture/note.md',
          'Evidence is in `docs/verification/evidence.log`.\n')
    if with_exempt:
        write(root, EXEMPT_REL, EXEMPT_TEXT)
        write(root, EXEMPT_PLANNER, EXEMPT_PLANNER_TEXT)
    return root


class Scan(unittest.TestCase):
    def scan(self, **kwargs) -> list[str]:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp), **kwargs)
            return guard.check(root)

    def test_clean_tree_passes(self) -> None:
        self.assertEqual(self.scan(), [])

    def test_english_scratch_phrase_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, 'TODO.md',
                  'logs are in the private goal scratch `x.log`\n')
            problems = guard.check(root)
        self.assertEqual(len(problems), 1, problems)
        self.assertTrue(problems[0].startswith('TODO.md:1:'), problems[0])

    def test_chinese_scratch_phrase_fails(self) -> None:
        for phrase in ('日志位于私有 scratch', '日志位于 goal 私有 scratch',
                       '日志位于私有 goal scratch'):
            with self.subTest(phrase=phrase):
                with tempfile.TemporaryDirectory() as tmp:
                    root = build(Path(tmp))
                    write(root, 'TODO.md', f'{phrase} `x.log`\n')
                    self.assertEqual(len(guard.check(root)), 1)

    def test_hyphenated_spelling_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, 'docs/architecture/note.md',
                  'see the goal-scratch directory for `x.log`\n')
            self.assertEqual(len(guard.check(root)), 1)

    def test_case_insensitive(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, 'docs/architecture/note.md',
                  'See the Goal Scratch for `x.log`\n')
            self.assertEqual(len(guard.check(root)), 1)

    def test_absolute_scratch_root_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, 'CHANGELOG.md',
                  '截图存于 `/tmp/grok-goal-3fd74e087187/implementer/`\n')
            problems = guard.check(root)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn('absolute path', problems[0])

    def test_plan_scratch_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, 'docs/architecture/note.md',
                  'paths in the earlier plan scratch do not align\n')
            self.assertEqual(len(guard.check(root)), 1)

    def test_unresolved_placeholder_cited_as_a_path_fails(self) -> None:
        # What actually shipped in TODO.md: a row whose evidence is named
        # `{SCRATCH}/updater-progress-style-test.log`. The braces survive the
        # copy out of a plan, so the "path" is a template nobody can open.
        for citation in ('`{SCRATCH}/run.log`', '`{SCRATCH_ROOT}/run.log`',
                         '`{SCRATCH} /run.log`'):
            with self.subTest(citation=citation):
                with tempfile.TemporaryDirectory() as tmp:
                    root = build(Path(tmp))
                    write(root, 'TODO.md', f'证据（未留存日志 {citation}）\n')
                    problems = guard.check(root)
                self.assertEqual(len(problems), 1, problems)
                self.assertIn('placeholder', problems[0])

    def test_placeholder_named_without_a_path_is_not_an_evidence_pointer(self) -> None:
        # The product prompt that explains the spelling says the literal word
        # out loud; only a citation shaped like a path is a pointer.
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, 'docs/architecture/note.md',
                  'plans name outputs with the literal `{SCRATCH}` placeholder\n')
            self.assertEqual(guard.check(root), [])

    def test_exempt_prompt_actually_needs_its_exemption(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp), with_exempt=False)
            path = write(root, EXEMPT_PLANNER, EXEMPT_PLANNER_TEXT)
            hits = guard.scan_file(path, root)
        self.assertEqual(len(hits), 1, hits)
        self.assertIn('placeholder', hits[0])

    def test_generated_tsv_is_scanned(self) -> None:
        # TODO.md was scrubbed but the export still carries the old row text;
        # this is the case that a Markdown-only check would miss entirely.
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, 'docs/verification/todo-open-items.tsv',
                  '296partial### M1.1 row（日志位于私有 goal scratch）\n')
            problems = guard.check(root)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn('todo-open-items.tsv', problems[0])

    def test_guard_data_tsv_is_not_scanned(self) -> None:
        # scripts/ci/baseline.tsv in the fixture already carries the phrase.
        self.assertEqual(self.scan(), [])

    def test_captured_transcript_is_not_scanned(self) -> None:
        # docs/verification/evidence.log already carries the absolute path.
        self.assertEqual(self.scan(), [])

    def test_exempt_document_may_use_the_phrase(self) -> None:
        self.assertEqual(self.scan(), [])

    def test_stale_exemption_fails(self) -> None:
        problems = self.scan(with_exempt=False)
        self.assertEqual(len(problems), 2, problems)
        self.assertTrue(all('not in the scanned set' in p for p in problems),
                        problems)

    def test_build_output_is_not_walked(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, 'target/docs/copied.md', 'in the goal scratch\n')
            write(root, 'apps/chaos-ui/node_modules/dep/README.md',
                  'in the goal scratch\n')
            self.assertEqual(guard.check(root), [])


class RealRepository(unittest.TestCase):
    def test_committed_docs_are_clean(self) -> None:
        self.assertEqual(guard.check(REPO), [])

    def test_both_shipped_prompts_are_still_declared_exempt(self) -> None:
        # Checked as a case rather than at import time: an assertion in module
        # scope turns a deleted exemption into an unloadable test file, which is
        # indistinguishable from the guard never having run.
        self.assertIn(EXEMPT_REL, guard.EXEMPT)
        self.assertIn(EXEMPT_PLANNER, guard.EXEMPT)

    def test_the_planner_prompt_exemption_is_load_bearing(self) -> None:
        # The shipped prompt is exempt for the placeholder pattern, so it has to
        # be a real hit: if the file ever stops using the spelling, this says the
        # exemption is dead weight rather than leaving it to rot.
        hits = guard.scan_file(REPO / EXEMPT_PLANNER, REPO)
        self.assertEqual(len(hits), 1, hits)
        self.assertIn('placeholder', hits[0])

    def test_scan_set_is_not_accidentally_narrow(self) -> None:
        names = {p.relative_to(REPO).as_posix() for p in guard.candidates(REPO)}
        self.assertIn('TODO.md', names)
        self.assertIn('CHANGELOG.md', names)
        self.assertIn(EXEMPT_REL, names)
        self.assertTrue(any(n.endswith('.tsv') for n in names),
                        'the generated export must stay in scope')

    def test_cli_exit_code(self) -> None:
        ok = subprocess.run([sys.executable, str(SCRIPT), '--root', str(REPO)],
                            capture_output=True, text=True)
        self.assertEqual(ok.returncode, 0, ok.stdout + ok.stderr)
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, 'TODO.md', 'goal scratch `x.log`\n')
            bad = subprocess.run([sys.executable, str(SCRIPT), '--root', str(root)],
                                 capture_output=True, text=True)
            self.assertEqual(bad.returncode, 1)
            self.assertIn('TODO.md:1', bad.stderr)
            listed = subprocess.run(
                [sys.executable, str(SCRIPT), '--root', str(root), '--list'],
                capture_output=True, text=True)
            self.assertEqual(listed.returncode, 0)
            self.assertIn(f'{EXEMPT_REL} (exempt)', listed.stdout)
            self.assertIn(f'{EXEMPT_PLANNER} (exempt)', listed.stdout)


if __name__ == '__main__':
    unittest.main(verbosity=2)
