#!/usr/bin/env python3
"""Reject a workflow step that runs a tool its job never installed.

Discovered from the other side: the `docs localization` job has been red on every
push since 2026-10-02, and nothing in this repository was wrong. The step runs
`scripts/l10n-guard.sh`, which begins with `command -v rg || exit 64` because a
Han-counting guard with no Unicode-aware counter is a broken instrument, not a
clean one. The runner image for that job has no ripgrep -- the `rust` job apt-get
installs it, the `platform tests` job downloads a release tarball for it, and
`docs-l10n` installs nothing at all. So the guard refused to run, exactly as it is
supposed to, and the job's only report was `Process completed with exit code 1`.

Two lessons are folded into this file. The first is why the previous page of
`l10n-guard.sh` history matters: before the fail-closed change the same missing
binary produced an *empty* listing, which the guard read as "no Chinese was lost"
-- green. The red we see now is the fix working. The second is that no check in
this repository compared a job's dependencies against a job's steps, so the gap
survived an entire day of pushes and was only visible as a red X with no diff to
point at.

The rule, per job, per tool:

- A step *needs* a tool when one of its `run:` bodies matches the tool's trigger
  patterns -- a script or command that cannot work without it. Comments inside a
  `run:` block do not count: a step that mentions `scripts/l10n-guard.sh` in a
  comment is describing the guard, not running it. That is the same rule
  `check-guard-wiring.py` learned when its own docstring satisfied it.
- A job *provides* a tool when an **earlier** step installs it: `uses:
  actions/setup-node@...`, or a `run:` body whose install command names the
  package. Earlier matters -- `apt-get install ripgrep` on the line after the
  guard ran is still a job that cannot run the guard.
- Needs without provides fails, naming the workflow, the job, the tool, the
  offending step and its line number.

Usage: python3 scripts/ci/check-workflow-toolchain.py [workflow.yml ...]
Exit: 0 = every job that needs a tool installs it first.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

WORKFLOW_DIR = Path(".github/workflows")


def default_workflows() -> list[Path]:
    """Every workflow in `.github/workflows`, rather than a list of names in a file.

    All three workflow gates used to hard-code `ci.yml` and `release.yml`, so a third
    file -- `docker-labs.yml`, added in the same change as this comment -- installed
    nothing and needed nothing as far as any gate was concerned.
    """
    return sorted(WORKFLOW_DIR.glob("*.yml"))


class Tool:
    """One binary, what proves a step needs it, what proves a job installed it."""

    def __init__(
        self,
        name: str,
        needs: list[str],
        provides_run: list[str],
        provides_uses: list[str],
        fix: str,
    ) -> None:
        self.name = name
        self.needs = [re.compile(p) for p in needs]
        self.provides_run = [re.compile(p) for p in provides_run]
        self.provides_uses = [re.compile(p) for p in provides_uses]
        self.fix = fix


# Deliberately short. Every entry here has already bitten once, and each pattern
# is anchored on something that only appears where a command runs -- a bare
# substring would make `grep -q 'npm' package.json` look like a Node install.
TOOLS: list[Tool] = [
    Tool(
        name="ripgrep (rg)",
        needs=[
            r"scripts/l10n-guard\.sh",
            r"scripts/l10n-guard-selftest\.py",
        ],
        provides_run=[
            r"apt-get install[^\n]*\bripgrep\b",
            r"ripgrep/releases/download",
            r"\bbrew install[^\n]*\bripgrep\b",
        ],
        provides_uses=[],
        fix="apt-get install -y --no-install-recommends ripgrep (the guard counts "
        "Han with `rg -o '[\\p{Han}]'` and exits 64 without it)",
    ),
    Tool(
        name="node",
        needs=[
            # Command position only: line start (indent allowed) or after `;`/`&`/`|`.
            # A substring match would call `grep -q 'npm' package.json` a Node program.
            r"(?:^[ \t]*|[;&|][ \t]*)(?:node|npm|npx)(?:\s|$)",
            r"scripts/npm/test-publish-npm\.sh",
        ],
        provides_run=[r"apt-get install[^\n]*\bnodejs\b"],
        provides_uses=[r"actions/setup-node@"],
        fix="uses: actions/setup-node@v4 with the node-version the package.json pins",
    ),
]


def strip_trailing_comment(line: str) -> str:
    """Drop a `# ...` tail, but not a `#` inside quotes (`.../releases/#anchor`)."""
    quote = ""
    for index, char in enumerate(line):
        if quote:
            if char == quote:
                quote = ""
        elif char in "\"'":
            quote = char
        elif char == "#" and (index == 0 or line[index - 1] in " \t"):
            return line[:index]
    return line


class Step:
    def __init__(self, workflow: str, job: str, index: int, line: int) -> None:
        self.workflow = workflow
        self.job = job
        self.index = index
        self.line = line
        self.name: str | None = None
        self.uses: str | None = None
        self.body: list[str] = []

    def describe(self) -> str:
        label = self.name or self.uses or f"step {self.index + 1}"
        return f"{self.workflow}:{self.line}: [{self.job}] {label}"


def parse_steps(text: str, workflow: str) -> list[Step]:
    """Ordered steps of every job, with each `run:` block collected verbatim.

    A second hand-written parser rather than PyYAML, matching
    `check-workflow-shells.py`: these workflows are hand-written and uniform, and
    the gate has to run in a container that is not promised a YAML module.
    """
    steps: list[Step] = []
    job = "top"
    job_indent = 2
    step: Step | None = None
    in_jobs = False
    run_body_indent: int | None = None
    counter = 0

    for lineno, raw in enumerate(text.splitlines(), 1):
        line = raw.rstrip("\n")
        stripped = line.strip()
        if not stripped:
            if run_body_indent is not None and step is not None:
                step.body.append("")
            continue
        indent = len(line) - len(line.lstrip(" "))

        if indent == 0:
            in_jobs = stripped == "jobs:"
            job, step, run_body_indent = "top", None, None
            counter = 0
            continue
        if not in_jobs:
            continue

        # A job key sits at the fixed indent under `jobs:`.
        if indent == job_indent and not stripped.startswith("- ") and stripped.endswith(":"):
            job = stripped[:-1]
            step, run_body_indent = None, None
            counter = 0
            continue

        # Inside a `run: |` block, everything deeper belongs to the script.
        if run_body_indent is not None:
            if step is not None and indent >= run_body_indent:
                step.body.append(line)
                continue
            run_body_indent = None

        if indent == job_indent + 4 and stripped.startswith("- "):
            counter += 1
            step = Step(workflow, job, counter - 1, lineno)
            steps.append(step)
            key = stripped[2:].strip()
        elif step is None:
            continue
        else:
            key = stripped

        match = re.match(r"name:\s*(.*)$", key)
        if match:
            step.name = match.group(1).strip().strip("\"'")
            continue
        match = re.match(r"uses:\s*(.*)$", key)
        if match:
            step.uses = match.group(1).strip().strip("\"'")
            continue
        match = re.match(r"run:\s*(.*)$", key)
        if match:
            value = match.group(1).strip()
            if value in {"|", "|-", "|+", ">", ">-", ">+"}:
                run_body_indent = indent + 2
            elif value:
                step.body.append(value)
            continue

    return steps


def executable_lines(step: Step) -> list[str]:
    """The step's script as executable lines, with comments removed.

    `strip_trailing_comment` already reduces a whole-line comment to ``""`` -- it
    reaches the `#` at the start of the code before any quote can open -- so a
    second, separate comment filter would be a filter nothing needs. Dropping that
    second filter was checked against the fixtures rather than assumed: it leaves
    all 15 green, while dropping the stripping itself turns both comment cases red.
    """
    return [strip_trailing_comment(line) for line in step.body]


def first_match(lines: list[str], patterns: list[re.Pattern[str]]) -> int | None:
    for index, line in enumerate(lines):
        if any(p.search(line) for p in patterns):
            return index
    return None


def check(path: Path, verbose: bool = False) -> list[str]:
    steps = parse_steps(path.read_text(), str(path))
    problems: list[str] = []

    for tool in TOOLS:
        by_job: dict[str, list[Step]] = {}
        for step in steps:
            by_job.setdefault(step.job, []).append(step)

        for job, job_steps in by_job.items():
            for step in job_steps:
                lines = executable_lines(step)
                need_line = first_match(lines, tool.needs)
                if need_line is None:
                    continue
                if verbose:
                    print(f"  need {tool.name:14s} {step.describe()}")

                # `apt-get install ripgrep` two lines above the guard in the same
                # script is a working step; two lines below it is not.
                own_line = first_match(lines, tool.provides_run)
                if own_line is not None and own_line < need_line:
                    continue

                provided = False
                for earlier in job_steps:
                    if earlier.index >= step.index:
                        continue
                    if earlier.uses and any(
                        p.search(earlier.uses) for p in tool.provides_uses
                    ):
                        provided = True
                        break
                    if first_match(executable_lines(earlier), tool.provides_run) is not None:
                        provided = True
                        break
                if provided:
                    continue
                problems.append(
                    f"{step.describe()} needs {tool.name}, which no earlier step in "
                    f"job '{job}' installs -- {tool.fix}"
                )
    return problems


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("workflows", nargs="*", type=Path)
    parser.add_argument("--verbose", action="store_true", help="print every detected need")
    args = parser.parse_args(argv)

    targets = args.workflows or default_workflows()
    if not targets:
        print("check-workflow-toolchain: no workflow files found -- refusing to pass",
              file=sys.stderr)
        return 1

    problems: list[str] = []
    for path in targets:
        if not path.exists():
            problems.append(f"{path}: named workflow does not exist")
            continue
        problems.extend(check(path, verbose=args.verbose))

    for problem in problems:
        print(f"check-workflow-toolchain: {problem}", file=sys.stderr)
    if problems:
        return 1

    print(
        f"check-workflow-toolchain: OK ({len(targets)} workflow file(s), "
        f"{len(TOOLS)} tool rule(s))"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
