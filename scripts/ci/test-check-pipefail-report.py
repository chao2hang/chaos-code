#!/usr/bin/env python3
"""Fixtures for `scripts/ci/check-pipefail-report.py`.

The gate rejects `name="$(pipeline)"` in a `set -e` script when the pipeline starts a command whose
non-zero exit is a normal answer (`grep` finding nothing, `diff` finding a difference, `wc` being
handed a file that is not there). Three such sites shipped on 2026-10-04, in
`scripts/verify-in-docker.sh`, `scripts/ci/check-versions.sh` and `scripts/install.sh`. In all three
the exit code was already right and the *report* was what got lost: the script ended at the
assignment, one statement before the code that explained the failure.

So these fixtures care about two failure directions, and a case is only worth having if it pins one:

- the gate must flag the shape, with the file, the line, the command and what to do about it;
- the gate must not flag the near-misses, because a gate that cries wolf on `ROOT="$(cd "$(dirname
  "$0")" && pwd)"` (two of the 23 in-scope scripts on this tree) gets silenced within a week.

The entries of `MEASURED` are the shipped lines copied out of the pre-fix files character for
character, so the defect that was actually found stays a test case after the fix to the script that
contained it. `OUT_OF_SCOPE_LINE` is the one finding this rule got wrong on the way in, and it is
here so that mistake cannot come back as a "fix" either.

    python3 scripts/ci/test-check-pipefail-report.py
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
GATE = HERE / "check-pipefail-report.py"

# Two lines: the prelude every in-scope script in this repository has.
HEADER = "#!/usr/bin/env bash\nset -euo pipefail\n"

# The three shipped lines that ended a script before its report, copied out of the pre-fix files
# character for character, so the defect that was actually found stays a test case after the fix to
# the script that contained it.
MEASURED = [
    (
        "scripts/ci/check-versions.sh (before the fix)",
        "declared_names=\"$(grep -v '^$' <<<\"$declared\" | cut -d' ' -f1 | sort)\"",
        "grep",
    ),
    (
        "scripts/verify-in-docker.sh (before the fix)",
        "  moved=\"$(diff \"${tree_before}\" \"${tree_after}\""
        " | sed -n 's/^[<>] [0-9][0-9]* [0-9][0-9]* //p' | sort -u)\"",
        "diff",
    ),
    (
        "scripts/install.sh (before the fix)",
        "      size=\"$(wc -c < \"$dest\" 2>/dev/null | tr -d '[:space:]')\"",
        "wc",
    ),
]

# The same shape in scripts/verify-gates.sh, which is where an early revision of the rule reported a
# finding it should not have. That runner is `set -uo pipefail` and never turns `-e` on, on purpose:
# it aggregates gate failures instead of dying on the first. Measured both ways, the statement after
# the assignment is reached with `lines=0` under `-uo pipefail` and unreachable under `-euo`.
OUT_OF_SCOPE_LINE = '  lines="$(printf \'%s\\n\' "${real_list}" | grep -c .)"'


def run_gate(root: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(GATE), "--root", str(root)],
        cwd=REPO,
        capture_output=True,
        text=True,
    )


def strict_script(body: str) -> str:
    """`body` wrapped in the prelude every scanned script on this tree carries."""
    return HEADER + body + "\n"


@unittest.skipIf(os.name == "nt", "the scripts under test are bash scripts")
class PipefailReportTests(unittest.TestCase):
    """The gate is driven as a subprocess over a generated tree; the repository is read-only."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tree = Path(self._tmp.name) / "repo"
        self.tree.mkdir()

    def write(self, body: str, rel: str = "scripts/probe.sh", header: str = HEADER) -> Path:
        script = self.tree / rel
        script.parent.mkdir(parents=True, exist_ok=True)
        script.write_text(header + body + "\n", encoding="utf-8")
        return script

    def assert_flags(self, *fragments: str) -> subprocess.CompletedProcess:
        """Failed, and named the site and the fix. An exit 1 with nothing to read is the defect."""
        proc = run_gate(self.tree)
        self.assertNotEqual(proc.returncode, 0, f"expected a finding\nstdout: {proc.stdout}")
        self.assertTrue(
            proc.stderr.strip(),
            f"gate failed without naming the site\nstdout: {proc.stdout}\nstderr: {proc.stderr}",
        )
        for fragment in fragments:
            self.assertIn(fragment, proc.stderr)
        return proc

    def assert_clean(self) -> subprocess.CompletedProcess:
        proc = run_gate(self.tree)
        self.assertEqual(proc.returncode, 0, f"stderr: {proc.stderr}\nstdout: {proc.stdout}")
        self.assertIn("0 hazard assignment(s)", proc.stdout)
        return proc

    # -- the shape has to be caught -------------------------------------------------------

    def test_measured_lines_are_all_flagged(self):
        # One script per line, so a finding names the script it came from. The source lands on
        # line 3, under the two-line prelude.
        for origin, source, hazard in MEASURED:
            with self.subTest(origin=origin):
                self.write(source)
                self.assert_flags("scripts/probe.sh:3", hazard, "fix:")

    def test_every_site_in_a_file_is_reported(self):
        # All three measured lines in one script: the gate reports every site, not the first one it
        # meets. A gate that stopped at the first finding would let the rest of a file rot, and the
        # count in the summary line is what the author reads before scrolling the list.
        self.write("\n".join(source for _, source, _ in MEASURED))
        proc = run_gate(self.tree)
        self.assertNotEqual(proc.returncode, 0)
        sites = re.findall(r"^scripts/probe\.sh:(\d+): ", proc.stderr, re.MULTILINE)
        self.assertEqual(sites, ["3", "4", "5"], f"stderr: {proc.stderr}")
        self.assertIn("3 assignment(s) that can end an errexit script", proc.stderr)

    def test_finding_names_the_answer_and_the_remedy(self):
        # The author has to be able to fix it from the message alone; there is no allow list to add.
        self.write('names="$(grep -c x <<<"$in")"')
        self.assert_flags(
            "grep: exits 1 when nothing matched",
            "sed '/^$/d'",
            "|| true",
        )

    def test_multiline_assignment_reported_at_its_first_line(self):
        body = 'versions="$(\n  for pkg in "${PKGS[@]}"; do\n    grep \'"version"\' "$pkg"\n  done\n)"'
        self.write(body)
        self.assert_flags("scripts/probe.sh:3", "versions=\"$(")

    def test_hazard_after_a_nested_substitution_is_still_seen(self):
        # The other half of the nesting problem: a scanner that closes the outer `$(...)` at the
        # first `)` it meets ends up with a body of `sed -n '1p' "$(head -1 "$f"`, which holds no
        # hazard at all, so the `grep` that ends this pipeline is never looked at.
        self.write('out="$(sed -n \'1p\' "$(head -1 "$f")" | grep -c x)"')
        self.assert_flags("grep: exits 1 when nothing matched")

    def test_paren_inside_a_quoted_string_does_not_end_the_body(self):
        # The converse of the nesting case: `)` inside a quoted string is text, so the body runs to
        # the delimiter that closes the substitution rather than to the first `)` of any kind. Cut
        # at the parenthesis and the body stops before the pipeline that carries the hazard.
        self.write('count="$(printf "found: %s (all)\\n" "$hit" | grep -c x)"')
        self.assert_flags("grep: exits 1 when nothing matched")

    # -- the near misses must stay quiet --------------------------------------------------

    def test_absorbed_status_is_accepted(self):
        # The fix the gate asks for, both spellings.
        self.write(
            'count="$(grep -c x <<<"$in" || true)"\n'
            'count2="$(grep -c x <<<"$in" || echo 0)"\n'
        )
        self.assert_clean()

    def test_declaration_keyword_is_out_of_scope(self):
        # `local`/`export` mask the status (SC2155): the value comes back empty, the script does not
        # stop, so this gate's defect is not reachable from here.
        self.write(
            'f() { local names="$(grep -v x <<<"$in")"; echo "$names"; }\n'
            'export names="$(grep -v x <<<"$in")"\n'
            'readonly names2="$(grep -v x <<<"$in")"\n'
            'declare names3="$(grep -v x <<<"$in")"\n'
        )
        self.assert_clean()

    def test_script_without_errexit_is_out_of_scope(self):
        # The exact line and the exact prelude from scripts/verify-gates.sh, which is `set -uo
        # pipefail` and never turns `-e` on: the non-zero status is produced there and goes nowhere.
        # Same line with `-e` added is a finding, so this cannot pass by the rule going blind.
        self.write(OUT_OF_SCOPE_LINE, header="#!/usr/bin/env bash\nset -uo pipefail\n")
        self.assert_clean()
        self.write(OUT_OF_SCOPE_LINE)
        self.assert_flags("grep: exits 1 when nothing matched")

    def test_scope_follows_the_shells_own_errexit(self):
        # The boundary this gate draws is the shell's, not a regex preference. If `STRICT_RE` were
        # widened to `-uo pipefail`, every false positive of the kind OUT_OF_SCOPE_LINE records
        # would come back dressed as a finding; this pins which of the two preludes can end a script
        # by running the assignment rather than by reasoning about it.
        probe = 'lines="$(printf \'%s\\n\' "" | grep -c .)"; echo "reached lines=${lines}"'
        for options, reaches in (("-uo pipefail", True), ("-euo pipefail", False)):
            with self.subTest(set=options):
                proc = subprocess.run(
                    ["bash", "-c", f"set {options}\n{probe}"],
                    capture_output=True,
                    text=True,
                )
                self.assertEqual(
                    "reached" in proc.stdout,
                    reaches,
                    f"set {options}: stdout={proc.stdout!r} stderr={proc.stderr!r}",
                )
                if reaches:
                    self.assertIn("lines=0", proc.stdout)

    def test_non_hazard_pipeline_is_accepted(self):
        # A stage that can only fail for real. `uname` the host cannot answer should stop the
        # script, and that is what the assignment does.
        self.write(
            'sum="$(sha256sum "$f" | cut -d" " -f1)"\n'
            'sorted="$(printf "%s\\n" "$listing" | sort -u)"\n'
            'trip="$(uname -s)/$(uname -m)"\n'
        )
        self.assert_clean()

    def test_nested_quoting_does_not_leak_past_the_assignment(self):
        # The idiom that a paren-counting scanner gets wrong: the inner `$(` opens a level whose
        # quotes are its own, so a naive scan reads the rest of the file as one body. Only the
        # `diff` line is a hazard; if the first line's body ran on, the gate would report both.
        self.write(
            'root="$(cd "$(dirname "$0")/.." && pwd)"\n'
            'cache="${root}/target"\n'
            'moved="$(diff "$a" "$b" | sort -u)"\n'
        )
        proc = self.assert_flags("probe.sh:5", "diff")
        sites = re.findall(r"^scripts/probe\.sh:\d+: ", proc.stderr, re.MULTILINE)
        self.assertEqual(len(sites), 1, f"expected one finding\nstderr: {proc.stderr}")

    def test_command_names_inside_strings_are_not_stages(self):
        # A `| grep` inside a message is text the script prints, not a stage it runs.
        self.write(
            "banner=\"$(echo 'one | grep two')\"\n"
            "hint=\"$(printf 'run: rg --files\\n')\"\n"
        )
        self.assert_clean()

    def test_absorption_inside_a_string_does_not_count(self):
        # The converse: a `||` written inside a message must not be mistaken for absorption,
        # or the gate goes blind exactly where a script prints instructions.
        self.write('note="$(printf \'a || b\\n\' | grep b)"')
        self.assert_flags("grep: exits 1 when nothing matched")

    def test_here_document_body_is_not_scanned(self):
        # Text the script writes out, not text it runs. Paired with the same line at top level so
        # this cannot pass by failing to see either one.
        self.write(
            "cat > /tmp/generated.sh <<'GEN'\n"
            'gone="$(grep -v \'^$\' <<<"$x")"\n'
            "GEN\n"
        )
        self.assert_clean()
        self.write('seen="$(grep -v \'^$\' <<<"$x")"')
        self.assert_flags("grep: exits 1 when nothing matched")

    # -- the gate's own footing -----------------------------------------------------------

    def test_repository_is_clean_and_actually_scanned(self):
        # Anti-vacuity: an earlier revision of the gate anchored its `set -e` pattern to the first
        # line of each file, so every script looked out of scope and it printed OK having compared
        # nothing. The in-scope count in the summary line is what makes `OK` mean something.
        proc = run_gate(REPO)
        self.assertEqual(proc.returncode, 0, f"stderr: {proc.stderr}")
        scanned = int(re.search(r"(\d+) with `set -e` scanned", proc.stdout).group(1))
        self.assertGreaterEqual(scanned, 5, f"only {scanned} scripts in scope: {proc.stdout}")

    def test_tree_without_shell_scripts_is_an_error(self):
        # A --root that matches nothing must not read as a pass; that is how a moved gate goes
        # silently green in CI.
        (self.tree / "readme.md").write_text("nothing here\n", encoding="utf-8")
        proc = run_gate(self.tree)
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("no shell scripts", proc.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
