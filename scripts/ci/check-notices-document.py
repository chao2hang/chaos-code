#!/usr/bin/env python3
"""Reject a third-party notices document that cannot be read, or that promises what it does not deliver.

`THIRD-PARTY-NOTICES` is not a comment on the code, it is the attribution that the compiled
product owes: one entry per third-party package linked into the released binary, each with the
license this distribution ships under, a copyright notice, and a pointer to the license text.
Nobody reads it, which is exactly why the ways it goes wrong are quiet:

- a reader cannot check a document it cannot parse. An entry whose heading lost its version, or
  a Part II section whose `#` banner lost a character, disappears from any tool that reads the
  file, including the generator that maintains it. The entry is still there for a human
  scrolling past, and missing for everything else;
- an entry that says `License: (MIT OR Apache-2.0) AND ISC` without saying which of MIT and
  Apache-2.0 was taken leaves the reader to guess about a legal document;
- `License text: see Part II — Zlib` where Part II has no `Zlib` section points a reader at a
  text that is not in the file they are holding. This is the one defect that has no
  self-evident symptom at all: the entry looks complete.

As of 2026-10-05 the document has 1139 entries and 12 license texts, and the entries are derived
from `cargo metadata` by `scripts/gen-third-party-notices.py`. That generator rewrites entries it
has to and leaves the rest byte for byte, so everything written by hand -- the vendored-crate blocks,
the ported-source sections, the wording of a note -- survives it unchanged. Which is the point of this guard: the
generator cannot degrade what it preserves, but a person can, and nothing else in the repository
reads this file.

What is checked. All of it is a fact about text, so it runs in the container, which has no cargo
registry and cannot resolve a dependency graph:

1. The document parses. Part I before Part II, at least one entry, at least one license section,
   the closing banner present. Anything unreadable is a failure, never a skip.
2. One entry per `(name, version)`, in the order the document sorts itself by -- a duplicate
   key means two entries making two different claims about one package.
3. Every entry says where the package came from, on a `Source:` line that reads as a URL.
4. Every entry carries a `Copyright notice:` block with something in it.
5. Every entry's license is readable: one id, or an expression with its applicable terms spelled
   out, and every term one this project reproduces or points at.
6. An entry that records what upstream declared must be consistent with that declaration: the
   terms it records are terms the declaration offers, the terms the tool would have chosen are
   among them, and the entry explains the choice in the note line, naming the same expression.
7. Every `License text:` line is in one of the two legal shapes, and a pointer into Part II
   resolves to a section that exists and is not empty.
8. Every Part II section is reachable, either from an entry's `License text:` line or from prose
   elsewhere in the file: the vendored-C-library case, where the entry names the file rather
   than the SPDX id.

Being more generous than the tool is allowed: an entry may record the full union of an
expression's terms where the tool would pick one (`encoding_rs` records BSD-3-Clause, Apache-2.0
and MIT for `Apache-2.0 OR MIT`). Narrowing below what the tool would choose, or recording a term
the declaration does not contain, is not: that is the document taking away permission that the
upstream license grants.

Usage: python3 scripts/ci/check-notices-document.py [--root DIR] [--document FILE] [--list]
Exit: 0 = the document reads, and every pointer and claim inside it holds.
"""

from __future__ import annotations

import argparse
import importlib.util
import re
import sys
from pathlib import Path
from typing import NamedTuple

GUARD = "check-notices-document"
LIB = "notices_lib.py"

# The note a choice-expression entry must carry, from the document's own convention: the
# expression upstream wrote, and the terms this distribution satisfies itself under. Wrapping is
# allowed, because a line break says nothing about what the note claims.
CHOICE_NOTE_RE = re.compile(
    r"^\s*Upstream\s+license\s+expression:\s+(.+?)\.\s+For\s+this\s+distribution,\s+"
    r"obligations\s+are\s+satisfied\s+under:\s+(.+?)\.?\s*$",
    re.M | re.S,
)
APPLICABLE_TERMS_RE = re.compile(r"^\s*\(applicable terms:\s*(.*?)\)\s*$", re.M)
END_BANNER = "END OF THIRD-PARTY NOTICES"
SOURCE_RE = re.compile(r"^Source:\s+(https?://\S+)\s*$", re.M)
URL_RE = re.compile(r"^https?://\S+$")


def load_lib() -> object:
    """Import `scripts/notices_lib.py`, the reading both this guard and the generator share."""
    path = Path(__file__).resolve().parent.parent / LIB
    if not path.is_file():
        raise SystemExit(f"{GUARD}: {path} is missing, so nothing here can be checked")
    spec = importlib.util.spec_from_file_location("notices_lib", path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class Stats(NamedTuple):
    entries: int
    sections: int
    with_declaration: int
    part2_pointers: int


def check_structure(document: object, text: str, lib: object, problems: list[str]) -> None:
    """1. The document reads, and reads as the two-part document it claims to be."""
    if text.find(lib.PART_I) >= text.find(lib.PART_II):
        problems.append(f"{lib.PART_I!r} must come before {lib.PART_II!r}; the reader splits the "
                        f"file at both headings and would put every entry in Part II")
    if END_BANNER not in text:
        problems.append(f"the closing {END_BANNER!r} banner is missing, so the end of the document "
                        f"is wherever a reader stops looking")
    if not document.entries:
        problems.append("Part I holds no readable entries")
    if not document.license_sections:
        problems.append("Part II holds no readable license sections")


def check_order(document: object, lib: object, problems: list[str]) -> None:
    """2. One entry per package version, in the order the document keeps itself in."""
    seen: dict[tuple[str, str], int] = {}
    for entry in document.entries:
        seen[entry.key] = seen.get(entry.key, 0) + 1
    for key, count in sorted(seen.items()):
        if count > 1:
            problems.append(f"{key[0]} {key[1]}: {count} entries for one package version, which "
                            f"can make two different claims about the same shipped code")
    keys = [entry.key for entry in document.entries]
    ordered = sorted(keys, key=lambda key: (key[0], lib.version_key(key[1])))
    if keys != ordered:
        index = next(i for i, (a, b) in enumerate(zip(keys, ordered)) if a != b)
        at = document.entries[index]
        problems.append(f"{at.name} {at.version}: entries are not in (name, version) order; the "
                        f"document is alphabetised so a diff shows one entry moved instead of "
                        f"rewriting everything between the first and the last")


def check_entry(entry: object, lib: object, problems: list[str], stats: list[int]) -> None:
    """3-7. Everything an entry owes the reader, entry by entry."""
    where = f"{entry.name} {entry.version}"

    sources = SOURCE_RE.findall(entry.body)
    if not sources:
        line = re.search(r"^Source:.*$", entry.body, re.M)
        if line:
            problems.append(f"{where}: `{line.group(0).strip()}` does not give a http(s) URL, so "
                            f"the reader cannot check the entry against the package it describes")
        else:
            problems.append(f"{where}: no `Source:` line")
    if not entry.has_copyright():
        problems.append(f"{where}: no `Copyright notice:` block; the attribution that has to "
                        f"travel with the code is not recorded for it")
    elif not re.search(r"^  \S", entry.body.split("Copyright notice:", 1)[1], re.M):
        problems.append(f"{where}: `Copyright notice:` is followed by nothing indented, so it "
                        f"records no notice")

    terms = entry.recorded_terms()
    if terms is None:
        problems.append(f"{where}: `License: {entry.chosen() or '(absent)'}` names neither one "
                        f"license nor an expression with its applicable terms spelled out; a "
                        f"reader should not have to guess which terms this was shipped under")
        terms = []
    for term in terms:
        if term not in lib.ALLOWED_LICENSES:
            problems.append(f"{where}: {term!r} is outside the license set this project ships "
                            f"under; add the text to Part II and the id to the allowlist in "
                            f"{LIB} only with a deliberate decision")

    declared = entry.declared()
    if declared:
        stats[2] += 1
        try:
            offered = lib.expression_terms(declared)
            expected = lib.choose_licenses(declared)
        except lib.Unreadable as exc:
            problems.append(f"{where}: the declared license expression {declared!r} cannot be "
                            f"read: {exc}")
            offered = None
        if offered is not None:
            for term in terms:
                if term not in offered:
                    problems.append(f"{where}: records {term!r}, which the declared expression "
                                    f"{declared!r} does not offer")
            for term in expected:
                if term not in terms:
                    problems.append(f"{where}: declares {declared!r}, which requires {term!r} to "
                                    f"be among the recorded terms {terms}")
        note = CHOICE_NOTE_RE.search(entry.body)
        if note is None:
            problems.append(f"{where}: says what upstream declared but not what was chosen from "
                            f"it; add the `Upstream license expression: ... For this "
                            f"distribution, obligations are satisfied under: ...` line")
        else:
            if re.sub(r"\s+", " ", note.group(1)).strip() != re.sub(r"\s+", " ", declared).strip():
                problems.append(f"{where}: the note names expression {note.group(1).strip()!r} "
                                f"while the License line declares {declared!r}")
            noted = [part.strip() for part in note.group(2).split(",")]
            if noted != terms:
                problems.append(f"{where}: the note says obligations are satisfied under "
                                f"{noted}, while the entry records {terms}")

    pointers = re.findall(r"^License text:.*$", entry.body, re.M)
    if len(pointers) != 1:
        problems.append(f"{where}: {len(pointers)} `License text:` lines, expected exactly one "
                        f"telling the reader where the license text is")
    elif entry.references():
        stats[3] += 1
    else:
        url, license_id = entry.upstream_text()
        if not URL_RE.match(url or ""):
            problems.append(f"{where}: `{pointers[0]}` is neither `see Part II — <id>` nor "
                            f"`see upstream <url> (license: <id>)`")
        elif license_id not in lib.ALLOWED_LICENSES:
            problems.append(f"{where}: sends the reader upstream for {license_id!r}, an id "
                            f"outside the license set this project ships under")


def check_pointers(document: object, lib: object, problems: list[str]) -> None:
    """7-8. Every pointer into Part II resolves, and every section is reachable."""
    for entry in document.entries:
        for section in entry.references():
            body = document.license_sections.get(section)
            if body is None:
                problems.append(f"{entry.name} {entry.version}: points at Part II section "
                                f"{section!r}, which the document does not contain")
            elif not body.strip():
                problems.append(f"{entry.name} {entry.version}: points at Part II section "
                                f"{section!r}, which is empty")

    # `notices_lib` owns the reachability rule, because the generator that writes Part II has to
    # apply the same one when it drops a text no entry asks for any more.
    for section in lib.unreachable_sections(document):
        problems.append(f"Part II section {section!r} is not pointed at by any entry and its "
                        f"id appears nowhere else in the document; either link it from the "
                        f"entry that needs it or drop the text")


def check(document: object, text: str, lib: object) -> tuple[list[str], Stats]:
    problems: list[str] = []
    stats = [0, 0, 0, 0]
    check_structure(document, text, lib, problems)
    check_order(document, lib, problems)
    for entry in document.entries:
        check_entry(entry, lib, problems, stats)
    check_pointers(document, lib, problems)
    stats[0] = len(document.entries)
    stats[1] = len(document.license_sections)
    return problems, Stats(*stats)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=".", type=Path)
    parser.add_argument("--document", default=None, type=Path,
                        help="check this file instead of the one at the repository root")
    parser.add_argument("--list", action="store_true",
                        help="print the entry and section counts, then exit")
    ns = parser.parse_args(argv)

    lib = load_lib()
    path = ns.document if ns.document else ns.root.resolve() / lib.DOCUMENT
    if not path.is_file():
        print(f"{GUARD}: {path} is not a file; run this from the repository root or pass "
              f"--document", file=sys.stderr)
        return 2
    text = path.read_text(encoding="utf-8")
    try:
        document = lib.Document(text)
    except lib.Unreadable as exc:
        print(f"{GUARD}: FAIL (1 problem(s))")
        print(f"  {exc}")
        return 1

    problems, stats = check(document, text, lib)
    if ns.list:
        print(f"{stats.entries} entries, {stats.sections} license sections, "
              f"{stats.with_declaration} recording a declared expression, "
              f"{stats.part2_pointers} pointing into Part II")
        return 0

    if problems:
        print(f"{GUARD}: FAIL ({len(problems)} problem(s))")
        for problem in problems:
            print(f"  {problem}")
        return 1
    print(f"{GUARD}: OK ({stats.entries} entries, {stats.sections} license texts, "
          f"{stats.with_declaration} license choices explained, every Part II pointer resolves)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
