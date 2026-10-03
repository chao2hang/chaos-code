#!/usr/bin/env python3
"""Fail when a committed document points its reader at a session-private directory.

Rows in `TODO.md` and the architecture notes used to cite evidence as
"see `mt6-sandbox-tty-check.log` in the private goal scratch", and the same
phrase in Chinese. Those logs lived in a per-session temporary directory that is
deleted when the session that produced them ends, so every one of those 40
pointers across 8 documents led a reader nowhere: 21 in `TODO.md`, 10 in
`docs/architecture/todo-open-item-classification.md`, 2 each in `CHANGELOG.md`
and `docs/architecture/todo-completion-roadmap.md`, and one or two in the audit
report, the GUI status note and `docs/ci-test-debt.md`. Nothing failed, because
nothing looked.

The defect is not the wording, it is that the claim was unverifiable by anyone
but the session that wrote it, and it rotted silently. Evidence that matters
belongs under `docs/verification/` in the repository; evidence that was thrown
away should say so instead of naming a path.

The fifth pattern is the same failure in a shape nothing matched: a row that
cites `{SCRATCH}/updater-progress-style-test.log`. The harness substitutes that
placeholder when it renders a plan, so inside a plan it names a real directory;
copied into a committed row it stays a template, and nobody can open it -- not a
reader, and not the session that wrote it either once that session is over.
Those four phrases at least point at a directory that existed once; this one
never existed anywhere, which is why the product prompt that *defines* the
spelling is exempt while a citation that uses it is not.

Scope is prose: tracked `*.md` plus the generated `*.tsv` exports under
`docs/verification/`. Captured command transcripts (`docs/verification/*.log`)
are deliberately *not* scanned -- they quote the absolute scratch path of the
script they ran because that is what the command line said, and rewriting a
transcript would falsify it.

Usage:
    check-evidence-paths.py [--root DIR] [--list]
Exit: 0 = no document points at scratch, and every exemption still exists.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

# Phrase => why it is a problem. Matched case-insensitively.
PATTERNS: tuple[tuple[str, re.Pattern[str]], ...] = (
    (
        "scratch named as the place evidence lives",
        re.compile(r"goal[-\s]scratch", re.IGNORECASE),
    ),
    (
        "scratch named as the place evidence lives (Chinese)",
        re.compile(r"私有\s*scratch", re.IGNORECASE),
    ),
    (
        "absolute path under a per-session scratch root",
        re.compile(r"/tmp/grok-goal-"),
    ),
    (
        "plan working directory named as the place evidence lives",
        re.compile(r"\bplan scratch\b", re.IGNORECASE),
    ),
    (
        "unresolved plan placeholder cited as an evidence path",
        re.compile(r"\{SCRATCH[A-Z_]*\}\s*/"),
    ),
)

# Tracked documents allowed to contain a matched phrase, with the reason. An
# entry whose file no longer exists is itself a failure, so this cannot collect
# retired waivers.
EXEMPT: dict[str, str] = {
    "crates/codegen/xai-grok-shell/src/session/templates/goal_strategist_prompt.md":
        "ships to the model as product vocabulary: `{SCRATCH_ROOT}` is documented "
        "as the per-goal scratch root the agent is told to write into",
    "crates/codegen/xai-grok-shell/src/session/templates/goal_planner_prompt.md":
        "ships to the model as product vocabulary: it is the file that defines the "
        "`{SCRATCH}/out.log` spelling the planner is told to write into plans",
}

SCAN_SUFFIXES = frozenset({".md", ".tsv"})
SKIP_DIRS = frozenset({".git", "target", "node_modules", "dist", "build"})
# The TSV files under scripts/ci/ are guard data, not evidence pointers.
TSV_ROOTS = ("docs/",)


def scan_file(path: Path, root: Path) -> list[str]:
    rel = path.relative_to(root).as_posix()
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        text = path.read_text(encoding="utf-8", errors="replace")
    hits: list[str] = []
    for lineno, line in enumerate(text.splitlines(), 1):
        for why, pattern in PATTERNS:
            if pattern.search(line):
                snippet = line.strip()
                if len(snippet) > 160:
                    snippet = snippet[:160] + "…"
                hits.append(f"{rel}:{lineno}: {why}\n    {snippet}")
                break
    return hits


def candidates(root: Path) -> list[Path]:
    """Every scanned document, without walking build output.

    `rglob` would descend into `target/` and `node_modules/` before the suffix
    filter could reject anything, which costs seconds in CI and minutes on a
    developer machine that has both.
    """
    out: list[Path] = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
        for name in sorted(filenames):
            path = Path(dirpath) / name
            if path.suffix not in SCAN_SUFFIXES:
                continue
            rel = path.relative_to(root)
            if path.suffix == ".tsv" and not rel.as_posix().startswith(TSV_ROOTS):
                continue
            out.append(path)
    return out


def check(root: Path) -> list[str]:
    problems: list[str] = []
    seen: set[str] = set()
    for path in candidates(root):
        rel = path.relative_to(root).as_posix()
        seen.add(rel)
        if rel in EXEMPT:
            continue
        problems.extend(scan_file(path, root))
    for name in sorted(EXEMPT):
        if name not in seen:
            problems.append(
                f"{name}: exempted document is not in the scanned set any more; "
                "drop the exemption"
            )
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=str(Path(__file__).resolve().parents[2]))
    parser.add_argument("--list", action="store_true", dest="list_files",
                        help="print the scanned files and exit")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    if args.list_files:
        for path in candidates(root):
            rel = path.relative_to(root).as_posix()
            mark = " (exempt)" if rel in EXEMPT else ""
            print(f"{rel}{mark}")
        return 0

    problems = check(root)
    for problem in problems:
        print(f"check-evidence-paths: {problem}", file=sys.stderr)
    if problems:
        print(
            "check-evidence-paths: committed documents point at a directory that is\n"
            "deleted when the session that wrote it ends. Either move the evidence\n"
            "into docs/verification/ and cite that path, or say the log was not\n"
            "retained. Scanned "
            f"{len(candidates(root))} markdown/tsv files for "
            f"{len(PATTERNS)} patterns.",
            file=sys.stderr,
        )
        return 1
    print(
        f"check-evidence-paths: {len(candidates(root))} markdown/tsv files scanned, "
        "none points at a session-private scratch directory"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
