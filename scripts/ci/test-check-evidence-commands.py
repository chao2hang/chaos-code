#!/usr/bin/env python3
"""Fixtures for `check-evidence-commands.py`.

Every case pins one decision the gate makes, and each decision is pinned from both
sides, because a scanner that stopped matching looks exactly like a repository whose
command lines are all correct. The pair for "python3 cannot run a .sh" is therefore not
just the wrong command but the right one in the same fixture: silence for
`bash scripts/ci/x.sh`, `python3 -c`, `python3 -m`, `python3 - <<'PY'` and `bash -c`,
where no script file is named and so nothing is claimed about a file.

Four things are pinned that are easy to get wrong and hard to notice:

- what counts as a command line at all: a `$ ` line anywhere, and any line of a
  shell-labelled fence, while the same text in a `text` fence without the prompt is
  prose and stays silent;
- that both rules read both shapes, so an absent path inside a fence is a finding, and
  that a line which *creates* what it names (`mkdir -p docs/verify`, `x.py > out.tsv`)
  is not a claim that the file was already there;
- that a finding is reported against the line `grep -n` prints, including in a file that
  contains `\f` and U+2028, which `str.splitlines` counts as breaks and `grep` does not;
- that rule 2 anchors on *tracked* top-level entries, so `target/`, an absolute scratch
  path and a directory that exists only on disk are not claims about this repository,
  while `scripts/gone.sh` is;
- that the verdict is a property of the commit and not of the checkout: an ignored build
  directory is no claim whether or not this tree was built, while an untracked file that
  only this checkout has is a claim even though the filesystem says it is there. Answering
  existence from the filesystem is what made the gate green on the machine that wrote a
  transcript naming `apps/chaos-ui/node_modules` and red on a runner with a clean tree;
- that a directory is content too, since `git ls-files` lists files only, so `ls docs/...`
  reads as a claim that holds while `ls scripts/nope` does not;
- that a recorded absence cannot rot: an allowlist entry whose finding is gone fails as
  stale, so the ledger cannot become a graveyard of fixed command lines;
- that each rule is recorded by its own key, a path for an absent file and the exact
  command for a quoted wrong command, and that neither kind of row excuses a finding
  belonging to the other rule;

One case runs against this repository itself, so the gate cannot be green in fixtures
and red on the tree it was written for.

    python3 scripts/ci/test-check-evidence-commands.py
"""

import importlib.util
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-evidence-commands.py")
REPO = SCRIPT.parents[2]
_spec = importlib.util.spec_from_file_location("check_evidence_commands", SCRIPT)
guard = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(guard)


def git(root: Path, *args: str) -> None:
    env = {**os.environ, "GIT_AUTHOR_NAME": "f", "GIT_AUTHOR_EMAIL": "f@e.invalid",
           "GIT_COMMITTER_NAME": "f", "GIT_COMMITTER_EMAIL": "f@e.invalid"}
    proc = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True, env=env)
    assert proc.returncode == 0, proc.stderr


def write(root: Path, rel: str, text: str) -> Path:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


CLEAN = """\
# transcript

    $ python3 scripts/ci/x.py --check
    $ python3 -u scripts/ci/x.py
    $ bash scripts/ci/x.sh
    $ python3 -c "import sys; print(sys.version)"
    $ python3 -m pytest -q
    $ python3 - <<'PY'
    print(open('scripts/ci/x.sh').read())
    PY
    $ bash -c "python3 -c 'print(1)'"
    $ RUST_MIN_STACK=16777216 python3 scripts/ci/x.py
    $ ./scripts/ci/x.sh 2>&1 | tee out.log
    $ node scripts/run.mjs
    $ pwsh ./scripts/deploy.ps1
    $ cp docs/exists.md /tmp && rm docs/*.md
    $ cat scripts/ci/x-<hash>.py
    $ git switch -c sync/upstream-$(date +%Y%m%d)
    $ ls target/debug/deps/xai_thing-9f2 && ls /tmp/scratch/probe.py
    $ ls ../other_repo/x.py && ls notatrackedtop/x.py
    $ ls docs/exists.md
"""

DIRTY = """\
# transcript

    $ python3 scripts/ci/x.sh
    $ bash scripts/ci/x.py
    $ pwsh scripts/ci/x.sh
    $ node scripts/ci/x.py
    $ /usr/bin/python3 scripts/ci/UPPER.SH
    $ cat notes | python3 scripts/ci/x.sh
    $ python3 -u scripts/deploy.ps1
    $ python3 "scripts/ci/x.sh"
    $ ls scripts/gone.sh
"""


def build(root: Path) -> Path:
    """A repository-shaped fixture: real git index, a transcript, docs, scripts."""
    git(root, "init", "-q")
    write(root, "docs/verification/one.log", CLEAN)
    write(root, "docs/exists.md", "a document\n")
    write(root, "scripts/ci/x.py", "print(1)\n")
    write(root, "scripts/ci/x.sh", "#!/usr/bin/env bash\n")
    write(root, "scripts/ci/UPPER.SH", "#!/usr/bin/env bash\n")
    write(root, "scripts/run.mjs", "console.log(1)\n")
    write(root, "scripts/deploy.ps1", "Write-Host 1\n")
    # Only tracked content can anchor a claim, so this directory exists on disk and owns
    # nothing: a path into it is not a claim about this repository.
    write(root, "on_disk_only/x.py", "print(1)\n")
    # Two generated shapes sitting under a *tracked* top-level entry, where the top-level
    # filter lets them through and only the ignore rule can excuse them: a directory
    # pattern, which git matches only against a directory, and a plain file pattern.
    write(root, ".gitignore", "on_disk_only/\nscripts/vendor/\ndocs/secret.env\n")
    git(root, "add", "-A")
    return root


def run(root: Path, *extra: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(SCRIPT), "--root", str(root), *extra],
                          capture_output=True, text=True)


class ScanTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = build(Path(self._tmp.name))
        self.addCleanup(self._tmp.cleanup)

    # -- the fixture proves the scanner runs ------------------------------------

    def test_clean_transcript_is_quiet_and_is_actually_read(self) -> None:
        proc = run(self.root)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        numbers = [int(n) for n in re.findall(r"(\d+) (?:files|command lines)", proc.stdout)]
        self.assertEqual(numbers[0], 2, proc.stdout)  # one log + docs/exists.md
        self.assertGreater(numbers[1], 15, proc.stdout)

    def test_every_wrong_interpreter_pair_is_named_with_its_line(self) -> None:
        write(self.root, "docs/verification/two.log", DIRTY)
        git(self.root, "add", "-A")
        proc = run(self.root)
        self.assertEqual(proc.returncode, 1)
        for lineno, cmd, want in (
            (3, "python3 scripts/ci/x.sh", "run it with bash"),
            (4, "bash scripts/ci/x.py", "run it with python3"),
            (5, "pwsh scripts/ci/x.sh", "run it with bash"),
            (6, "node scripts/ci/x.py", "run it with python3"),
            (7, "/usr/bin/python3 scripts/ci/UPPER.SH", "run it with bash"),
            (8, "cat notes | python3 scripts/ci/x.sh", "run it with bash"),
            (9, "python3 -u scripts/deploy.ps1", "run it with pwsh"),
            (10, 'python3 "scripts/ci/x.sh"', "run it with bash"),
        ):
            hits = [p for p in proc.stderr.splitlines()
                    if f"two.log:{lineno}:" in p and want in p]
            self.assertEqual(len(hits), 1, f"line {lineno}: {proc.stderr}")
            self.assertIn(cmd.split("| ")[-1].split(" ")[-1].strip("\"'"), hits[0])
        # The quotes around an operand are the fixture's, not part of the path: line 10 is
        # reported without them, so a reader can copy the path out of the finding.
        self.assertIn("two.log:10: `python3 scripts/ci/x.sh` cannot work", proc.stderr)
        # Rule 2 still fires on the same file, and the two rules are separate findings.
        self.assertEqual(proc.stderr.count("cannot work"), 8, proc.stderr)
        self.assertIn("two.log:11: `scripts/gone.sh`", proc.stderr)

    def test_a_path_qualified_interpreter_is_still_the_same_interpreter(self) -> None:
        # Pinned separately: the mismatch above is found by basename, so a venv path does
        # not excuse the wrong family.
        problems = guard.check_interpreter("d.log", 1, "./venv/bin/python3 scripts/ci/x.sh")
        self.assertEqual(len(problems), 1, problems)

    # -- what counts as a command line ------------------------------------------

    def test_prose_is_not_a_command_but_a_shell_fence_line_is(self) -> None:
        write(self.root, "docs/guide.md",
              "Run `python3 scripts/ci/x.sh` if you like.\n"
              "```text\npython3 scripts/ci/x.sh\n```\n"
              "```sh\npython3 scripts/ci/x.sh\ncat docs/does-not-exist.md\n"
              "mkdir -p docs/verify && touch docs/verify/new.log\n```\n")
        git(self.root, "add", "-A")
        proc = run(self.root)
        self.assertEqual(proc.returncode, 1, proc.stderr)
        hits = [p for p in proc.stderr.splitlines() if "docs/guide.md:" in p]
        self.assertEqual(len(hits), 2, proc.stderr)
        # Both rules read a fence line: line 6 for the interpreter rule, line 7 for the
        # path rule, because `cat` is not a command that puts a file there.
        self.assertIn("docs/guide.md:6: `python3 scripts/ci/x.sh` cannot work", hits[0])
        self.assertIn("docs/guide.md:7: `docs/does-not-exist.md`", hits[1])
        # Line 8 is the same shape as a line 7 that creates what it names: silence.
        self.assertNotIn("docs/guide.md:8:", proc.stderr)
        # The prose and the `text` fence carry the same wrong command and stay silent.
        self.assertEqual(proc.stderr.count("cannot work"), 1, proc.stderr)

    def test_a_command_written_inside_quotes_is_data_and_not_a_claim(self) -> None:
        # Writing down a ledger row for a wrong command means printing that wrong command,
        # and so does quoting it in an example. Neither is a claim that it should run.
        write(self.root, "docs/guide.md",
              "```sh\nprintf '%s\\n' 'python3 scripts/ci/x.sh\\tquoted-command\\twhy'\n"
              "echo \"python3 scripts/ci/x.sh\" is the typo that started this\n```\n")
        git(self.root, "add", "-A")
        quiet = run(self.root)
        self.assertEqual(quiet.returncode, 0, quiet.stderr)
        # Out of the quotes, the same text is a command and is reported.
        write(self.root, "docs/guide.md", "```sh\npython3 scripts/ci/x.sh\n```\n")
        git(self.root, "add", "-A")
        proc = run(self.root)
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("`python3 scripts/ci/x.sh` cannot work", proc.stderr)

    def test_a_prompt_line_inside_a_plain_fence_is_still_a_command(self) -> None:
        write(self.root, "docs/guide.md",
              "```text\n    $ bash scripts/ci/x.sh\n    $ python3 scripts/ci/x.sh\n```\n")
        git(self.root, "add", "-A")
        proc = run(self.root)
        self.assertEqual(proc.returncode, 1, proc.stderr)
        hits = [p for p in proc.stderr.splitlines() if "docs/guide.md:" in p]
        self.assertEqual(len(hits), 1, proc.stderr)
        self.assertIn("docs/guide.md:3:", hits[0])   # the `$` line, prompt-marked
        self.assertNotIn("docs/guide.md:2:", proc.stderr)  # a fence that is not a shell

    def test_fences_close_on_their_own_marker_and_length(self) -> None:
        # A 3-run cannot close a 4-run fence, and a fence cannot close a fence written
        # with the other marker, so the lines that are commands are pinned one by one.
        write(self.root, "docs/guide.md",
              "~~~sh\npython3 scripts/ci/x.sh\n~~~\n"
              "````sh\npython3 scripts/ci/x.sh\n```\n"
              "python3 scripts/ci/x.sh\n"
              "````\nbash scripts/ci/x.sh\n~~~\n"
              "python3 scripts/ci/x.sh\n")
        git(self.root, "add", "-A")
        proc = run(self.root)
        self.assertEqual(proc.returncode, 1, proc.stderr)
        hits = sorted(int(p.split("guide.md:")[1].split(":")[0])
                      for p in proc.stderr.splitlines() if "docs/guide.md:" in p)
        self.assertEqual(hits, [2, 5, 7], proc.stderr)

    def test_line_numbers_match_grep_even_with_form_feed_and_u2028(self) -> None:
        text = ("line\n\x0c\n\u2028\n\n    $ python3 scripts/ci/x.sh\n")
        write(self.root, "docs/verification/three.log", text)
        git(self.root, "add", "-A")
        proc = run(self.root)
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("three.log:5:", proc.stderr)
        grep = subprocess.run(["grep", "-n", "-F", "$ python3 scripts/ci/x.sh",
                               str(self.root / "docs/verification/three.log")],
                              capture_output=True, text=True)
        self.assertTrue(grep.stdout.startswith("5:"), grep.stdout)

    # -- rule 2: the path has to still be there ---------------------------------

    def test_missing_path_is_reported_and_recording_it_silences_only_that(self) -> None:
        write(self.root, "docs/verification/four.log",
              "    $ python3 scripts/ci/old-name.py > docs/exists.md\n"
              "    $ ls scripts/another-gone.py\n")
        git(self.root, "add", "-A")
        proc = run(self.root)
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("scripts/ci/old-name.py", proc.stderr)
        self.assertIn("scripts/another-gone.py", proc.stderr)
        ledger = write(self.root, "ledger.tsv",
                       "scripts/ci/old-name.py\thistorical\tthe run this transcript "
                       "captured used that name before it was renamed\n")
        proc = run(self.root, "--allowlist", str(ledger))
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertNotIn("old-name.py", proc.stderr)
        self.assertIn("scripts/another-gone.py", proc.stderr)
        self.assertIn("1 recorded", proc.stderr)

    def test_a_pattern_or_foreign_root_is_not_a_claim(self) -> None:
        body = (
            "    $ cp docs/exists.md /tmp\n"
            "    $ rm docs/*.md\n"
            "    $ cat scripts/ci/x-<hash>.py\n"
            "    $ git switch -c sync/upstream-$(date +%Y%m%d)\n"
            "    $ ls target/debug/deps/x-9f2\n"
            "    $ ls /tmp/grok-goal-1/implementer/probe.py\n"
            "    $ ls ../other/x.py\n"
            "    $ ls on_disk_only/x.py\n"
            "    $ mkdir -p docs/verify && touch docs/verify/new.log\n"
            "    $ python3 scripts/ci/x.py > scripts/ci/new-output.py\n")
        write(self.root, "docs/verification/five.log", body)
        git(self.root, "add", "-A")
        proc = run(self.root)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(guard.check_paths(
            guard.Repo(self.root), "d.log", 1, "ls docs/exists.md"), [])

    def test_generated_output_is_no_claim_built_or_unbuilt(self) -> None:
        # The verdict has to be a property of the commit, not of the checkout: this is the
        # difference between green on the machine that wrote the transcript and red in CI.
        # A build directory is named by .gitignore, so reading it is a recipe with a build
        # step in front of it rather than a reference to a file that went missing.
        write(self.root, "docs/verification/gen.log",
              "    $ du -sh scripts/vendor\n"
              "    $ cat docs/secret.env\n")
        git(self.root, "add", "-A")
        self.assertFalse((self.root / "scripts" / "vendor").exists())
        unbuilt = run(self.root)
        self.assertEqual(unbuilt.returncode, 0, unbuilt.stderr)
        (self.root / "scripts" / "vendor").mkdir()
        (self.root / "scripts" / "vendor" / "lib.js").write_text("export 1\n", encoding="utf-8")
        (self.root / "docs" / "secret.env").write_text("TOKEN=1\n", encoding="utf-8")
        built = run(self.root)
        self.assertEqual(built.returncode, 0, built.stderr)
        # Same commit, same verdict, built or not.
        self.assertEqual(unbuilt.stdout, built.stdout)

    def test_only_this_checkout_having_a_file_is_not_a_clean_verdict(self) -> None:
        # The mirror image of the case above: neither tracked nor declared generated, so
        # the path is a claim about the repository even though this checkout has the file.
        # Answering rule 2 from the filesystem forgave exactly this.
        write(self.root, "scripts/only_here/tool.py", "print(1)\n")
        write(self.root, "docs/verification/mine.log",
              "    $ python3 scripts/only_here/tool.py\n")
        git(self.root, "add", "docs/verification/mine.log")
        self.assertTrue((self.root / "scripts" / "only_here" / "tool.py").exists())
        proc = run(self.root)
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("scripts/only_here/tool.py", proc.stderr)

    def test_a_tracked_directory_can_be_what_a_command_reads(self) -> None:
        # `git ls-files` never lists directories, so a transcript reading a directory
        # would be reported as missing if existence were answered from that list alone.
        write(self.root, "docs/verification/dirs.log",
              "    $ ls scripts/ci\n"
              "    $ ls docs/verification\n"
              "    $ ls scripts/nope\n")
        git(self.root, "add", "-A")
        proc = run(self.root)
        self.assertEqual(proc.returncode, 1, proc.stderr)
        findings = [ln for ln in proc.stderr.splitlines() if "names a file" in ln]
        self.assertEqual(len(findings), 1, proc.stderr)
        self.assertIn("scripts/nope", findings[0])

    def test_recorded_absence_that_no_longer_happens_fails_as_stale(self) -> None:
        ledger = write(self.root, "ledger.tsv",
                       "scripts/gone.sh\trecorded-absent\tthis entry describes nothing "
                       "that the current tree still shows, on purpose\n")
        proc = run(self.root, "--allowlist", str(ledger))
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("stale", proc.stderr)
        self.assertIn("scripts/gone.sh", proc.stderr)

    def test_all_flag_prints_recorded_findings_too(self) -> None:
        write(self.root, "docs/verification/six.log", "    $ ls scripts/gone.sh\n")
        git(self.root, "add", "-A")
        ledger = write(self.root, "ledger.tsv",
                       "scripts/gone.sh\trecorded-absent\tkept so the transcript can "
                       "quote a run whose output was the missing file\n")
        quiet = run(self.root, "--allowlist", str(ledger))
        loud = run(self.root, "--allowlist", str(ledger), "--all")
        self.assertEqual(quiet.returncode, 0, quiet.stderr)
        self.assertEqual(loud.returncode, 1)
        self.assertIn("scripts/gone.sh", loud.stderr)
        self.assertIn("recorded-absent", loud.stderr)

    def test_a_quoted_wrong_command_is_recorded_by_quoting_that_command(self) -> None:
        # Diagnosing a mistake means writing the wrong command down, so rule 1 needs a
        # recording route too. The key is the command itself, so an exemption can never
        # cover a later command that merely looks similar.
        write(self.root, "docs/verification/seven.log",
              "    $ python3 scripts/ci/x.sh    # the SyntaxError this raised is the point\n")
        git(self.root, "add", "-A")
        ledger = write(self.root, "ledger.tsv",
                       "python3 scripts/ci/x.sh\tquoted-command\tthe transcript prints "
                       "this command because the error it raises is the finding itself\n")
        quiet = run(self.root, "--allowlist", str(ledger))
        self.assertEqual(quiet.returncode, 0, quiet.stderr)
        loud = run(self.root, "--allowlist", str(ledger), "--all")
        self.assertEqual(loud.returncode, 1)
        self.assertIn("seven.log:1: `python3 scripts/ci/x.sh` cannot work", loud.stderr)
        self.assertIn("(quoted-command)", loud.stderr)

    def test_a_row_only_excuses_the_rule_its_category_belongs_to(self) -> None:
        # One loose row may not silence both rules: a path row cannot excuse a command an
        # interpreter cannot read, and a quoted command cannot excuse an absent path.
        write(self.root, "docs/verification/eight.log",
              "    $ python3 scripts/ci/x.sh\n    $ ls scripts/gone.sh\n")
        git(self.root, "add", "-A")

        path_row = write(self.root, "paths.tsv",
                         "python3 scripts/ci/x.sh\thistorical\ta path row must not be "
                         "able to excuse a command the interpreter cannot run\n")
        proc = run(self.root, "--allowlist", str(path_row))
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("eight.log:1: `python3 scripts/ci/x.sh` cannot work", proc.stderr)
        self.assertIn("run it with bash (unrecorded)", proc.stderr)
        self.assertIn("stale", proc.stderr)

        quoted_row = write(self.root, "quoted.tsv",
                           "scripts/gone.sh\tquoted-command\ta quoted command row must "
                           "not be able to excuse a path that is simply not there\n")
        proc = run(self.root, "--allowlist", str(quoted_row))
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("`scripts/gone.sh` names a file", proc.stderr)
        self.assertIn("(unrecorded)", proc.stderr)
        self.assertIn("stale", proc.stderr)

    def test_a_quoted_row_that_matches_no_command_left_is_stale(self) -> None:
        ledger = write(self.root, "ledger.tsv",
                       "python3 scripts/ci/gone.sh\tquoted-command\tno command line in "
                       "the tree prints this any more, which is what stale means here\n")
        proc = run(self.root, "--allowlist", str(ledger))
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("stale", proc.stderr)
        self.assertIn("python3 scripts/ci/gone.sh", proc.stderr)

    def test_a_copy_source_is_a_claim_and_its_destination_is_not(self) -> None:
        # `cp a b` reads a and writes b, so a is a claim about the repository and b is not.
        # `ln` is exempt whole: a symlink may name something that does not exist yet.
        write(self.root, "docs/verification/nine.log",
              "    $ cp docs/exists.md docs/copy-dest.md\n"
              "    $ mv docs/gone.md docs/moved.md\n"
              "    $ ln -s docs/never-existed.md docs/link-name.md\n")
        git(self.root, "add", "-A")
        proc = run(self.root)
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("`docs/gone.md` names a file", proc.stderr)
        for not_a_claim in ("docs/copy-dest.md", "docs/moved.md",
                            "docs/never-existed.md", "docs/link-name.md"):
            self.assertNotIn(not_a_claim, proc.stderr)

    # -- the ledger itself ------------------------------------------------------

    def test_a_bad_ledger_line_is_an_error_not_a_silence(self) -> None:
        cases = {
            "unknown category": "x.py\tmade-up\tthe reason is long enough to be a note\n",
            "short reason": "x.py\thistorical\ttoo short\n",
            "two columns": "x.py\thistorical\n",
            "duplicate": ("x.py\thistorical\tthe reason is long enough to be a note\n"
                          "x.py\thistorical\tthe reason is long enough to be a note\n"),
        }
        for label, body in cases.items():
            with self.subTest(label):
                ledger = write(self.root, "ledger.tsv", body)
                proc = run(self.root, "--allowlist", str(ledger))
                self.assertEqual(proc.returncode, 2, f"{label}: {proc.stderr}")
                self.assertIn("bad allowlist", proc.stderr)

    def test_list_prints_transcripts_and_documents_only(self) -> None:
        write(self.root, "docs/verification/notes.txt", "    $ python3 x.sh\n")
        write(self.root, "scripts/ci/other.log", "    $ python3 x.sh\n")
        git(self.root, "add", "-A")
        proc = run(self.root, "--list")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        listed = proc.stdout.split()
        self.assertIn("docs/verification/one.log", listed)
        self.assertIn("docs/exists.md", listed)
        self.assertNotIn("docs/verification/notes.txt", listed)
        self.assertNotIn("scripts/ci/other.log", listed)

    # -- the real tree ----------------------------------------------------------

    def test_the_repository_this_guard_was_written_for_is_clean(self) -> None:
        proc = subprocess.run([sys.executable, str(SCRIPT)],
                              capture_output=True, text=True, cwd=REPO)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        m = re.search(r"(\d+) files, (\d+) command lines, "
                      r"(\d+) interpreter mismatch\(es\), "
                      r"(\d+) dangling path\(s\), (\d+) recorded", proc.stdout)
        self.assertIsNotNone(m, proc.stdout)
        files, lines, mismatch, dangling, recorded = (int(n) for n in m.groups())
        self.assertGreater(files, 100)
        self.assertGreater(lines, 500)
        self.assertGreater(recorded, 0)
        # `mismatch` and `dangling` are raw finding counts: a finding that is recorded is
        # still counted there. So "clean" cannot mean zero findings, and what it has to
        # mean is checked against --all, which prints the recorded ones too.
        loud = subprocess.run([sys.executable, str(SCRIPT), "--all"],
                              capture_output=True, text=True, cwd=REPO)
        self.assertNotIn("(unrecorded)", loud.stdout + loud.stderr)
        keys = set()
        for line in (loud.stdout + loud.stderr).splitlines():
            named = re.search(r":\d+: `(.+?)` (?:cannot work|names a file)", line)
            if named:
                keys.add(named.group(1))
        self.assertEqual(len(keys), mismatch + dangling, loud.stdout + loud.stderr)
        # Every row on the ledger is live, so the ledger is not a pile of old excuses.
        self.assertLessEqual(recorded, len(keys))


if __name__ == "__main__":
    unittest.main(verbosity=2)
