#!/usr/bin/env python3
"""Fail when a repository shell script uses a construct that the stock macOS
shell or BSD userland does not provide.

Why this exists: `scripts/test-platform.sh` is the documented one-command entry
point for macOS, and the other `scripts/*.sh` are run by contributors on their
own machines. macOS ships bash 3.2 (2006) plus BSD userland, so `mapfile`,
`${var^^}` and `sed -i` abort there with a confusing "command not found" or a
half-written file, while every Linux developer sees them pass.

There is deliberately no allow list. A rule that fires on an existing script is
fixed in the script, not exempted here; a check with exemptions quietly stops
protecting the thing it names.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

# (rule id, pattern, hint). Patterns match code only; comments are stripped.
RULES: list[tuple[str, str, str]] = [
    ("bash4-mapfile", r"\bmapfile\b", "use `while IFS= read -r line; do arr+=(\"$line\"); done < <(...)`"),
    ("bash4-readarray", r"\breadarray\b", "same as mapfile: read loop"),
    ("bash4-assoc-array", r"\b(?:declare|typeset)\s+-A\b", "associative arrays need bash 4; use a case statement"),
    ("bash4-nameref", r"\b(?:declare|typeset|local)\s+-n\b", "namerefs need bash 4.3"),
    ("bash4-case-fold", r"\$\{[A-Za-z_][A-Za-z0-9_]*(?:\^\^|,,)", "use `tr '[:lower:]' '[:upper:]'` / `tr '[:upper:]' '[:lower:]'`"),
    ("bash4-parameter-transform", r"\$\{[A-Za-z_][A-Za-z0-9_]*@", "parameter transformation/`@Q` need bash 4.4+"),
    ("bash4-epoch", r"\b(?:EPOCHSECONDS|EPOCHREALTIME|BASHPID)\b", "bash 4.2+ only; use `date +%s` / `$$`"),
    ("bash4-globstar", r"\bshopt\s+-s\s+globstar\b", "globstar is bash 4; use `find`"),
    ("bash4-wait-n", r"\bwait\s+-n\b", "bash 4.3+ only"),
    ("gnu-nproc", r"\bnproc\b", "not on macOS; use `getconf _NPROCESSORS_ONLN`"),
    ("gnu-readlink-f", r"\breadlink\s+(?:-f|--canonicalize)\b", "BSD readlink has no -f; resolve with `cd`/`pwd -P`"),
    ("gnu-realpath", r"\brealpath\b", "not on macOS"),
    ("gnu-date-d", r"\bdate\s+-d\b", "BSD date has no -d"),
    ("gnu-stat-c", r"\bstat\s+-(?:c|--format)\b", "BSD stat has no -c/--format"),
    ("gnu-sed-inplace", r"\bsed\s+-i\b(?!\s*(?:''|\"\")\s)", "BSD sed needs an argument to -i; write to a temp file"),
    ("gnu-grep-pcre", r"\bgrep\b[^|;&]*\s-[A-Za-z]*P\b", "BSD grep has no -P; use `grep -E` or rg"),
    ("gnu-grep-include", r"\bgrep\b[^|;&]*--(?:include|exclude-dir)\b", "BSD grep has no --include/--exclude-dir; use rg or find"),
    ("gnu-find-printf", r"\bfind\b[^|;&]*-printf\b", "BSD find has no -printf"),
    ("gnu-xargs-run", r"\bxargs\b[^|;&]*-r\b", "BSD xargs has no -r; guard the empty input yourself"),
    ("gnu-install-d", r"\binstall\s+-D\b", "BSD install has no -D"),
    ("gnu-cp-reflink", r"\bcp\s+--reflink\b", "GNU cp only"),
    ("gnu-base64-wrap", r"\bbase64\b[^|;&]*-w[0-9]*\b", "BSD base64 has no -w"),
    ("gnu-timeout", r"(?:^|[|;&]\s*)timeout\s+\d", "not on macOS (gtimeout via coreutils); use a background pid + kill"),
    ("gnu-tac", r"\btac\b", "BSD uses `tail -r`"),
    ("gnu-wc-L", r"\bwc\s+-L\b", "byte-count line length with awk instead"),
]

COMPILED = [(name, re.compile(pattern), hint) for name, pattern, hint in RULES]

DEFAULT_ROOTS = ("scripts",)
EXTRA_FILES = ("scripts/hooks/pre-commit",)


def strip_comment(line: str) -> str:
    """Drop a trailing `# ...` comment, but not a `#` inside quotes."""
    quote = ""
    for index, char in enumerate(line):
        if quote:
            if char == quote:
                quote = ""
            continue
        if char in "\"'":
            quote = char
        elif char == "#" and (index == 0 or line[index - 1] in " \t"):
            return line[:index]
    return line


def scan(path: Path) -> list[str]:
    findings: list[str] = []
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not raw.strip():
            continue
        code = strip_comment(raw)
        if "portability-check:ignore" in raw:
            continue
        for name, pattern, hint in COMPILED:
            if pattern.search(code):
                findings.append(f"{path}:{number}: {name}: {raw.strip()}\n    fix: {hint}")
    return findings


def candidates(root: Path) -> list[Path]:
    found: list[Path] = []
    for directory in DEFAULT_ROOTS:
        base = root / directory
        if base.is_dir():
            found.extend(sorted(base.rglob("*.sh")))
    for extra in EXTRA_FILES:
        path = root / extra
        if path.is_file():
            found.append(path)
    return [path for path in found if path.is_file()]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default=".", help="repository root to scan")
    args = parser.parse_args()

    root = Path(args.root)
    files = candidates(root)
    if not files:
        print("script-portability: no shell scripts found", file=sys.stderr)
        return 1

    findings: list[str] = []
    for path in files:
        findings.extend(scan(path))

    if findings:
        for line in findings:
            print(line, file=sys.stderr)
        print(f"script-portability: {len(findings)} non-portable construct(s)", file=sys.stderr)
        return 1
    print(f"script-portability: OK ({len(files)} shell script(s))")
    return 0


if __name__ == "__main__":
    sys.exit(main())
