#!/usr/bin/env python3
"""Machine checks for the Chinese localization of the user guide.

The 27 chapters under `crates/codegen/xai-grok-pager/docs/user-guide/` are
compiled into the binary via `include_str!` (see `pager/src/docs.rs`) and
unpacked to `~/.chaos/docs/user-guide/` on first launch. Translating them
must not disturb anything a reader or a test depends on.

Invariants checked per file, between two git revisions:

  fences   the ordered list of fenced code blocks is byte-identical
  inline   the multiset of inline-code spans is identical
  tables   every table keeps its column count and its row count
  links    the set of link targets is identical
  headings the per-level heading count is identical
  numbers  the multiset of prose numeric literals is identical, so a
           translation cannot silently change a default value

Plus a heuristic for text that is still English:

  english  reports prose lines with no Han characters and >= MIN_WORDS
           ASCII words, excluding code, tables, HTML comments, link
           definitions and lines that are mostly identifiers

Usage:
  scripts/check-doc-l10n.py --before <rev> --after <rev|WORKTREE> [--glob PATH]
  scripts/check-doc-l10n.py --english [--glob PATH]
  scripts/check-doc-l10n.py --links [--glob PATH]

`--english` and `--links` scan the working tree, so a file can be checked
right after it is edited and before it is committed. `--after WORKTREE`
compares a revision against the working tree. Every mode exits non-zero when
a check fails, so this can gate a commit the way `scripts/l10n-guard.sh` does.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

DEFAULT_GLOB = "crates/codegen/xai-grok-pager/docs/user-guide/*.md"

HAN = re.compile(r"[\u4e00-\u9fff]")
FENCE = re.compile(r"^(\s*)(`{3,}|~{3,})")
INLINE = re.compile(r"`([^`\n]+)`")
LINK = re.compile(r"\]\(([^)\s]+)")
HEADING = re.compile(r"^(#{1,6})\s")
WORD = re.compile(r"[A-Za-z][A-Za-z'’-]*")
# Version strings (`0.4.0`) or multi-digit runs (`30`, `800`), but not the
# digits inside an identifier (`sha256`, `utf8`, `v0.4.0` as a bare word).
NUMBER = re.compile(r"(?<![A-Za-z_.])(\d+(?:\.\d+)+|\d{2,})")


def split_lines(text: str) -> list[str]:
    return text.split("\n")


def fenced_blocks(text: str) -> list[str]:
    """Return the body of every fenced code block, in order."""
    blocks: list[str] = []
    current: list[str] | None = None
    marker = ""
    for line in split_lines(text):
        m = FENCE.match(line)
        if current is None:
            if m:
                marker = m.group(2)[0]
                current = []
            continue
        if m and m.group(2)[0] == marker:
            blocks.append("\n".join(current))
            current = None
            continue
        current.append(line)
    if current is not None:
        blocks.append("\n".join(current))  # unterminated: still comparable
    return blocks


def strip_code(text: str) -> str:
    """Blank out fenced blocks so later scans ignore them."""
    out: list[str] = []
    inside = False
    marker = ""
    for line in split_lines(text):
        m = FENCE.match(line)
        if inside:
            if m and m.group(2)[0] == marker:
                inside = False
            out.append("")
            continue
        if m:
            inside = True
            marker = m.group(2)[0]
            out.append("")
            continue
        out.append(line)
    return "\n".join(out)


def inline_spans(text: str) -> Counter[str]:
    return Counter(INLINE.findall(strip_code(text)))


def link_targets(text: str) -> set[str]:
    return set(LINK.findall(strip_code(text)))


def numeric_literals(text: str) -> Counter[str]:
    """Prose numeric literals: default values, sizes, timeouts, versions.

    Fenced blocks are already compared byte-for-byte and inline-code spans as
    a multiset, so both are stripped here; only naked prose digits are left.
    """
    prose = INLINE.sub(" ", strip_code(text))
    prose = re.sub(r"\]\([^)\s]*\)", " ", prose)   # link targets
    prose = re.sub(r"^\s*\[[^\]]+\]:.*$", "", prose, flags=re.MULTILINE)
    return Counter(NUMBER.findall(prose))


def heading_counts(text: str) -> Counter[int]:
    counts: Counter[int] = Counter()
    inside = False
    marker = ""
    for line in split_lines(text):
        m = FENCE.match(line)
        if inside:
            if m and m.group(2)[0] == marker:
                inside = False
            continue
        if m:
            inside = True
            marker = m.group(2)[0]
            continue
        h = HEADING.match(line)
        if h:
            counts[len(h.group(1))] += 1
    return counts


def table_shapes(text: str) -> list[tuple[int, int]]:
    """(columns, rows) for each table, where rows excludes the separator."""
    shapes: list[tuple[int, int]] = []
    cols = 0
    rows = 0
    row_re = re.compile(r"^\s*\|.*\|\s*$")
    sep_re = re.compile(r"^\s*\|[\s:|-]+\|\s*$")
    for line in split_lines(strip_code(text)):
        if row_re.match(line):
            n = line.strip().strip("|").count("|") + 1
            if sep_re.match(line):
                cols = n
                continue
            if cols:
                rows += 1
                continue
        if cols:
            shapes.append((cols, rows))
            cols = 0
            rows = 0
    if cols:
        shapes.append((cols, rows))
    return shapes


def read_rev(rev: str, path: str) -> str | None:
    proc = subprocess.run(
        ["git", "show", f"{rev}:{path}"],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        return None
    return proc.stdout


WORKTREE_ALIASES = {".", "WORKTREE", "worktree"}


def slug(heading_text: str) -> str:
    """GitHub's heading anchor slug: lowercase, punctuation dropped."""
    s = re.sub(r"`([^`]*)`", r"\1", heading_text.strip().lower())
    s = re.sub(r"[^\w\s-]", "", s, flags=re.UNICODE)
    return re.sub(r"\s+", "-", s)


def heading_list(text: str) -> list[str]:
    """Heading slugs in document order, fenced blocks excluded."""
    inside = False
    marker = ""
    found: list[str] = []
    for line in split_lines(text):
        m = FENCE.match(line)
        if inside:
            if m and m.group(2)[0] == marker:
                inside = False
            continue
        if m:
            inside = True
            marker = m.group(2)[0]
            continue
        h = HEADING.match(line)
        if h:
            found.append(slug(line[h.end():]))
    return found


def heading_slugs(text: str) -> set[str]:
    found = set(heading_list(text))
    # Explicit anchors also satisfy a link.
    found.update(re.findall(r"\{#([^}]+)\}", text))
    found.update(re.findall(r"<a\s+[^>]*id=\"([^\"]+)\"", text))
    return found


def check_links(glob: str) -> int:
    """Every `](file.md#anchor)` must resolve, in the worktree."""
    paths = expand(glob, None)
    slugs = {p: heading_slugs(Path(p).read_text(encoding="utf-8")) for p in paths}
    broken = 0
    for path in paths:
        text = strip_code(Path(path).read_text(encoding="utf-8"))
        for target in sorted(link_targets(text)):
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            file_part, _, anchor = target.partition("#")
            if not file_part:
                resolved = path
            else:
                cand = Path(path).parent / file_part
                alt = Path(file_part)
                if cand.exists():
                    resolved = str(cand)
                elif alt.exists():
                    resolved = str(alt)
                else:
                    print(f"{path}: missing link target -> {target}")
                    broken += 1
                    continue
            if anchor and resolved in slugs and anchor not in slugs[resolved]:
                print(f"{path}: dead anchor -> {target}")
                broken += 1
    print(f"\n{broken} broken link(s)")
    return 1 if broken else 0


def fix_anchors(before: str, glob: str) -> int:
    """Rewrite inbound `file.md#anchor` links after headings are translated.

    Translation preserves heading count and order (the `headings` invariant),
    so the i-th heading before maps to the i-th heading after. That gives an
    old-slug -> new-slug table per file with no guessing. Anchors that exist
    in neither revision are reported for a human to resolve.
    """
    paths = expand(glob, None)
    mapping: dict[str, dict[str, str]] = {}
    for path in paths:
        old = read_rev(before, path)
        if old is None:
            continue
        olds = heading_list(old)
        news = heading_list(Path(path).read_text(encoding="utf-8"))
        if len(olds) != len(news):
            print(f"{path}: heading count {len(olds)} -> {len(news)}, skipped")
            continue
        mapping[path] = dict(zip(olds, news))

    touched_files = 0
    rewrites = 0
    unresolved: list[str] = []

    def rewrite(path: str, line: str) -> str:
        nonlocal rewrites

        def repl(m: re.Match[str]) -> str:
            nonlocal rewrites
            file_part, _, anchor = m.group(1).partition("#")
            if not anchor:
                return m.group(0)
            resolved = str(Path(path).parent / file_part) if file_part else path
            table = mapping.get(resolved)
            if table is None:
                return m.group(0)
            new_anchor = table.get(anchor)
            if new_anchor is None:
                unresolved.append(f"{path} -> {m.group(1)}")
            elif new_anchor != anchor:
                rewrites += 1
                return f"]({file_part}#{new_anchor})"
            return m.group(0)

        return LINK.sub(repl, line)

    for path in paths:
        p = Path(path)
        original = p.read_text(encoding="utf-8")
        out: list[str] = []
        inside = False
        marker = ""
        for line in split_lines(original):
            m = FENCE.match(line)
            if inside:
                if m and m.group(2)[0] == marker:
                    inside = False
                out.append(line)
                continue
            if m:
                inside = True
                marker = m.group(2)[0]
                out.append(line)
                continue
            out.append(rewrite(path, line))
        updated = "\n".join(out)
        if updated != original:
            p.write_text(updated, encoding="utf-8")
            touched_files += 1

    for item in sorted(set(unresolved)):
        print(f"unresolved anchor: {item}")
    print(f"\n{rewrites} anchor(s) rewritten in {touched_files} file(s), "
          f"{len(set(unresolved))} unresolved")
    return 1 if unresolved else 0


def compare(before: str, after: str, path: str) -> list[str]:
    problems: list[str] = []
    if fenced_blocks(before) != fenced_blocks(after):
        problems.append(
            "fenced code blocks changed "
            f"({len(fenced_blocks(before))} -> {len(fenced_blocks(after))})"
        )
    if inline_spans(before) != inline_spans(after):
        lost = inline_spans(before) - inline_spans(after)
        added = inline_spans(after) - inline_spans(before)
        detail = []
        if lost:
            detail.append(f"lost {sorted(lost.elements())[:8]}")
        if added:
            detail.append(f"added {sorted(added.elements())[:8]}")
        problems.append("inline-code spans changed: " + "; ".join(detail))
    if table_shapes(before) != table_shapes(after):
        problems.append(
            f"table shapes changed ({table_shapes(before)} -> {table_shapes(after)})"
        )
    if link_targets(before) != link_targets(after):
        lost = sorted(link_targets(before) - link_targets(after))
        added = sorted(link_targets(after) - link_targets(before))
        problems.append(f"link targets changed: lost {lost}, added {added}")
    if heading_counts(before) != heading_counts(after):
        problems.append(
            f"heading counts changed ({dict(heading_counts(before))} "
            f"-> {dict(heading_counts(after))})"
        )
    if numeric_literals(before) != numeric_literals(after):
        lost = numeric_literals(before) - numeric_literals(after)
        added = numeric_literals(after) - numeric_literals(before)
        detail = []
        if lost:
            detail.append(f"lost {sorted(lost.elements())[:12]}")
        if added:
            detail.append(f"added {sorted(added.elements())[:12]}")
        problems.append("prose numeric literals changed: " + "; ".join(detail))
    return problems


SKIP_PROSE = (
    re.compile(r"^\s*$"),
    re.compile(r"^\s*<!--"),
    re.compile(r"^\s*\|"),
    re.compile(r"^\s*\[[^\]]+\]:"),
    re.compile(r"^\s*[-*+]\s+`|^\s*\d+\.\s+`$"),
)


def english_lines(text: str, min_words: int) -> list[tuple[int, str]]:
    hits: list[tuple[int, str]] = []
    for i, line in enumerate(split_lines(strip_code(text)), start=1):
        if HAN.search(line):
            continue
        if any(p.match(line) for p in SKIP_PROSE):
            continue
        stripped = INLINE.sub(" ", line)
        words = WORD.findall(stripped)
        if len(words) < min_words:
            continue
        # Require real sentence structure, not a list of identifiers.
        if not re.search(r"\b(the|a|an|is|are|to|of|for|and|or|with|when|if|"
                         r"you|your|this|that|it|in|on|by|from|not|do|does)\b",
                         stripped, re.IGNORECASE):
            continue
        hits.append((i, line.strip()))
    return hits


def expand(pattern: str, rev: str | None) -> list[str]:
    """Resolve a glob to repo-relative paths, at a revision or in the worktree."""
    if rev is None:
        return sorted(
            str(p) for p in Path().glob(pattern) if p.is_file()
        )
    proc = subprocess.run(
        ["git", "ls-tree", "-r", "--name-only", rev],
        capture_output=True,
        text=True,
        check=True,
    )
    root = pattern.split("*")[0]
    return sorted(
        line
        for line in proc.stdout.split("\n")
        if line.startswith(root) and line.endswith(".md")
    )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--before", help="git revision to compare from")
    ap.add_argument("--after", help="git revision to compare to")
    ap.add_argument("--glob", default=DEFAULT_GLOB, help="path glob")
    ap.add_argument("--english", action="store_true", help="scan worktree prose")
    ap.add_argument("--links", action="store_true",
                    help="verify internal file/anchor links in the worktree")
    ap.add_argument("--fix-anchors", action="store_true",
                    help="rewrite inbound anchors for retitled headings "
                         "(needs --before)")
    ap.add_argument("--min-words", type=int, default=6)
    args = ap.parse_args()

    if args.fix_anchors:
        if not args.before:
            ap.error("--fix-anchors requires --before")
        return fix_anchors(args.before, args.glob)

    if args.links:
        return check_links(args.glob)

    if args.english:
        total = 0
        for path in expand(args.glob, None):
            hits = english_lines(Path(path).read_text(encoding="utf-8"),
                                 args.min_words)
            if hits:
                total += len(hits)
                print(f"{path}: {len(hits)} English prose line(s)")
                for n, line in hits[:12]:
                    print(f"  {n}: {line[:110]}")
        print(f"\n{total} English prose line(s) remaining")
        return 1 if total else 0

    if not args.before or not args.after:
        ap.error("--before and --after are required unless --english/--links")

    worktree = args.after in WORKTREE_ALIASES
    failures = 0
    for path in expand(args.glob, None if worktree else args.after):
        before = read_rev(args.before, path)
        if before is None:
            continue
        if worktree:
            p = Path(path)
            after = p.read_text(encoding="utf-8") if p.is_file() else None
        else:
            after = read_rev(args.after, path)
        if after is None:
            continue
        problems = compare(before, after, path)
        if problems:
            failures += 1
            print(f"{path}")
            for p in problems:
                print(f"  - {p}")
    print(f"\n{failures} file(s) with structural drift")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
