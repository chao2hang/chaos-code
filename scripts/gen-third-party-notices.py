#!/usr/bin/env python3
"""Make THIRD-PARTY-NOTICES cover the dependencies this binary is actually built from.

The notices document is a legal artifact and it had drifted: 135 shipped packages had no entry, 28
entries named a version that is no longer in the build, and 191 entries described packages nothing
ships any more. `scripts/ci/check-notices-coverage.py` re-measures those three counts on every
commit; this script is the way to fix them, which matters because an 18 738-line document is not
something to repair by hand.

What it does on `--write`:

- drops every entry for a package that is not in the shipped dependency set, and writes one entry
  per shipped package that has no entry (or whose entry names a version that is gone),
- leaves every entry that survives exactly as it is. Entry bodies carry prose no tool should
  rewrite: verbatim upstream `NOTICE` files, and `VENDORED WITH LOCAL MODIFICATIONS:` blocks that
  describe what this repository changed inside `third_party/`. Bodies are spliced out of the old
  text and back into the new one byte for byte, and the script checks that after writing.
- appends a Part II license text only for a license id that has no section yet, and only from the
  text the package itself ships. Texts already in the document are never rewritten, so what was
  reviewed once keeps its wording. A text is dropped when the last entry that pointed at it goes
  away, because a license nobody ships under has no business being in the file.

Two properties are promised and both are checked in `docs/verification/`:

- the entry format is the format the committed document already uses, derived from the 1 167
  entries it held when this script was written (see `scripts/notices_lib.py` for the license-choice
  rule and the measurement that reproduced all 692 of those entries' recorded licenses),
- running `--write` twice leaves the file byte-identical, so `cargo`-level churn shows up as a
  notices diff and nothing else.

Usage:
  python3 scripts/gen-third-party-notices.py              # report the difference, change nothing
  python3 scripts/gen-third-party-notices.py --write      # rewrite THIRD-PARTY-NOTICES
  python3 scripts/gen-third-party-notices.py --license-text EPL-2.0=path/to/text.txt
  python3 scripts/gen-third-party-notices.py --metadata saved.json   # read a saved `cargo metadata`
                                     instead of running cargo, the way `check-notices-coverage.py`
                                     does, so a regeneration can be rehearsed without a toolchain
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from pathlib import Path

MODULE = Path(__file__).resolve().parent / "notices_lib.py"


def _load_lib():
    spec = importlib.util.spec_from_file_location("notices_lib", MODULE)
    if spec is None or spec.loader is None:
        raise SystemExit(f"gen-third-party-notices: cannot load {MODULE}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


notices = _load_lib()

RULE = "-" * 80
CRULER = "#" * 80
END_BANNER = f"{'=' * 80}\nEND OF THIRD-PARTY NOTICES\n{'=' * 80}\n"
CRATES_IO = "https://crates.io/crates"


def package_url(dep) -> str:
    """Where the code came from: the manifest's repository, else the crates.io page."""
    return dep.url or f"{CRATES_IO}/{dep.name}/{dep.version}"


def _kinds(node: tuple) -> set[str]:
    if node[0] == "id":
        return {"id"}
    found = {node[0]}
    for child in node[1]:
        found |= _kinds(child)
    return found


def license_lines(dep, terms: list[str]) -> list[str]:
    """The `License:` line(s), in whichever of the document's three shapes applies.

    A single-license package gets the id alone. A package that offered a choice gets the chosen id
    plus what upstream declared, so a reader can see the choice was made and what it was made from.
    A package whose terms genuinely stack gets the declared expression plus the list of terms that
    stack, because `License: (A OR B) AND C` on its own does not say which of A and B applies.
    """
    node = notices.parse_expression(dep.expression)
    canonical = notices.CANONICAL_LICENSES.get(node[1], node[1]) if node[0] == "id" else None
    if node[0] == "id" and canonical == node[1]:
        return [f"License: {terms[0]}"]
    if len(terms) == 1:
        return [f"License: {terms[0]}  (upstream declares: {dep.expression})"]
    return [f"License: {dep.expression}", f"  (applicable terms: {', '.join(terms)})"]


def additional_lines(dep, terms: list[str]) -> list[str]:
    """The `Additional requirements / notices:` lines, when the expression needs a word about it.

    The rule the document already follows is that an entry naming a license other than the one
    upstream wrote, or naming more than one, explains itself here. A bare id that needed no
    rewriting is the only case left silent.
    """
    node = notices.parse_expression(dep.expression)
    kinds = _kinds(node)
    if kinds == {"id"} and notices.CANONICAL_LICENSES.get(node[1], node[1]) == node[1]:
        return []
    if "or" in kinds or kinds == {"id"}:
        verb = "For this distribution, obligations are satisfied under"
    else:
        verb = "All of the following license terms apply"
    return [f"  Upstream license expression: {dep.expression}. {verb}: {', '.join(terms)}."]


def copyright_lines(dep, source: Path | None) -> list[str]:
    """The indented block under `Copyright notice:`, from the most authoritative reading available.

    The copyright line inside the package's own license file is the attribution the author wrote, so
    it comes first. Failing that, the metadata's `authors` list, which is the fallback the document
    already used for 232 of the entries this was derived from. Failing both, the entry says so in
    words rather than inventing a holder.
    """
    lines = notices.copyright_lines(source) if source is not None else []
    if lines:
        return [f"  {line}" for line in lines]
    if dep.authors:
        return ["  Copyright holders / authors (from package metadata):"] + [
            f"    - {author}" for author in dep.authors
        ]
    return [
        "  The package as published carries no copyright notice in the files it ships and its",
        "  metadata names no authors; the attribution available for it is the upstream",
        f"  repository recorded under Source above: {package_url(dep)}",
    ]


def license_text_source(dep, license_id: str, repo: Path) -> tuple[Path, str] | None:
    """The package's own copy of a license text, when exactly one file can be that text.

    Only consulted for a package that declares nothing else, so the file at its root has to be the
    license named. When several candidate files sit at the root, the one named after the id is used
    and an ambiguous set is refused: guessing which of two texts is the license is not a call a
    script should make about a legal document.
    """
    source = notices.registry_source(dep, repo)
    if source is None:
        return None
    candidates = [
        path for path in sorted(source.iterdir()) if path.is_file() and notices.LICENSE_FILE_RE.match(path.name)
    ]
    if not candidates:
        return None
    stem = license_id.split()[0].split("+")[0].lower()
    preferred = [path for path in candidates if stem in path.name.lower()]
    chosen = preferred or (candidates if len(candidates) == 1 else [])
    if len(chosen) != 1:
        return None
    try:
        text = chosen[0].read_text(encoding="utf-8", errors="strict")
    except (OSError, UnicodeDecodeError):
        return None
    return (chosen[0], text.strip("\n"))


def render_entry(dep, sections: set[str], source: Path | None) -> str:
    """One Part I entry, formatted the way the document already formats them."""
    terms = notices.sort_licenses(dep.licenses())
    body = [f"Source:  {package_url(dep)}", *license_lines(dep, terms), "", "Copyright notice:"]
    body += copyright_lines(dep, source)
    body.append("")
    in_document = [term for term in terms if term in sections]
    if in_document:
        body.append(f"License text: see Part II — {'; '.join(in_document)}")
    for term in terms:
        if term not in sections:
            body.append(f"License text: see upstream {package_url(dep)} (license: {term})")
    additional = additional_lines(dep, terms)
    if additional:
        body += ["", "Additional requirements / notices:", *additional]
    return f"{RULE}\n{dep.name} {dep.version}\n{RULE}\n" + "\n".join(body) + "\n\n"


def collect_license_texts(to_add, document, explicit: dict[str, str], repo: Path) -> tuple[dict[str, str], set[str]]:
    """Texts to append to Part II, and the ids that will have to keep pointing at upstream.

    A text is only added when it can be named without doubt: either it was handed in with
    `--license-text`, or a package that declares that license alone ships exactly one plausible
    license file. Everything else keeps the document's existing fallback of pointing the reader at
    the upstream repository, which is what the two entries for `notify` and `borrow-or-share` do.
    """
    existing = set(document.license_sections)
    adding: dict[str, str] = {}
    unresolvable: set[str] = set()
    needed: set[str] = set()
    for dep in to_add:
        for term in dep.licenses():
            if term not in existing:
                needed.add(term)
    for license_id in sorted(needed):
        if license_id in explicit:
            adding[license_id] = explicit[license_id]
            continue
        text = None
        for dep in to_add:
            if dep.licenses() != [license_id]:
                continue
            found = license_text_source(dep, license_id, repo)
            if found is not None:
                text = found[1]
                break
        if text is None:
            unresolvable.add(license_id)
        else:
            adding[license_id] = text
    return adding, unresolvable


def prune_dead_sections(text: str) -> str:
    """Drop every Part II text that no entry in `text` asks for.

    Entries come and go with the dependency graph, and a license text left behind after the last entry
    that needed it is a text in a legal document describing nothing this binary contains. The
    reachability rule is `notices_lib.unreachable_sections`, the same one `check-notices-document.py`
    enforces, so what this keeps is exactly what the gate accepts. Deleting a text can remove the prose
    mention that made a second text reachable, so this repeats until the reading is stable.
    """
    while True:
        document = notices.Document(text)
        dead = [document.section_spans[section] for section in notices.unreachable_sections(document)]
        if not dead:
            return text
        pieces: list[str] = []
        cursor = 0
        for start, end in sorted(dead):
            pieces.append(text[cursor:start])
            cursor = end
        pieces.append(text[cursor:])
        pruned = "".join(pieces)
        if pruned == text:
            raise SystemExit(
                "gen-third-party-notices: a Part II license text is unreachable but occupies no "
                "space; Part II does not read back consistently, so nothing was written"
            )
        text = pruned


def line_count(text: str) -> int:
    """How many lines `text` has, counted the way `wc -l` counts them.

    `len(text.splitlines())` is not the same number: it also ends a line at the form feeds that
    some license texts carry (the GNU GPL texts have one per printed page), so the counts this
    tool prints would not match what a reviewer gets from `wc -l` on the same file.
    """
    if not text:
        return 0
    return text.count("\n") + (0 if text.endswith("\n") else 1)


def rebuild(document, kept, new_entries: list[str], added_sections: dict[str, str]) -> str:
    """The whole document: untouched head, Part I in sorted order, Part II plus any new text."""
    text = document.text
    entries = document.entries
    head = text[: entries[0].start]
    tail = text[entries[-1].end:]
    ordered = sorted([*kept, *new_entries], key=lambda block: _block_key(block.splitlines()[1]))
    part1 = head + "".join(ordered) + tail
    if added_sections:
        blocks = [
            f"{CRULER}\n# {license_id}\n{CRULER}\n\n{text.strip()}\n"
            for license_id, text in sorted(added_sections.items())
        ]
        # Part II's last section ends one blank line before the END banner, and a new section is
        # separated from its neighbour the same way; getting this wrong silently changes the
        # spacing of every section after the insertion point, so the anchor is asserted, not hoped.
        marker = f"\n\n{END_BANNER}"
        if not part1.endswith(marker):
            raise SystemExit(
                "gen-third-party-notices: Part II does not end with the END OF THIRD-PARTY NOTICES "
                "banner, so a license text cannot be appended without guessing at the format"
            )
        part1 = part1[: -len(marker)] + "\n\n" + "\n\n".join(blocks) + marker
    return prune_dead_sections(part1)


def _block_key(heading: str) -> tuple[str, tuple]:
    name, _, version = heading.rpartition(" ")
    return (name, notices.version_key(version))


def parse_license_text(values: list[str]) -> dict[str, str]:
    explicit: dict[str, str] = {}
    for value in values:
        license_id, _, raw = value.partition("=")
        path = Path(raw)
        if not raw or not path.is_file():
            raise SystemExit(f"gen-third-party-notices: --license-text {value!r} is not ID=FILE with a readable file")
        explicit[license_id.strip()] = path.read_text(encoding="utf-8")
    return explicit


def plan(repo: Path, explicit: dict[str, str], metadata: dict | None = None) -> tuple[str, dict]:
    """The document as it should read, plus the counts that describe the change.

    Built for `--report` too, because the shape of the change is the useful part of a report: it is
    computed by the same code that writes, so a report cannot describe a change the writer would not
    make.
    """
    document_path = repo / notices.DOCUMENT
    document = notices.Document(document_path.read_text(encoding="utf-8"))
    dependencies = notices.shipped_dependencies(repo, metadata)
    result = notices.triage(document, dependencies)
    drop = {entry.key for entry in result.unshipped}
    kept_blocks = [document.text[entry.start : entry.end] for entry in document.entries if entry.key not in drop]
    kept = {(entry.name, entry.version): entry.body for entry in document.entries if entry.key not in drop}
    to_add = sorted(
        [*result.missing, *[dep for dep, _ in result.drifted]],
        key=lambda dep: (dep.name, dep.version),
    )
    adding, unresolvable = collect_license_texts(to_add, document, explicit, repo)
    sections = set(document.license_sections) | set(adding)
    new_entries = [render_entry(dep, sections, notices.registry_source(dep, repo)) for dep in to_add]
    text = rebuild(document, kept_blocks, new_entries, adding)

    # The rebuild is only trustworthy if the reader agrees about what it produced: every entry that
    # survived must still be one entry with the same body, and every entry that was written must
    # parse back. A body containing a line that reads like an entry heading would split an entry in
    # two here rather than silently in the released document.
    after = notices.Document(text)
    by_key = after.by_key()
    changed = [key for key, body in kept.items() if key not in by_key or by_key[key].body != body]
    extra = {entry.key for entry in after.entries} - set(kept) - {dep.key for dep in to_add}
    missing_new = [dep.key for dep in to_add if dep.key not in by_key]
    # Pruning is span arithmetic over Part II, so say out loud that the texts which stayed kept
    # their bytes; a section eaten from the wrong end would otherwise only surface in review.
    rewritten = [
        section
        for section, body in document.license_sections.items()
        if section in after.license_sections and after.license_sections[section] != body
    ]
    if changed or extra or missing_new or rewritten:
        details = ", ".join(
            [
                f"{len(changed)} existing entr(ies) altered or lost",
                f"{len(missing_new)} written entr(ies) unreadable",
                f"{len(extra)} unexpected entr(ies)",
                f"{len(rewritten)} retained license text(s) altered",
            ]
        )
        raise SystemExit(
            f"gen-third-party-notices: the rebuilt document does not read back as intended ({details}); "
            "nothing was written"
        )
    return text, {
        "document": document,
        "dependencies": dependencies,
        "after": after,
        "drop": drop,
        "to_add": to_add,
        "adding": adding,
        "dropped_sections": sorted(set(document.license_sections) - set(after.license_sections)),
        "unresolvable": unresolvable,
        "refused": result.refused,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", type=Path, default=Path("."), help="repository root")
    parser.add_argument("--write", action="store_true", help="rewrite the notices document")
    parser.add_argument(
        "--license-text",
        action="append",
        default=[],
        metavar="ID=FILE",
        help="text to add to Part II for license id ID, verbatim from FILE",
    )
    parser.add_argument(
        "--metadata",
        default=None,
        type=Path,
        help="a saved `cargo metadata --format-version 1` JSON, in place of running cargo",
    )
    args = parser.parse_args()
    repo = args.repo.resolve()
    if not (repo / notices.DOCUMENT).is_file():
        print(f"gen-third-party-notices: {repo / notices.DOCUMENT} is not a file", file=sys.stderr)
        return 2
    try:
        saved = None
        if args.metadata is not None:
            try:
                saved = json.loads(args.metadata.read_text(encoding="utf-8"))
            except json.JSONDecodeError as exc:
                raise notices.Unreadable(
                    f"--metadata {args.metadata} is not JSON, so no dependency set can be read "
                    f"from it: {exc}") from exc
        text, info = plan(repo, parse_license_text(args.license_text), saved)
    except (notices.Unreadable, OSError, json.JSONDecodeError) as exc:
        print(f"gen-third-party-notices: {exc}", file=sys.stderr)
        return 2
    if info["refused"]:
        for label, message in info["refused"]:
            print(f"gen-third-party-notices: {label}: {message}", file=sys.stderr)
        print(
            "gen-third-party-notices: record the license in scripts/notices_lib.py "
            "(ALLOWED_LICENSES) and add its text to Part II before writing entries",
            file=sys.stderr,
        )
        return 2

    document, after = info["document"], info["after"]
    print(
        f"shipped third-party packages: {len(info['dependencies'])}; "
        f"documented entries: {len(document.entries)} -> {len(after.entries)}"
    )
    print(
        f"entries to drop: {len(info['drop'])}; entries to write: {len(info['to_add'])}; "
        f"Part II texts to add: {len(info['adding'])}; Part II texts to drop: "
        f"{len(info['dropped_sections'])}"
    )
    for license_id, body in sorted(info["adding"].items()):
        print(f"  + Part II {license_id} ({line_count(body)} lines)")
    for license_id in info["dropped_sections"]:
        print(f"  - Part II {license_id} (no remaining entry asks for the text)")
    for license_id in sorted(info["unresolvable"]):
        print(f"  ! Part II {license_id}: no package ships an unambiguous copy of the text")
    for dep in info["to_add"][:10]:
        print(f"  + {dep.name} {dep.version}")
    if len(info["to_add"]) > 10:
        print(f"  + … {len(info['to_add']) - 10} more")
    for entry in sorted(info["drop"], key=lambda key: (key[0], key[1]))[:10]:
        print(f"  - {entry[0]} {entry[1]}")
    if len(info["drop"]) > 10:
        print(f"  - … {len(info['drop']) - 10} more")

    if not args.write:
        print("no file written; pass --write to rewrite THIRD-PARTY-NOTICES")
        return 0
    path = repo / notices.DOCUMENT
    if text == path.read_text(encoding="utf-8"):
        print(f"{notices.DOCUMENT} already matches what is shipped; left untouched")
        return 0
    path.write_text(text, encoding="utf-8")
    print(f"wrote {notices.DOCUMENT}: {line_count(text)} lines")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
