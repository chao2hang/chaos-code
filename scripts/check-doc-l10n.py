#!/usr/bin/env python3
"""Machine checks for the Chinese localization of the user guide.

The 27 chapters under `crates/codegen/xai-grok-pager/docs/user-guide/` are
compiled into the binary via `include_str!` (see `pager/src/docs.rs`) and
unpacked to `~/.chaos/docs/user-guide/` on first launch. Translating them
must not disturb anything a reader or a test depends on.

Invariants checked per file, between two git revisions:

  fences   the ordered list of fenced code blocks is identical
  inline   the multiset of inline-code spans is identical
  tables   every table keeps its declared column count and the cell count of
           each data row
  links    the set of link targets is identical
  headings the per-level heading count is identical
  numbers  the multiset of prose numeric literals is identical, so a
           translation cannot silently change a default value

`fences` and `inline` compare *canonicalised* text: the fork renamed the
binary to `chaos` and its native config dir to `.chaos`, so docs must use the
fork names. `fork_normalize` maps the upstream spelling onto the fork
spelling on both sides, which accepts either form (a translator may keep a
legacy literal or localise it) while still rejecting anything invented. Use
`--fork-names` to see which literals are still upstream-spelled.

For inline spans, *losing* one is drift (that is what catches a mangled
identifier); *adding* one is only reported, because a verified content
correction may introduce a literal the upstream text never named. Pass
`--strict-spans` to treat additions as drift too.

Plus two heuristics for localization residue:

  english    reports prose lines with no Han characters and >= MIN_WORDS
             ASCII words, excluding code, tables, HTML comments, link
             definitions and lines that are mostly identifiers
  fork-names reports `grok` used as a command or a config path (prose *and*
             code), i.e. names the fork has since renamed. Legacy `GROK_*`
             env vars, `xai-grok-*` crates, `grok-<model>` ids, `grok.com`
             and `/etc/grok` are allowlisted; a line that explicitly discusses
             compatibility may name the legacy path.

Usage:
  scripts/check-doc-l10n.py --before <rev> --after <rev|WORKTREE> [--glob PATH]
  scripts/check-doc-l10n.py --english [--glob PATH]
  scripts/check-doc-l10n.py --fork-names [--strict] [--glob PATH]
  scripts/check-doc-l10n.py --links [--glob PATH]

`--english`, `--fork-names` and `--links` scan the working tree, so a file can
be checked right after it is edited and before it is committed. `--after
WORKTREE` compares a revision against the working tree. Every mode exits
non-zero when a check fails, so this can gate a commit the way
`scripts/l10n-guard.sh` does; `--fork-names` needs `--strict` to do so.
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

# Fork-localization renames. The fork renamed the binary to `chaos` and the
# native home/project directory to `.chaos` (see `xai-dirs/src/lib.rs` and
# `xai-grok-config/src/paths.rs`); documentation must spell these the fork
# way. Deliberately narrow: `GROK_*` env vars (only `GROK_HOME` got a
# `CHAOS_HOME` twin), model ids (`grok-4.5`), crate names (`xai-grok-pager`)
# and the system config dir `/etc/grok` are unchanged facts and must survive.
RENAMES = (
    # `grok inspect`, `grok -c`: a command word, so a following space.
    (re.compile(r"(?<![\w./-])grok(?=[ \t])"), "chaos"),
    # A bare `grok` (a whole span or a line by itself).
    (re.compile(r"(?<![\w./-])grok(?![\w./-])"), "chaos"),
    # `~/.grok` -> `~/.chaos`; `.grok/rules` -> `.chaos/rules`; `.grok` alone.
    (re.compile(r"~/\.grok(?![\w-])"), "~/.chaos"),
    (re.compile(r"(?<![\w.-])\.grok(?![\w-])"), ".chaos"),
    # The one environment variable that gained a Chaos twin.
    (re.compile(r"\bGROK_HOME\b"), "CHAOS_HOME"),
)


def fork_normalize(text: str) -> str:
    """Canonicalise upstream spellings to fork spellings for comparison."""
    for pattern, repl in RENAMES:
        text = pattern.sub(repl, text)
    return text


# Upstream spellings that should no longer appear: a `grok` command word, a
# bare `grok`, or a `~/.grok` / `.grok` config path.
FORK_NAME = re.compile(
    r"(?<![\w./-])grok(?=[ \t])"
    r"|(?<![\w./-])grok(?![\w./-])"
    r"|~/\.grok(?![\w-])"
    r"|(?<![\w.-])\.grok(?![\w-])"
)


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


def table_shapes(text: str) -> list[tuple[int, tuple[int, ...]]]:
    """(separator columns, cell count of each data row) for each table.

    The separator row gives the declared column count; each data row's own
    cell count is recorded too, so a row that gains or loses a cell (the usual
    slip when translating table text) is caught even though the separator is
    untouched.
    """
    shapes: list[tuple[int, tuple[int, ...]]] = []
    cols = 0
    rows: list[int] = []
    row_re = re.compile(r"^\s*\|.*\|\s*$")
    sep_re = re.compile(r"^\s*\|[\s:|-]+\|\s*$")
    for line in split_lines(strip_code(text)):
        if row_re.match(line):
            n = line.strip().strip("|").count("|") + 1
            if sep_re.match(line):
                cols = n
                continue
            if cols or rows:
                rows.append(n)
                continue
        if cols or rows:
            shapes.append((cols, tuple(rows)))
            cols = 0
            rows = []
    if cols or rows:
        shapes.append((cols, tuple(rows)))
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


def compare(before: str, after: str, path: str,
            strict_spans: bool = False) -> tuple[list[str], list[str]]:
    """Return (problems, notes).

    Problems are structural damage. Notes are visible-but-tolerated changes:
    a deliberately introduced literal (for example documenting a command that
    did not exist in the upstream text) adds an inline span without removing
    one. A *changed* span still registers as a loss, which is what catches a
    mangled identifier, so tolerating additions does not weaken that.
    """
    problems: list[str] = []
    notes: list[str] = []
    # Fences and inline spans are compared canonicalised: a fork rename that
    # a translator applied (or deliberately did not) is not drift.
    before_fences = [fork_normalize(b) for b in fenced_blocks(before)]
    after_fences = [fork_normalize(b) for b in fenced_blocks(after)]
    if before_fences != after_fences:
        problems.append(
            "fenced code blocks changed "
            f"({len(before_fences)} -> {len(after_fences)})"
        )
    before_inline = Counter(fork_normalize(s) for s in inline_spans(before).elements())
    after_inline = Counter(fork_normalize(s) for s in inline_spans(after).elements())
    lost = before_inline - after_inline
    added = after_inline - before_inline
    if lost:
        problems.append(
            "inline-code spans lost: " + str(sorted(lost.elements())[:8])
        )
    if added:
        message = "inline-code spans added: " + str(sorted(added.elements())[:8])
        (problems if strict_spans else notes).append(message)
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
    return problems, notes


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


# Names the fork has not renamed: legacy env vars, crate/package names, model
# ids, the upstream service, and the system config dir.
FORK_NAME_ALLOW = re.compile(
    r"GROK_[A-Z0-9_]+"          # env vars keep their names
    r"|xai-grok[\w-]*"          # crate and package names
    r"|grok-[0-9][\w.-]*"       # model ids (`grok-4.5`, `grok-3-mini`)
    r"|grok\.com|auth\.x\.ai"   # upstream service, not the local binary
    r"|/etc/grok"               # system config dir, `.chaos` twin does not exist
)
# A line that explicitly discusses the dual-read compatibility policy is
# allowed to name the legacy path: that is the one place it belongs.
COMPAT_LINE = re.compile(r"兼容")


def fork_name_hits(text: str) -> list[tuple[int, str]]:
    """Lines still using an upstream name the fork has since renamed.

    The compatibility exemption is applied per *paragraph*: a wrapped sentence
    can put the legacy path on a different line than the word 「兼容」, so
    testing line by line would report a legitimate note as residue.
    """
    lines = split_lines(text)
    # Paragraph = a run of consecutive non-blank lines. Collect every
    # paragraph that mentions compatibility, then scan; the legacy mention may
    # come before the word 「兼容」 within the same paragraph.
    compat_para: set[int] = set()
    para = 0
    in_para = False
    for line in lines:
        if not line.strip():
            in_para = False
            continue
        if not in_para:
            para += 1
            in_para = True
        if COMPAT_LINE.search(line):
            compat_para.add(para)

    hits: list[tuple[int, str]] = []
    para = 0
    in_para = False
    for i, line in enumerate(lines, start=1):
        if not line.strip():
            in_para = False
            continue
        if not in_para:
            para += 1
            in_para = True
        if para in compat_para:
            continue
        # Blank allowlisted occurrences, then look for what is left.
        residue = FORK_NAME_ALLOW.sub(lambda m: " " * len(m.group(0)), line)
        if FORK_NAME.search(residue):
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
    ap.add_argument("--fork-names", action="store_true",
                    help="report worktree lines still using an upstream name "
                         "the fork renamed (`grok <cmd>`, `~/.grok`, `.grok/`)")
    ap.add_argument("--strict", action="store_true",
                    help="with --fork-names: exit non-zero when any remain")
    ap.add_argument("--strict-spans", action="store_true",
                    help="treat an added inline-code span as drift, not a note")
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

    if args.fork_names:
        total = 0
        for path in expand(args.glob, None):
            hits = fork_name_hits(Path(path).read_text(encoding="utf-8"))
            if hits:
                total += len(hits)
                print(f"{path}: {len(hits)} upstream name(s)")
                for n, line in hits[:12]:
                    print(f"  {n}: {line[:110]}")
        print(f"\n{total} upstream name(s) remaining")
        return 1 if (total and args.strict) else 0

    if not args.before or not args.after:
        ap.error("--before and --after are required unless --english/--links")

    worktree = args.after in WORKTREE_ALIASES
    failures = 0
    noted = 0
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
        problems, notes = compare(before, after, path, args.strict_spans)
        if problems or notes:
            print(f"{path}")
            for item in problems:
                print(f"  - {item}")
            for item in notes:
                print(f"  note: {item}")
        if problems:
            failures += 1
        noted += len(notes)
    print(f"\n{failures} file(s) with structural drift"
          + (f", {noted} tolerated addition(s)" if noted else ""))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
