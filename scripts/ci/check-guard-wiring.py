#!/usr/bin/env python3
"""Reject a CI guard that nothing runs.

Every checker in `scripts/ci/` exists because something broke and somebody wrote
the check. Three of them were found doing nothing at all -- `classify-open-todos.py`,
its fixtures `test-classify-open-todos.py`, and `test-brand-protocol.py` -- and the
drift that hid was real: the TODO status document sat 13 rows off its own source of
truth (149 claimed against 136 real) while describing itself as the snapshot. A green
suite proves nothing about a file nobody invoked.

The rule is reachability, not a list of names:

- The roots are `.github/workflows/*.yml` and `scripts/verify-in-docker.sh`.
- A gate is any `*.py`, `*.sh` or `*.mjs` directly in `scripts/ci/`.
- A file counts as run when a root or an already-run file under `scripts/` names
  it on an executable line. The transitive hop is what lets a lab helper such as
  `release-integrity-serve.py` count: CI runs `install-integrity-in-docker.sh`,
  which starts the server.
- Prose never counts, in either direction. `install.sh` mentions
  `test-installer-signature-policy.py` in a header comment, and this file's own
  docstring names four gates it explains; if either counted, the check would be
  satisfied by commentary -- which is how it first reported OK while
  `test-brand-protocol.py` was wired to nothing. So `#` lines, trailing `#`
  comments and Python docstrings are all removed before matching.
- Docs are not scanned at all. `ignored-tests.sh` is quoted in two audit reports
  and executed by nothing.
- Every exemption has to exempt a file that still exists, so the list cannot rot
  into a graveyard of retired gates.

The other direction is checked too: a `scripts/...` path a workflow or
`verify-in-docker.sh` names must exist, so a renamed guard whose call site was
left behind fails here instead of as a "No such file or directory" deep in a job.

Usage: python3 scripts/ci/check-guard-wiring.py [--root DIR]
Exit: 0 = every gate reachable and no dangling call site, 1 = otherwise.
"""

from __future__ import annotations

import argparse
import ast
import io
import re
import sys
import tokenize
from pathlib import Path

GATE_SUFFIXES = frozenset({".py", ".sh", ".mjs"})
GATE_DIR = "scripts/ci"
VERIFY_SCRIPT = "scripts/verify-in-docker.sh"
WORKFLOW_DIR = ".github/workflows"

# Files in scripts/ci/ deliberately not run by CI, each with why. An entry whose
# file no longer exists is itself a failure.
EXEMPT: dict[str, str] = {
    "ignored-tests.sh": "`exec` alias for ignored-tests.py, which is wired; the docs quote the alias",
}

SCRIPT_PATH_RE = re.compile(r"(scripts/[A-Za-z0-9_.\-/]*\.(?:py|sh|mjs|ps1))")


def executable_lines(path: Path) -> str:
    """The file's text minus everything a reader, not the interpreter, consumes.

    YAML/shell/PowerShell prose is a line starting with `#`. Python needs more:
    a bare string statement is a docstring, and `#` may also trail code. Both
    matter here. `check-guard-wiring.py`'s own docstring names four gates it
    explains, so without the docstring rule the checker certifies itself green
    out of its own commentary -- which is the precise failure this script was
    written to catch, and how it first passed while `test-brand-protocol.py`
    was wired to nothing.
    """
    text = path.read_text(encoding="utf-8")
    if path.suffix == ".py":
        return _python_code(text)
    return "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("#"))


def _python_code(text: str) -> str:
    """Python source with docstrings and comments removed, line numbers preserved."""
    prose_lines: set[int] = set()
    try:
        tree = ast.parse(text)
    except SyntaxError:
        tree = None
    if tree is not None:
        for node in ast.walk(tree):
            if isinstance(node, ast.Expr) and isinstance(node.value, ast.Constant) and isinstance(node.value.value, str):
                prose_lines.update(range(node.lineno, (node.end_lineno or node.lineno) + 1))

    comment_at: dict[int, int] = {}
    try:
        tokens = list(tokenize.generate_tokens(io.StringIO(text).readline))
    except (tokenize.TokenError, IndentationError, SyntaxError):
        tokens = []
    for token in tokens:
        if token.type == tokenize.COMMENT:
            comment_at.setdefault(token.start[0], token.start[1])

    kept: list[str] = []
    for number, line in enumerate(text.splitlines(), 1):
        if number in prose_lines or line.lstrip().startswith("#"):
            continue
        if number in comment_at:
            line = line[: comment_at[number]]
        kept.append(line)
    return "\n".join(kept)


def scripts_by_name(root: Path) -> dict[str, list[Path]]:
    by_name: dict[str, list[Path]] = {}
    for path in sorted((root / "scripts").rglob("*")):
        if path.is_file():
            by_name.setdefault(path.name, []).append(path)
    return by_name


def roots(root: Path) -> list[Path]:
    found = sorted((root / WORKFLOW_DIR).glob("*.yml"))
    verify = root / VERIFY_SCRIPT
    if verify.is_file():
        found.append(verify)
    return found


def gate_files(root: Path) -> list[Path]:
    gate_dir = root / GATE_DIR
    if not gate_dir.is_dir():
        raise SystemExit(f"{gate_dir}: no such directory; run this from the repository root")
    return sorted(p for p in gate_dir.iterdir() if p.suffix in GATE_SUFFIXES and p.is_file())


def reachable(root: Path, by_name: dict[str, list[Path]]) -> tuple[set[str], list[str]]:
    """Names of files under `scripts/` the pipeline actually reaches, plus dangling call sites."""
    live: set[str] = set()
    dangling: list[str] = []
    sources: list[Path] = list(roots(root))
    seen_files: set[Path] = set()

    while sources:
        path = sources.pop()
        if path in seen_files:
            continue
        seen_files.add(path)
        try:
            text = executable_lines(path)
        except (OSError, UnicodeDecodeError):
            continue
        if path.suffix == ".yml" or path == root / VERIFY_SCRIPT:
            for token in SCRIPT_PATH_RE.findall(text):
                if not (root / token).is_file():
                    dangling.append(f"  {path.relative_to(root)}: names {token}, which does not exist")
        for name, paths in by_name.items():
            if name in live or name not in text:
                continue
            live.add(name)
            sources.extend(paths)
    return live, dangling


def check(root: Path) -> int:
    gates = gate_files(root)
    if not sorted((root / WORKFLOW_DIR).glob("*.yml")):
        raise SystemExit(f"{root / WORKFLOW_DIR}: no workflows found; run this from the repository root")

    by_name = scripts_by_name(root)
    live, dangling = reachable(root, by_name)
    gate_names = {gate.name for gate in gates}
    orphans = sorted(gate_names - live - set(EXEMPT))
    stale_exemptions = sorted(name for name in EXEMPT if not (root / GATE_DIR / name).is_file())

    for line in dangling:
        print(line)
    for name in stale_exemptions:
        print(f"  EXEMPT: {name} no longer exists in {GATE_DIR}/, drop the exemption")
    for name in orphans:
        print(
            f"  {GATE_DIR}/{name} is not invoked by any workflow or by {VERIFY_SCRIPT}; "
            "wire it into a CI step and the `gates` array, or record why in EXEMPT"
        )

    problems = len(dangling) + len(stale_exemptions) + len(orphans)
    if problems:
        print(f"check-guard-wiring: {problems} problem(s)")
        return 1
    verify = root / VERIFY_SCRIPT
    run_in_docker = sorted(
        name
        for name in gate_names
        if verify.is_file() and name in executable_lines(verify)
    )
    print(
        f"check-guard-wiring: OK ({len(gates)} files in {GATE_DIR}/, {len(gate_names & live)} reachable, "
        f"{len(run_in_docker)} also run by {VERIFY_SCRIPT}, {len(EXEMPT)} exempt)"
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path.cwd(), help="repository root to inspect")
    args = parser.parse_args(argv)
    return check(args.root)


if __name__ == "__main__":
    sys.exit(main())
