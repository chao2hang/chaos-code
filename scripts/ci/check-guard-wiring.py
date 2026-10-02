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

The second rule is about the local entry point. `verify-in-docker.sh` used to run
16 of these 32 guards and print that number without failing on it, which meant a
guard could stop being mirrored and nobody would notice -- the same "green proves
nothing" shape as an unwired gate, one layer down. Now every guard is either
mirrored (named by the entry point, or by a script the entry point runs, the same
transitive rule as above) or recorded in `scripts/ci/docker-entry-ci-only.tsv`
with what it needs that a clean container does not have. `--list-mirror` prints
the resulting classification, one guard per line.

Usage: python3 scripts/ci/check-guard-wiring.py [--root DIR] [--list-mirror]
Exit: 0 = every gate reachable, mirrored or recorded, no dangling call site.
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
# Only these can *name* another script. A `.tsv`/`.txt` under `scripts/` is data:
# `docker-entry-ci-only.tsv` lists the guards it exempts, and reading a data file
# as a caller would turn every name in it into an invocation -- including this
# check's own list, which would then mark the guards it exempts as mirrored.
CALLER_SUFFIXES = frozenset({".py", ".sh", ".mjs", ".yml"})
GATE_DIR = "scripts/ci"
VERIFY_SCRIPT = "scripts/verify-in-docker.sh"
WORKFLOW_DIR = ".github/workflows"
CI_ONLY_TSV = "docker-entry-ci-only.tsv"

# Files in scripts/ci/ deliberately not run by CI, each with why. An entry whose
# file no longer exists is itself a failure.
EXEMPT: dict[str, str] = {
    "ignored-tests.sh": "`exec` alias for ignored-tests.py, which is wired; the docs quote the alias",
}

SCRIPT_PATH_RE = re.compile(r"(scripts/[A-Za-z0-9_.\-/]*\.(?:py|sh|mjs|ps1))")


def name_pattern(name: str) -> re.Pattern[str]:
    """Match a file name only where a path names it: the `/` in front is required.

    Bare-name matching over-counts in both directions. `test-ignored-tests.py`
    contains `ignored-tests.py`, and a test file that lists the guards it covers
    would otherwise mark them invoked -- which reads as fine for reachability but
    turns into a false "the entry point runs it" for the mirror rule.
    `"$(dirname "$0")/helper.py"` still counts, because the slash is there.
    """
    return re.compile(r"/" + re.escape(name) + r"(?![A-Za-z0-9_.\-])")


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


def _closure(
    root: Path,
    by_name: dict[str, list[Path]],
    seeds: list[Path],
    *,
    collect_dangling: bool,
) -> tuple[set[str], list[str]]:
    """File names reachable from `seeds`, plus call sites pointing at nothing.

    Matching is by file name, so a guard reached through a script that CI runs
    counts exactly once per hop, and the same walk serves both directions: CI from
    the workflows, the local entry point from `verify-in-docker.sh` alone.
    """
    live: set[str] = set()
    dangling: list[str] = []
    sources: list[Path] = list(seeds)
    seen_files: set[Path] = set()
    patterns = {name: name_pattern(name) for name in by_name}

    while sources:
        path = sources.pop()
        if path in seen_files or path.suffix not in CALLER_SUFFIXES:
            continue
        seen_files.add(path)
        try:
            text = executable_lines(path)
        except (OSError, UnicodeDecodeError):
            continue
        if collect_dangling and (path.suffix == ".yml" or path == root / VERIFY_SCRIPT):
            for token in SCRIPT_PATH_RE.findall(text):
                if not (root / token).is_file():
                    dangling.append(f"  {path.relative_to(root)}: names {token}, which does not exist")
        for name, paths in by_name.items():
            if name in live or not patterns[name].search(text):
                continue
            live.add(name)
            sources.extend(paths)
    return live, dangling


def reachable(root: Path, by_name: dict[str, list[Path]]) -> tuple[set[str], list[str]]:
    """Names of files under `scripts/` the pipeline actually reaches, plus dangling call sites."""
    return _closure(root, by_name, roots(root), collect_dangling=True)


def mirrored(root: Path, by_name: dict[str, list[Path]]) -> set[str]:
    """Names the local Docker entry point reaches, directly or through a script it runs."""
    verify = root / VERIFY_SCRIPT
    if not verify.is_file():
        return set()
    live, _ = _closure(root, by_name, [verify], collect_dangling=False)
    return live


def ci_only(root: Path) -> dict[str, str]:
    """Guards kept out of the local entry point, each with what it needs.

    A malformed row is fatal rather than skipped. Dropping one quietly would
    report the guard it names as "not mirrored", sending the reader to the gates
    array instead of to the row with the missing tab.
    """
    path = root / GATE_DIR / CI_ONLY_TSV
    entries: dict[str, str] = {}
    if not path.is_file():
        return entries
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        name, tab, reason = raw.partition("\t")
        name, reason = name.strip(), reason.strip()
        if not tab or not name or not reason:
            raise SystemExit(
                f"{path}:{number}: expected `<name>\\t<reason>` with a non-empty reason, got {raw!r}"
            )
        entries[name] = reason
    return entries


def check(root: Path, list_mirror: bool = False) -> int:
    gates = gate_files(root)
    if not sorted((root / WORKFLOW_DIR).glob("*.yml")):
        raise SystemExit(f"{root / WORKFLOW_DIR}: no workflows found; run this from the repository root")

    by_name = scripts_by_name(root)
    live, dangling = reachable(root, by_name)
    gate_names = {gate.name for gate in gates}
    orphans = sorted(gate_names - live - set(EXEMPT))
    stale_exemptions = sorted(name for name in EXEMPT if not (root / GATE_DIR / name).is_file())

    keep = ci_only(root)
    mirror = gate_names & mirrored(root, by_name)
    # A guard the entry point does not reach has to be written down somewhere.
    # This is the rule that keeps the local mirror from shrinking by accident:
    # nothing else notices when a line leaves the `gates` array.
    unmirrored = sorted(gate_names - mirror - set(keep) - set(EXEMPT))
    # The list has to describe this repository: a row for a deleted guard is a
    # graveyard, and a row for a guard the entry point runs anyway is a lie about
    # coverage that the next reader would trust.
    stale_ci_only = sorted(name for name in keep if not (root / GATE_DIR / name).is_file())
    mirrored_but_listed = sorted(name for name in keep if name in mirror and (root / GATE_DIR / name).is_file())

    for line in dangling:
        print(line)
    for name in stale_exemptions:
        print(f"  EXEMPT: {name} no longer exists in {GATE_DIR}/, drop the exemption")
    for name in orphans:
        print(
            f"  {GATE_DIR}/{name} is not invoked by any workflow or by {VERIFY_SCRIPT}; "
            "wire it into a CI step and the `gates` array, or record why in EXEMPT"
        )
    for name in unmirrored:
        print(
            f"  {GATE_DIR}/{name} runs in CI but not in {VERIFY_SCRIPT}; mirror it into the `gates` "
            f"array or record what it needs in {GATE_DIR}/{CI_ONLY_TSV}"
        )
    for name in stale_ci_only:
        print(f"  {CI_ONLY_TSV}: {name} no longer exists in {GATE_DIR}/, drop the row")
    for name in mirrored_but_listed:
        print(
            f"  {CI_ONLY_TSV}: {name} is listed as CI-only but {VERIFY_SCRIPT} runs it; "
            "drop the row so the coverage count is not inflated"
        )

    problems = len(dangling) + len(stale_exemptions) + len(orphans) + len(unmirrored)
    problems += len(stale_ci_only) + len(mirrored_but_listed)
    if problems:
        print(f"check-guard-wiring: {problems} problem(s)")
        return 1
    if list_mirror:
        for name in sorted(gate_names):
            if name in mirror:
                print(f"mirrored  {name}")
            elif name in keep:
                print(f"ci-only   {name}  -- {keep[name]}")
            else:
                print(f"exempt    {name}")
    print(
        f"check-guard-wiring: OK ({len(gates)} files in {GATE_DIR}/, {len(gate_names & live)} reachable, "
        f"{len(mirror)} run by {VERIFY_SCRIPT}, {len(keep)} recorded CI-only, {len(EXEMPT)} exempt)"
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path.cwd(), help="repository root to inspect")
    parser.add_argument(
        "--list-mirror",
        action="store_true",
        help="print how each guard is classified (mirrored / ci-only / exempt) before the summary",
    )
    args = parser.parse_args(argv)
    return check(args.root, list_mirror=args.list_mirror)


if __name__ == "__main__":
    sys.exit(main())
