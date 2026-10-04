#!/usr/bin/env python3
"""Reject a shell assignment that can end the script instead of printing its report.

The shape: in a script running `set -e`, a plain assignment `name="$(pipeline)"` takes its exit
status from the command substitution. If that status is non-zero, the script ends right there. For
a command that genuinely failed, that is correct. It is wrong for a command whose non-zero status
is a normal answer: `grep` exits 1 when nothing matched, `diff` exits 1 when the files differ, `wc`
exits non-zero when the file it was asked to measure is not there. In those cases the script has
already decided what the non-zero answer means -- and the code that says so is unreachable.

Three such sites shipped, all in scripts whose job is to report something; all three exited with
the right code and in all three the words were missing:

- `scripts/verify-in-docker.sh`: `moved="$(diff "$before" "$after" | sed 's/^<name> /  /' | sort -u)"`.
  Files differing is the case that branch exists for, so the run that had just finished printed no
  verdict at all, `UNATTRIBUTABLE` included.
- `scripts/ci/check-versions.sh`: `declared_names="$(grep -v '^$' <<<"$declared" | cut -d' ' -f1)"`.
  An empty `optionalDependencies` is what the check reports, and the gate exited 1 with an empty
  stderr; the comparison that would have named the missing package set sat one statement later.
- `scripts/install.sh`: `size="$(wc -c < "$dest" 2>/dev/null | tr -d '[:space:]')"`. The
  `|| size=0` fallback on the very next line is unreachable, so a download that could not be
  measured killed the mirror loop instead of moving on to the next mirror.

A fourth was reported by an early revision of this rule and is wrong: the same shape in
`scripts/verify-gates.sh` (`lines="$(printf '%s\n' "$list" | grep -c .)"`). That runner sets
`set -uo pipefail` at line 43 and never turns `-e` on, because it aggregates gate failures rather
than dying on the first one, so the assignment's status went nowhere and the `not ok` branch below
it ran. Measured: under `set -uo pipefail` the next statement is reached with `lines=0`; under
`set -euo pipefail` it is not reached and the shell exits 1. That is the whole reason `-e` and not
`pipefail` is what puts a file in scope: `-uo pipefail` on its own produces the non-zero status and
leaves nobody to act on it.

The rule is deliberately narrow, and each exclusion is there for a reason:

- `local`/`export`/`readonly`/`declare`/`typeset` assignments are not matched, because the
  assignment pattern is anchored to the start of the line. Those mask the status (ShellCheck's
  SC2155) rather than propagate it, so the failure there is an empty value used later, not a lost
  report, and nothing on this tree has been observed to break that way.
- Commands outside `HAZARD`. A `sha256sum` over a tunnel that is down or a `uname` the host cannot
  answer should stop the script; the sites like that were read and left alone.
- Text inside single or double quotes, and here-document bodies. A `| grep` or a `|| true` written
  inside a string is content the script prints, not a stage it runs or a status it absorbs, so both
  are masked out before the stage scan.
- `set +e` regions are not modelled. A file is in scope if it turns `-e` on anywhere, which scans a
  little more than strictly necessary; the hazard list is what decides whether a site is reported.

There is deliberately no allow list, following `check-script-portability.py`: a rule that fires is
fixed in the script. The fix is either a command without the extra status (`sed '/^$/d'` instead of
`grep -v '^$'`, `awk 'NF { n += 1 } END { print n + 0 }'` instead of `grep -c .`) or an explicit
absorption (`|| true`) that leaves the handling below reachable.

Usage: python3 scripts/ci/check-pipefail-report.py [--root DIR] [--verbose]
Exit: 0 = no bare assignment in an `errexit` script depends on a normal non-zero status.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

# Commands whose non-zero exit is an answer rather than an error, with the answer each one gives.
HAZARD = {
    "grep": "exits 1 when nothing matched",
    "egrep": "exits 1 when nothing matched",
    "fgrep": "exits 1 when nothing matched",
    "rg": "exits 1 when nothing matched",
    "pgrep": "exits 1 when no process matched",
    "diff": "exits 1 when the files differ",
    "cmp": "exits 1 when the files differ or one is shorter",
    "wc": "exits non-zero when its input cannot be read",
}
HINT = (
    "the non-zero status here is a normal answer, and a plain assignment hands it to `set -e`; use "
    "a command without that status (`sed '/^$/d'`, `awk 'NF { n += 1 } END { print n + 0 }'`) or "
    "absorb it explicitly with `|| true` so the handling below stays reachable"
)

# A plain `name="$(...)"` assignment, anchored so a declaration keyword in front of it does not
# match: `local name="$(...)"` masks the status instead of propagating it.
ASSIGN_RE = re.compile(r'^([ \t]*)([A-Za-z_][A-Za-z0-9_]*)="\$\(', re.MULTILINE)
# In scope means "`set -e` is turned on somewhere in the file", which is what ends the script.
# Per line, because the `set` line is never the first line: shebang and header comments come first.
STRICT_RE = re.compile(r"^[ \t]*set[ \t]+(?:-[A-Za-z]*e[A-Za-z]*|-o[ \t]+errexit)\b", re.MULTILINE)
# A stage in command position: start of the substitution, or after a pipe / sequence / subshell.
STAGE_RE = re.compile(r"(?:^|[|;&(]|\$\()[ \t]*([A-Za-z_][A-Za-z0-9_]*)")
HEREDOC_RE = re.compile(r"<<-?[ \t]*(['\"]?)([A-Za-z_][A-Za-z0-9_]*)\1")

SKIP_DIRS = {".git", "target", "node_modules", "dist", "build", "out", ".venv", "site-packages"}


def strict_line(text: str) -> str | None:
    """The `set` line that puts the file in scope, or None if it never enables `errexit`."""
    match = STRICT_RE.search(text)
    return match.group(0).strip() if match else None


def code_text(text: str) -> str:
    """`text` with here-document bodies and whole-line comments blanked, positions preserved.

    Blanking keeps the newlines, so a line number in the result is the line number the author sees.
    """
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    index = 0
    while index < len(lines):
        line = lines[index]
        index += 1
        if line.lstrip().startswith("#"):
            out.append("\n")
            continue
        match = HEREDOC_RE.search(line)
        out.append(line)
        if not match:
            continue
        marker = match.group(2)
        while index < len(lines):
            body = lines[index]
            index += 1
            out.append("\n")
            if body.strip() == marker:
                break
    return "".join(out)


def walk(text: str, pos: int, stack: list[str], mask: bool, stop_when_empty: bool) -> tuple[str, int]:
    """Consume `text` from `pos` following bash's quoting and `$(...)` nesting.

    `stack` holds the contexts the parser is inside (`sub`, `dq`, `sq`); `stop_when_empty` ends the
    walk once the outermost context closes, which is how the body of one command substitution is
    cut out. With `mask`, characters inside a quoted string are replaced by spaces while everything
    inside a nested `$(...)` is kept, so the result shows the code and hides the prose.

    Closing delimiters are found this way rather than by searching or counting, because neither
    works: `name="$(f)"` ends in a `)"` that a quote-stripping pass deletes, and in
    `ROOT="$(cd "$(dirname "$0")" && pwd)"` the inner `$(` opens a level whose quotes are its own,
    so a counter stops one paren short and a scan runs on to the end of the file.
    """
    out: list[str] = []
    index = pos
    length = len(text)

    def emit(chunk: str, blank: bool = False) -> None:
        out.append(" " * len(chunk) if blank and chunk.strip() else chunk)

    while index < length:
        char = text[index]
        # An empty stack is the top level of a masked body, not a reason to stop.
        top = stack[-1] if stack else ""
        if top == "sq":
            emit(char, blank=mask)
            index += 1
            if char == "'":
                stack.pop()
        elif char == "\\":
            emit(text[index : index + 2])
            index += 2
        elif char == "`" and top != "dq":
            end = text.find("`", index + 1)
            end = length - 1 if end < 0 else end
            emit(text[index : end + 1], blank=mask)
            index = end + 1
        elif char == '"':
            emit(char)
            if top == "dq":
                stack.pop()
            else:
                stack.append("dq")
            index += 1
        elif char == "'" and top != "dq":
            # A `'` inside double quotes is literal text, not the start of a single-quoted span.
            emit(char)
            stack.append("sq")
            index += 1
        elif text.startswith("$((", index):
            end = text.find("))", index + 3)
            end = length - 1 if end < 0 else end + 1
            emit(text[index : end + 1])
            index = end + 1
        elif text.startswith("$(", index):
            emit("$(")
            stack.append("sub")
            index += 2
        elif char == ")":
            index += 1
            if top == "sub":
                stack.pop()
                if stop_when_empty and not stack:
                    break
            # A `)` inside a double-quoted string is text, not a delimiter.
            emit(char)
        else:
            emit(char, blank=mask and top == "dq")
            index += 1
    return "".join(out), index


def substitution_body(text: str, pos: int) -> str:
    """The body of the `$(...)` whose opening paren ends at `pos`; empty if it never closes."""
    body, _ = walk(text, pos, ["sub"], mask=False, stop_when_empty=True)
    return body


def mask_strings(body: str) -> str:
    """`body` with quoted string contents blanked, nested command substitutions left intact."""
    masked, _ = walk(body, 0, [], mask=True, stop_when_empty=False)
    return masked


def hazard_stages(text: str) -> list[str]:
    """Hazard command names appearing in command position within the substitution."""
    found: list[str] = []
    for line in mask_strings(text).splitlines():
        for match in STAGE_RE.finditer(line):
            name = match.group(1)
            if name in HAZARD and name not in found:
                found.append(name)
    return found


def scan_text(text: str) -> list[tuple[int, str, list[str]]]:
    """(line number, source line, hazard names) for each assignment that can end the script."""
    findings: list[tuple[int, str, list[str]]] = []
    code = code_text(text)
    for match in ASSIGN_RE.finditer(code):
        # ASSIGN_RE ends just past the `$(`, so the substitution body starts there.
        body = substitution_body(code, match.end())
        if "||" in mask_strings(body):
            continue
        hazards = hazard_stages(body)
        if hazards:
            start = code.rfind("\n", 0, match.start()) + 1
            end = code.find("\n", match.start())
            line = code.count("\n", 0, match.start()) + 1
            findings.append((line, code[start : len(code) if end < 0 else end].strip(), hazards))
    return findings


def scan(path: Path, shown: Path) -> list[str]:
    """Findings for one file, labelled `shown`, which is the path as the reader should see it."""
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return []
    if not strict_line(text):
        return []
    return [
        f"{shown}:{line}: {source}\n"
        f"    {', '.join(sorted(hazards))}: "
        + "; ".join(sorted({HAZARD[name] for name in hazards}))
        + f"\n    fix: {HINT}"
        for line, source, hazards in scan_text(text)
    ]


def candidates(root: Path) -> list[Path]:
    """Every shell script under `root`, absolute, in a stable order.

    Absolute, so the same path shape is reported whether `--root` was given as `.` or as a build
    directory; the display form is computed from that in `main`.
    """
    base = root.resolve()
    found: list[Path] = []
    for path in sorted(base.rglob("*.sh")):
        if SKIP_DIRS.intersection(path.parts):
            continue
        if path.is_file():
            found.append(path)
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description="Scan shell scripts for the assignment shape")
    parser.add_argument("--root", default=".", help="repository root to scan")
    parser.add_argument("--verbose", action="store_true", help="also list the files in scope")
    args = parser.parse_args()

    base = Path(args.root).resolve()
    files = candidates(base)
    if not files:
        print(f"pipefail-report: no shell scripts under {args.root}", file=sys.stderr)
        return 1

    findings: list[str] = []
    strict = 0
    for path in files:
        shown = path.relative_to(base)
        turned_on = strict_line(path.read_text(encoding="utf-8", errors="replace"))
        if turned_on:
            strict += 1
            if args.verbose:
                print(f"pipefail-report: {shown} ({turned_on})", file=sys.stderr)
        findings.extend(scan(path, shown))

    if findings:
        for line in findings:
            print(line, file=sys.stderr)
        print(
            f"pipefail-report: {len(findings)} assignment(s) that can end an errexit script "
            f"before it prints its report",
            file=sys.stderr,
        )
        return 1
    print(
        f"pipefail-report: OK ({len(files)} shell script(s), {strict} with `set -e` scanned, "
        f"0 hazard assignment(s))"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
