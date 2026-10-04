#!/usr/bin/env python3
"""Reject a Docker acceptance lab that nothing runs.

`scripts/` ships six files ending in `-in-docker.sh`: five acceptance labs that build a real
binary or download a real artifact and interrogate it inside a container that has never seen this
repository, and `verify-in-docker.sh`, the container entry point that owns the gate list. The rule
does not try to tell them apart by what they are, because that is not decidable from a name; it
asks one question of each, and each is worth exactly one thing: the transcript it printed the last
time something ran it.

Four of the six were named by nothing -- every acceptance lab except
`install-integrity-in-docker.sh`. `check-guard-wiring.py` accounts for `scripts/ci/`, so a
lab one directory up is invisible to every gate in the repository: on 2026-10-04 the only
labs a workflow named were `install-integrity-in-docker.sh` and the entry point itself, and
the newest transcripts of `install-sh-in-docker.sh`,
`npm-install-in-docker.sh`, `remote-acceptance-in-docker.sh` and
`web-deployment-in-docker.sh` were two days old, 48 to 70 commits behind the tree, and their
verdicts described code that had since changed. Nothing
would have noticed if a change had broken one, because the only reader of their verdicts is a
person who happens to remember to run them.

The rule is coverage, with a date, because the two ways a lab goes unrunned need different
answers:

- A lab CI can run has to be run by it. "Can run" is measured rather than believed: the lab
  must be named on an executable line of some `.github/workflows/*.yml`. Comments do not
  count, so a workflow that mentions a lab while explaining why it is not wired fails here.
- A lab whose verdict is about something other than the commit under test cannot be a merge
  gate at all. It is recorded in `scripts/ci/docker-labs.tsv` with a category, the transcript
  it produced, the date and commit it ran at, and what CI cannot supply. That row is not an
  exemption from being run: its date has to be within `STALE_AFTER_DAYS`, or the gate fails and
  names the command to re-run. A documented-but-never-run lab therefore turns red on a schedule
  instead of quietly becoming archaeology.

A row rots in several directions and every one of them is an error: the script is gone; the
script is now run by CI, making the row redundant; the same script has two rows; the category is
not one of the ones that mean "CI cannot judge this commit"; the transcript is not tracked, and
an untracked log is not evidence for a commit; the date does not parse, is in the future, or is
older than the budget; the sha is not a sha; the verdict is neither `green` nor `red`.
A shallow checkout, which is what CI gets, cannot resolve a sha older than its window, so
resolution is skipped there and the shape check stays.

Everything is asked of git rather than of the filesystem, including which labs exist: the answer
has to be a property of the commit, not of the tree somebody happens to be standing in. `--list`
prints the classification, one lab per line.

Usage: python3 scripts/ci/check-lab-coverage.py [--root DIR] [--list]
Exit: 0 = every lab is run by CI or carries a current ledger row.
"""

from __future__ import annotations

import argparse
import datetime as dt
import re
import subprocess
import sys
from pathlib import Path

LEDGER = "scripts/ci/docker-labs.tsv"
COLUMNS = ("script", "category", "transcript", "last_run", "last_sha", "verdict", "reason")
# A dated run is only half the fact; the other half is what it said. `red` is allowed, because a
# lab can be right about a defect CI cannot fix, but it has to be declared rather than implied.
VERDICTS = ("green", "red")
LAB_DIR = "scripts"
# Any depth under `scripts/`, so a lab cannot be filed one directory down to escape the rule.
LAB_SUFFIX = "-in-docker.sh"
WORKFLOW_DIR = ".github/workflows"
SHA_RE = re.compile(r"[0-9a-f]{7,40}")
LAB_NAME_RE = re.compile(r"[A-Za-z0-9_.-]+-in-docker\.sh")
LAB_PATH_RE = re.compile(r"(scripts/[A-Za-z0-9_.\-/]*-in-docker\.sh)")
# The only reasons a lab may be excused from CI: the verdict is about something other than the
# commit under test. Anything else means the lab is excused for a reason unrelated to what CI can
# do, which is how a coverage ledger turns into a wishlist.
CATEGORIES = {
    "consumes-published-artifact": "the lab installs a released artifact, so its verdict "
                                   "describes that artifact rather than the commit under test",
    "needs-external-service": "the lab asks a third-party service a question, so a red there "
                              "says nothing about this repository",
    "already-done-in-ci": "CI runs the same commands natively in another job, so a container leg "
                          "in CI would be measuring the image rather than the change",
}
STALE_AFTER_DAYS = 30


def git(root: Path, args: list[str]) -> str:
    proc = subprocess.run(["git", "-C", str(root), *args], capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {proc.stderr.strip()}")
    return proc.stdout


class Repo:
    """The repository's own view of its content, asked of git rather than of the filesystem.

    A transcript that exists only on the machine that produced it is not evidence for a commit,
    and a lab that exists only there is not a lab the repository has. Answering from the
    filesystem would make this gate green here and red in CI, or the other way round, which is
    the bug class `check-evidence-commands.py` records against itself.
    """

    def __init__(self, root: Path) -> None:
        self.root = root
        self.files = {f for f in git(root, ["ls-files", "-z"]).split("\0") if f}
        self.shallow = git(root, ["rev-parse", "--is-shallow-repository"]).strip() == "true"

    def tracked(self, rel: str) -> bool:
        return rel in self.files

    def matching(self, directory: str, name_re: re.Pattern[str]) -> list[str]:
        """Tracked paths under `directory`, at any depth, whose file name matches `name_re`.

        Depth is included deliberately: a lab one subdirectory down is the same liability, and
        matching only `scripts/` itself would hand anyone a hiding place.
        """
        prefix = directory + "/"
        return sorted(rel for rel in self.files
                      if rel.startswith(prefix) and name_re.fullmatch(rel.rpartition("/")[2]))

    def labs(self) -> list[str]:
        return self.matching(LAB_DIR, LAB_NAME_RE)

    def workflows(self) -> list[str]:
        return self.matching(WORKFLOW_DIR, re.compile(r"[^/]*\.yml"))

    def read(self, rel: str, problems: list[str]) -> str | None:
        path = self.root / rel
        if not path.is_file():
            problems.append(f"{rel}: tracked, but not on disk, so the working tree is mid-edit")
            return None
        return path.read_text(encoding="utf-8")

    def is_commit(self, sha: str) -> bool:
        if not SHA_RE.fullmatch(sha):
            return False
        if self.shallow:
            # actions/checkout fetches one commit, so a genuine older sha cannot be resolved here.
            return True
        probe = subprocess.run(["git", "-C", str(self.root), "cat-file", "-e", f"{sha}^{{commit}}"],
                               capture_output=True, text=True)
        return probe.returncode == 0


def executable_text(text: str) -> str:
    """The file with prose removed: whole-line comments, and trailing comments on a step line.

    Only a whole-line comment can fake an invocation, and that is the case that has to be
    refused. A trailing `#` is cut too, because a commented-out command at the end of a line is
    exactly as dead as one on its own line.
    """
    kept = []
    for line in text.splitlines():
        if line.lstrip().startswith("#"):
            continue
        cut = re.search(r"(?<=\s)#", line)
        kept.append(line[: cut.start()] if cut else line)
    return "\n".join(kept)


def labs_run_by_ci(repo: Repo, labs: set[str], problems: list[str]) -> dict[str, str]:
    """Which labs an executable workflow line names, keyed by lab, valued by the workflow."""
    runners: dict[str, str] = {}
    for workflow in repo.workflows():
        text = repo.read(workflow, problems)
        if text is None:
            continue
        for line in executable_text(text).splitlines():
            for rel in LAB_PATH_RE.findall(line):
                if rel in labs:
                    runners.setdefault(rel, workflow)
                elif not repo.tracked(rel):
                    problems.append(f"{workflow}: names {rel}, which the repository does not "
                                    f"carry")
    return runners


def parse_ledger(repo: Repo, problems: list[str]) -> dict[str, dict[str, str]]:
    if not repo.tracked(LEDGER):
        if (repo.root / LEDGER).is_file():
            problems.append(f"{LEDGER}: rows are read from a file the repository does not track; "
                            f"an untracked ledger excuses nothing")
        return {}
    text = repo.read(LEDGER, problems)
    if text is None:
        return {}
    rows: dict[str, dict[str, str]] = {}
    header_seen = False
    for number, raw in enumerate(text.splitlines(), 1):
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        fields = raw.split("\t")
        if not header_seen:
            header_seen = True
            if tuple(fields) != COLUMNS:
                problems.append(f"{LEDGER}:{number}: the header must be the tab-separated columns "
                                f"{'|'.join(COLUMNS)}, found {'|'.join(fields)}")
                return rows
            continue
        where = f"{LEDGER}:{number}"
        if len(fields) != len(COLUMNS):
            problems.append(f"{where}: {len(fields)} field(s), expected {len(COLUMNS)} "
                            f"({'|'.join(COLUMNS)})")
            continue
        row = dict(zip(COLUMNS, fields))
        script = row["script"].strip()
        if script in rows:
            problems.append(f"{where}: {script} already has a row at line {rows[script]['_line']}, "
                            f"and only one row can be current")
            continue
        row["_line"] = str(number)
        rows[script] = row
    return rows


def check_row(row: dict[str, str], repo: Repo, today: dt.date, problems: list[str]) -> None:
    lab = row["script"].strip()
    where = f"{LEDGER}:{row['_line']}"
    if not repo.tracked(lab):
        problems.append(f"{where}: {lab} is not tracked, so the row vouches for a script the "
                        f"repository does not carry")
    if row["category"] not in CATEGORIES:
        problems.append(f"{where}: category {row['category']!r} is not one of "
                        f"{', '.join(sorted(CATEGORIES))}; the ledger is only for a lab whose "
                        f"verdict cannot be about this commit")
    transcript = row["transcript"].strip()
    if not transcript.startswith("docs/verification/"):
        problems.append(f"{where}: transcript {transcript!r} is not under docs/verification/, "
                        f"which is where a lab leaves its evidence")
    elif not repo.tracked(transcript):
        problems.append(f"{where}: transcript {transcript} is not tracked; a log that exists only "
                        f"on one machine is not evidence for a commit")
    if row["verdict"] not in VERDICTS:
        problems.append(f"{where}: verdict {row['verdict']!r} is not one of "
                        f"{' or '.join(VERDICTS)}; say what the run reported")
    if not row["reason"].strip():
        problems.append(f"{where}: reason is empty; say what CI cannot supply")
    if not repo.is_commit(row["last_sha"].strip()):
        problems.append(f"{where}: last_sha {row['last_sha']!r} is not a commit in this "
                        f"repository (7-40 hex characters)")
    try:
        ran = dt.date.fromisoformat(row["last_run"].strip())
    except ValueError:
        problems.append(f"{where}: last_run {row['last_run']!r} is not an ISO date (YYYY-MM-DD)")
        return
    if ran > today:
        problems.append(f"{where}: last_run {ran.isoformat()} is in the future")
        return
    age = (today - ran).days
    if age > STALE_AFTER_DAYS:
        problems.append(f"{where}: {lab} last ran on {ran.isoformat()}, {age} days ago, past the "
                        f"{STALE_AFTER_DAYS}-day budget; re-run it and update the row: "
                        f"bash {lab} 2>&1 | tee docs/verification/<dated log>")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=".", type=Path)
    parser.add_argument("--list", action="store_true", help="print one line per lab and exit")
    ns = parser.parse_args(argv)
    root: Path = ns.root.resolve()

    try:
        repo = Repo(root)
    except RuntimeError as exc:
        print(f"check-lab-coverage: {exc}; run this from the repository root", file=sys.stderr)
        return 2

    labs = repo.labs()
    if not labs:
        print("check-lab-coverage: no tracked scripts/*-in-docker.sh, nothing to account for")
        return 0

    problems: list[str] = []
    runners = labs_run_by_ci(repo, set(labs), problems)
    rows = parse_ledger(repo, problems)
    for script in sorted(rows):
        if script in runners:
            problems.append(f"{LEDGER}:{rows[script]['_line']}: {script} is run by "
                            f"{runners[script]}, so the row is redundant; delete it")
        else:
            check_row(rows[script], repo, dt.date.today(), problems)
    for lab in labs:
        if lab not in runners and lab not in rows:
            problems.append(f"{lab}: no workflow runs it and {LEDGER} carries no row for it; "
                            f"wire it into a job, or record what CI cannot supply")

    shallow_note = ""
    if repo.shallow:
        shallow_note = ", last_sha shape-checked only (shallow checkout)"
    if ns.list:
        for lab in labs:
            if lab in runners:
                print(f"{lab}\trun by CI\t{runners[lab]}")
            elif lab in rows:
                print(f"{lab}\tledgered\t{rows[lab]['last_run']}\t{rows[lab]['last_sha']}\t"
                      f"{rows[lab]['verdict']}\t{rows[lab]['category']}")
            else:
                print(f"{lab}\tunaccounted")
        return 0

    if problems:
        print(f"check-lab-coverage: FAIL ({len(problems)} problem(s))")
        for problem in problems:
            print(f"  {problem}")
        return 1
    ledgered = [lab for lab in labs if lab in rows]
    print(f"check-lab-coverage: OK ({len(labs)} lab(s): {len(runners)} run by CI, "
          f"{len(ledgered)} with a current ledger row{shallow_note})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
