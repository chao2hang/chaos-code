#!/usr/bin/env python3
"""Reject a `run:` step that would execute under pwsh on a Windows runner.

GitHub picks a step's default shell from the runner's OS: `bash` on Linux and
macOS, `pwsh` on Windows. A step that calls `sed`, `test` or `set -euo pipefail`
therefore passes every check we run locally and in CI -- `ci.yml` is
ubuntu-only -- and only breaks in the release build matrix, which is the one
place we cannot afford it.

That is not hypothetical. The first v0.4.0 tag died on it: a `Read pinned
toolchain` step was added without `shell: bash`, so two of six builds failed at
step 5 while Linux and macOS sailed through, and the run's only visible symptom
was a red X on a tag that had already been published.

The rule enforced here: in any job whose `runs-on` can be Windows, every step
that has a `run:` must either pin `shell: bash` or carry an `if:` mentioning
`runner.os` (which is how `Install Linux deps` legitimately opts out). The
matrix form counts too -- `runs-on: ${{ matrix.os }}` is Windows-capable if any
`os:` in that job's matrix names Windows.

Usage: python3 scripts/ci/check-workflow-shells.py [workflow.yml ...]
Exit: 0 = clean, 1 = a step would run under the wrong shell.
"""

import re
import sys
from pathlib import Path

DEFAULT_WORKFLOWS = [
    Path(".github/workflows/ci.yml"),
    Path(".github/workflows/release.yml"),
]


def parse_jobs(text):
    """Minimal indentation walk. Returns (order, jobs).

    Only the handful of keys this check needs are collected, which keeps the
    script dependency-free: these workflows are hand-written and uniform enough
    that YAML semantics beyond nesting do not matter here.
    """
    jobs = {}
    order = []
    job = None
    step = None
    in_jobs = False

    for lineno, raw in enumerate(text.splitlines(), 1):
        line = raw.rstrip()
        body = line.strip()
        if not body or body.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        # A sequence entry's key sits after the dash.
        key = body[2:].strip() if body.startswith("- ") else body

        if indent == 0:
            in_jobs = body == "jobs:"
            job, step = None, None
            continue
        if not in_jobs:
            continue

        if indent == 2 and body.endswith(":"):
            job = body[:-1]
            jobs[job] = {"runs_on": None, "matrix_os": [], "steps": []}
            order.append(job)
            step = None
            continue
        if job is None:
            continue

        # Job-level keys: a step block cannot start at this indent.
        if indent == 4:
            step = None
            match = re.match(r"runs-on:\s*(.*)$", key)
            if match:
                jobs[job]["runs_on"] = match.group(1).strip()
            continue

        # Matrix entries are nested deeper than the job keys.
        match = re.match(r"os:\s*(.*)$", key)
        if match:
            jobs[job]["matrix_os"].append(match.group(1).strip())

        if indent == 6 and body.startswith("- "):
            step = {
                "line": lineno,
                "name": None,
                "has_run": False,
                "shell": None,
                "guard": None,
            }
            jobs[job]["steps"].append(step)

        if step is None:
            continue

        match = re.match(r"name:\s*(.*)$", key)
        if match:
            step["name"] = match.group(1).strip().strip("'\"")
        if re.match(r"run:", key):
            step["has_run"] = True
        match = re.match(r"shell:\s*(.*)$", key)
        if match:
            step["shell"] = match.group(1).strip()
        match = re.match(r"if:\s*(.*)$", key)
        if match:
            step["guard"] = match.group(1).strip()

    return order, jobs


def windows_capable(job):
    runs_on = (job["runs_on"] or "").lower()
    if "windows" in runs_on:
        return True
    if "matrix" in runs_on:
        return any("windows" in os.lower() for os in job["matrix_os"])
    return False


def check(path):
    order, jobs = parse_jobs(path.read_text())
    problems = []
    for name in order:
        job = jobs[name]
        if not windows_capable(job):
            continue
        for step in job["steps"]:
            if not step["has_run"]:
                continue
            if step["shell"] == "bash":
                continue
            if step["guard"] and "runner.os" in step["guard"]:
                continue
            problems.append((name, step))
    return problems


def main(argv):
    paths = [Path(a) for a in argv[1:]] or DEFAULT_WORKFLOWS
    failed = False
    for path in paths:
        if not path.exists():
            print(f"check-workflow-shells: missing workflow: {path}", file=sys.stderr)
            failed = True
            continue
        problems = check(path)
        for job, step in problems:
            label = step["name"] or "(unnamed step)"
            print(
                f"{path}:{step['line']}: job '{job}' can run on Windows, but step "
                f"'{label}' has a `run:` with no `shell: bash` and no "
                f"`if: runner.os ...` guard -- it would execute under pwsh",
                file=sys.stderr,
            )
        if problems:
            failed = True
        else:
            print(f"check-workflow-shells: {path}: OK")
    if failed:
        print(
            "check-workflow-shells: FAIL -- add `shell: bash` to the step, or "
            "guard it with `if: runner.os == '...'`",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
