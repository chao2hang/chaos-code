#!/usr/bin/env python3
"""Print every open/partial TODO line grouped by nearest Markdown heading.

Three modes:

- (default) one line per open/partial row: `LINE<TAB>STATE<TAB>SECTION<TAB>TEXT`, plus a
  `TOTAL` line. This is the inventory a person reads.
- `--groups` collapses those rows into the milestone groups used by
  `docs/architecture/todo-open-item-classification.md`, and refuses a row whose section
  belongs to no group -- a new `### M6.` heading must be assigned on purpose, because a
  silently ungrouped row is invisible to every count in that document.
- `--check-doc <path>` compares that document's count table against `TODO.md` and exits 1
  on drift. The document already tells the reader to "recompute after the next edit"; an
  instruction nobody enforces is how its table sat 13 rows off (149 claimed vs 136 real)
  while the file claimed to be the status snapshot.

`--todo <path>` points at another TODO file, which is what the fixtures use: they run this
same file against a generated tree instead of copying its logic.
"""

from __future__ import annotations

import argparse
import sys
from collections import OrderedDict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_TODO = ROOT / "TODO.md"
DEFAULT_DOC = ROOT / "docs/architecture/todo-open-item-classification.md"

MAINTENANCE = "Maintenance items / §8"
# Order matters: `### M-1.` must be tried before `### M1.`, and the group a section maps
# to is decided by the first prefix it starts with.
GROUP_PREFIXES: tuple[tuple[str, str], ...] = (
    ("### M-1.", "M-1"),
    ("### M0.", "M0"),
    ("### M1.", "M1"),
    ("### M2.", "M2"),
    ("### M3.", "M3"),
    ("## M3.3", "M3"),
    ("### M4.", "M4"),
    ("### M5.", "M5"),
    ("## MT-", MAINTENANCE),
    ("### 8.", MAINTENANCE),
)
GROUPS: tuple[str, ...] = ("M-1", "M0", "M1", "M2", "M3", "M4", "M5", MAINTENANCE)


def rows(todo_path: Path) -> list[tuple[int, str, str, str]]:
    """`(line number, unchecked|partial, nearest heading, text)` for every open row."""
    found: list[tuple[int, str, str, str]] = []
    section = "(document preamble)"
    for number, line in enumerate(todo_path.read_text(encoding="utf-8").splitlines(), 1):
        if line.startswith("#"):
            section = line
        if line.startswith("- [ ]"):
            found.append((number, "unchecked", section, line))
        elif line.startswith("- [~]"):
            found.append((number, "partial", section, line))
    return found


def group_of(section: str) -> str | None:
    for prefix, group in GROUP_PREFIXES:
        if section.startswith(prefix):
            return group
    return None


def tally(found: list[tuple[int, str, str, str]]) -> OrderedDict[str, dict[str, int]]:
    """Per-group counts. Raises on a row no group owns, rather than dropping it."""
    counts: OrderedDict[str, dict[str, int]] = OrderedDict(
        (group, {"unchecked": 0, "partial": 0}) for group in GROUPS
    )
    unowned = []
    for number, state, section, _line in found:
        group = group_of(section)
        if group is None:
            unowned.append(f"line {number}: {section}")
            continue
        counts[group][state] += 1
    if unowned:
        raise SystemExit(
            "TODO rows are not assigned to a milestone group, so the classification "
            "document cannot count them:\n  "
            + "\n  ".join(unowned)
            + f"\nAdd the section to GROUP_PREFIXES in {Path(__file__).name} and to "
            f"{DEFAULT_DOC.relative_to(ROOT)}."
        )
    return counts


def documented_counts(doc_path: Path) -> dict[str, dict[str, int]]:
    """The `| group | unchecked | partial |` rows of the classification document."""
    table: dict[str, dict[str, int]] = {}
    for line in doc_path.read_text(encoding="utf-8").splitlines():
        if not line.startswith("| "):
            continue
        columns = [column.strip() for column in line.strip("|").split("|")]
        if len(columns) == 3 and columns[0] in GROUPS:
            if columns[0] in table:
                raise SystemExit(f"{doc_path}: group {columns[0]} appears twice in its table")
            try:
                table[columns[0]] = {"unchecked": int(columns[1]), "partial": int(columns[2])}
            except ValueError:
                raise SystemExit(f"{doc_path}: non-numeric counts for {columns[0]}: {line!r}")
    missing = [group for group in GROUPS if group not in table]
    if missing:
        raise SystemExit(f"{doc_path}: table has no row for {', '.join(missing)}")
    return table


def check_doc(doc_path: Path, actual: dict[str, dict[str, int]]) -> int:
    """Exit 0 when the document's table equals TODO.md, 1 with a readable diff."""
    claimed = documented_counts(doc_path)
    drift = []
    for group in GROUPS:
        for state in ("unchecked", "partial"):
            if claimed[group][state] != actual[group][state]:
                drift.append(
                    f"  {group} {state}: document says {claimed[group][state]}, "
                    f"TODO.md has {actual[group][state]}"
                )
    claimed_total = sum(c["unchecked"] + c["partial"] for c in claimed.values())
    actual_total = sum(c["unchecked"] + c["partial"] for c in actual.values())
    if claimed_total != actual_total:
        drift.append(
            f"  total rows: document says {claimed_total}, TODO.md has {actual_total}"
        )
    if not drift:
        print(
            f"classify-open-todos: {doc_path.name} matches TODO.md "
            f"({actual_total} open/partial rows)"
        )
        return 0
    print(
        f"classify-open-todos: {doc_path.name} does not match TODO.md\n" + "\n".join(drift)
    )
    print("recompute with: python3 scripts/ci/classify-open-todos.py --groups")
    return 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--todo", type=Path, default=DEFAULT_TODO, help="TODO file to read")
    parser.add_argument("--groups", action="store_true", help="print per-milestone counts")
    parser.add_argument("--check-doc", type=Path, metavar="PATH", help="compare a classification document to --todo")
    args = parser.parse_args(argv)

    found = rows(args.todo)
    counts = tally(found)

    if args.check_doc:
        return check_doc(args.check_doc, counts)
    if args.groups:
        for group, states in counts.items():
            print(f"{group}\t{states['unchecked']}\t{states['partial']}")
        return 0

    for number, state, section, line in found:
        print(f"{number}\t{state}\t{section}\t{line}")
    unchecked = sum(c["unchecked"] for c in counts.values())
    partial = sum(c["partial"] for c in counts.values())
    print(f"TOTAL\tunchecked={unchecked}\tpartial={partial}\trows={unchecked + partial}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
