#!/usr/bin/env python3
"""Fail when a committed document points a reader at a file that is not in the repo.

A path in a document is a pointer. `TODO.md` and `CHANGELOG.md` are full of them, and so
are the architecture notes, crate READMEs and the skill reference files. When the file
they name is gone, or never existed, or was never where the text says it is, the reader
has nothing to open and nothing fails. That is the same "green proves nothing" shape as
an unwired guard, one layer down, in prose instead of YAML.

A first pass at this used one regex for "looks like a path" over every line of every
Markdown file. It reported 3,919 hits across 2,266 distinct tokens, and almost none of
them were pointers: `I/O`, `go/no-go`, `tok/s`, `macOS/Windows`, `desktop/mobile`, the
slash command `session/new`, the git branch `sync/curated-port-20260918`, and the
user-home config file `.chaos/config.toml`, which the product creates at runtime and is
not repository content at all. A gate whose first rule has to accommodate three kinds of
exception is no gate, so rather than filter that list by hand, the four rules below make
the noise impossible. What survives was short enough to read one by one, and the
residual that is legitimately unresolvable is written down with a reason.

The rules:

1. Only two shapes count as references: a Markdown link target `[text](path)`, and an
   inline code span whose entire content is a path. Free prose is not scanned, so a
   slash inside a sentence can never be a hit. Fenced code blocks are not scanned
   either: they hold transcripts and shell examples, where `target/release/chaos` and
   `statusline.sh` are things the reader is told to build or to write, not pointers into
   this repository.
2. A link written inside an inline code span is a quoted link, not a link. Markdown
   agrees. `CHANGELOG.md` says the conventions doc carried
   `[CHAOS.md](../../../../CHAOS.md)`, which resolved from neither side, and that it was
   fixed; that sentence is about a dead link, and is not itself one.
3. A pattern is not a path. Anything containing `* ? [ ] < > $ % ~ ...` or the ellipsis
   character, or ending in `-`, describes a shape (`scripts/ci/release-integrity-*.py`,
   `docs/tutorial/01…09-*.md`, `changelogs/X.Y.Z.md`) and is left alone. The one
   exception is a brace group of complete file names, `src/{lib.rs,tests.rs}`, which is
   this repository's house style for citing several files in one span: it is expanded and
   every alternative is checked. If any alternative is not a file name, the span is
   treated as a pattern.
4. A mention has to claim to be inside this repository. Its first path segment, after a
   leading `./` is dropped, must name a top-level entry that has tracked content, and the
   target must resolve, from the citing file's own directory first and then from the
   repository root, to something tracked. That one rule removes the runtime config trees
   (`.chaos/`, `.grok/`, `.claude/`: no tracked files), the build output (`target/`), and
   the crate-relative shorthand that starts a path in the middle of a tree. Link targets
   skip the anchor test: an author who wrote `[text](guide.md)` meant a click to work, so
   a link to a sibling file is checked with no exemption at all.

Resolution order matters more than it looks.
`crates/codegen/xai-grok-pager/README.md` links
`[终端支持与故障排查](docs/user-guide/21-terminal-support.md)`, which is correct, because
the link is relative to that README. A root-anchored checker would call every crate
README broken.

What is enforced:

- Link targets: no exemptions. A click that goes nowhere is a defect wherever it lives,
  and three were present when this gate was written (evidence log
  `docs/verification/doc-path-refs-2026-10-03.log`).
- Inline code mentions: must resolve, or be recorded in
  `scripts/ci/doc-path-refs-allowlist.tsv` as `path<TAB>category<TAB>reason`, with the
  category taken from `CATEGORIES`. Recording is not free: an entry whose path has no
  live dangling hit any more fails as stale, and a missing reason, an unknown category or
  a duplicate path fails outright.

Not covered, stated rather than pretended: references inside fenced code blocks; paths
written with backslashes; a bare name with neither an extension nor a trailing slash,
because `sync/curated-port-20260918` (a branch) and `scripts/hooks` (a directory) are
indistinguishable without a second source of truth; and any path whose first segment is
not a tracked top-level entry, which includes the crate-relative shorthand this repo's
own evidence lines sometimes use (`bin/workspace_server.rs` is checked, because `bin` is
top level, while `xai-grok-workspace/src/bin/workspace_server.rs` is not).

Usage:
    check-doc-path-refs.py [--root DIR] [--allowlist FILE] [--list]
Exit: 0 = every reference resolves or is recorded and still live; 1 = problems;
      2 = the repository or the allowlist could not be read.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

CATEGORIES = (
    "runtime-path",     # the product creates it at runtime; it is not repository content
    "recorded-absent",  # the citing prose itself says the file is missing or removed
    "historical",       # records a state that has since moved or been deleted
    "other-root",       # relative to another root: upstream repo, a crate dir, a user project
    "planned",          # meant to be created later
    "quoted-text",      # the span is text *about* a path, not a pointer to open
)

REASON_MIN = 20

KNOWN_EXT = set(
    """rs md mdx py sh ts tsx js jsx mjs cjs toml json yaml yml log tsv ps1 txt lock nix
    svg png jpg html css service conf diff patch go c h cpp wasm sql
    deb rpm AppImage dmg msi nsis gz br zst asc sha256 sha512 xml xsl plist desktop""".split()
)

# One path segment: word chars, dot, dash, plus, at, and CJK. Names in this repo are
# ASCII; the surrounding prose is bilingual, so a quoted name can sit next to hanzi.
SEG = r"[\w.\-+@\u4e00-\u9fff]"
PATH_RE = re.compile(r"(?:\.{1,2}/)*" + SEG + r"+/(?:" + SEG + r"+/)*" + SEG + r"+/?")
LINK_RE = re.compile(r"(?<!\!)\[[^\]]*\]\(\s*([^)\s]+?)\s*\)")
BACKTICK_RUN = re.compile(r"`+")
BRACE_RE = re.compile(r"^([^{}]*)\{([^{}]*)\}(.*)$")
# A `#` always starts an anchor, whatever script it is written in. Requiring the
# fragment to be ASCII sent `README.md#安装` and
# `user-guide/10-hooks.md#配置文件中的钩子` to the resolver whole, and the gate called
# both links broken; the fixtures found that.
ANCHOR_RE = re.compile(r"#.*$")
LINE_SUFFIX = re.compile(r":\d+(?:-\d+)?$")
FENCE_RE = re.compile(r"^\s{0,3}(`{3,}|~{3,})")

PATTERN_CHARS = ("...", "…", "<", ">", "$", "%", "[", "]", "*", "?", "~")
NOT_A_PATH_PREFIX = ("#", "http://", "https://", "mailto:", "file://", "~", "/", "$")
DOC_SUFFIXES = (".md",)
SKIP_TOPS = {"target", "node_modules", "dist", "test-results", ".git"}


class Repo:
    """The repository's own view of what its content is."""

    def __init__(self, root: Path) -> None:
        self.root = root
        self.files = self._git(["ls-files", "-z"])
        # A file added but not yet staged is still a valid target on a working tree;
        # `--exclude-standard` keeps ignored build output out of that set.
        self.worktree_extra = self._git(["ls-files", "--others", "--exclude-standard", "-z"])
        self.known = self.files | self.worktree_extra
        self.dirs: set[str] = set()
        for f in self.known:
            parts = f.split("/")
            for i in range(1, len(parts)):
                self.dirs.add("/".join(parts[:i]))
        # Only tracked content can *anchor* a reference: `.chaos/` exists on disk in a
        # developer checkout and holds nothing the repository owns.
        self.tops = {f.split("/", 1)[0] for f in self.files}

    def _git(self, args: list[str]) -> set[str]:
        proc = subprocess.run(["git", "-C", str(self.root), *args], capture_output=True, text=True)
        if proc.returncode != 0:
            raise RuntimeError(f"git {' '.join(args)}: {proc.stderr.strip()}")
        return {f for f in proc.stdout.split("\0") if f}

    def docs(self) -> list[Path]:
        return sorted(
            self.root / f
            for f in self.known
            if f.endswith(DOC_SUFFIXES) and f.split("/", 1)[0] not in SKIP_TOPS
        )

    def claims_repo(self, tok: str) -> bool:
        """Does this mention claim to name something inside this repository?

        A leading `./` is stripped first: `./.chaos/skills/`, written in the pager's skill
        docs, means the directory the product runs in, and testing the literal `.` as the
        top-level name would let every runtime path through the anchor test.
        """
        head = tok[2:] if tok.startswith("./") else tok
        head = head.split("/", 1)[0]
        return head in (".", "..") or head in self.tops

    def resolves(self, doc: Path, tok: str) -> bool:
        target = tok.rstrip("/")
        for base in (doc.parent, self.root):
            try:
                norm = (base / target).resolve().relative_to(self.root).as_posix()
            except (OSError, ValueError):
                continue
            if norm in self.known or norm in self.dirs:
                return True
        return False


def lines_of(text: str) -> list[str]:
    """Split on newlines only.

    `str.splitlines()` also breaks on `\\x0b`, `\\x0c`, `\\x85`, U+2028 and U+2029, which
    a bilingual document can carry inside a paragraph. Line numbers reported by a guard
    have to be the numbers `grep -n` prints, or the first job of the guard -- telling
    the reader where to look -- is broken.
    """
    return text.split("\n")


def blank_out(line: str) -> str:
    """Spaces of the same width, so nothing shifts sideways."""
    return " " * len(line)


def strip_fences(text: str) -> str:
    """Blank fenced code blocks out, keeping every surviving character at its position.

    The join is `\\n` for a reason. The first version of this file blanked fenced lines
    with `" " * len(line)` over newline-carrying lines, which ate the newlines too and
    folded 33 lines away in `docs/telemetry-policy.md`; the guard then reported
    `docs/release-process.md` at 116 instead of 149 and sent its reader to the wrong
    place. Splitting first and joining back is what keeps that structurally impossible,
    and `scan` re-checks the count rather than trusting it.
    """
    out: list[str] = []
    fence: str | None = None
    for line in lines_of(text):
        marker = FENCE_RE.match(line)
        if fence is None:
            if marker:
                fence = marker.group(1)[0] * 3
                out.append(blank_out(line))
                continue
            out.append(line)
        else:
            if marker and marker.group(1)[0] * 3 == fence:
                fence = None
            out.append(blank_out(line))
    return "\n".join(out)


def code_spans(line: str) -> list[tuple[int, int, str]]:
    """Inline code spans on one line as (start, end, inner).

    CommonMark does not let a code span cross a line, so a per-line scan is not an
    approximation. An odd trailing run (an unpaired backtick) yields nothing.
    """
    spans: list[tuple[int, int, str]] = []
    pos = 0
    while (m := BACKTICK_RUN.search(line, pos)) is not None:
        tick = m.group(0)
        close = line.find(tick, m.end())
        if close == -1:
            break
        spans.append((m.start(), close + len(tick), line[m.end():close]))
        pos = close + len(tick)
    return spans


def enclosing(spans: list[tuple[int, int, str]], at: int) -> bool:
    return any(start <= at < end for start, end, _ in spans)


def is_pattern(tok: str) -> bool:
    return tok.endswith("-") or any(c in tok for c in PATTERN_CHARS)


def names_a_file(tok: str) -> bool:
    """A file name, or a directory the writer marked with a trailing slash."""
    if tok.endswith("/"):
        return True
    last = tok.rsplit("/", 1)[-1]
    return "." in last and last.rsplit(".", 1)[-1] in KNOWN_EXT


def expand_braces(tok: str) -> list[str] | None:
    """`src/{lib.rs,tests.rs}` to its two paths; None when the braces are not file names.

    A brace group with no comma is a placeholder (`docs/{name}.md`), not a set, and is
    left alone: expanding it would invent a file nobody cited.
    """
    m = BRACE_RE.match(tok)
    if not m:
        return [tok]
    prefix, group, suffix = m.groups()
    if not group or "," not in group:
        return None
    alts = [f"{prefix}{part}{suffix}" for part in group.split(",")]
    return alts if all(names_a_file(a) for a in alts) else None


def strip_target(raw: str) -> str:
    """Drop the anchor and a trailing line range, which are not part of the file name."""
    return LINE_SUFFIX.sub("", ANCHOR_RE.sub("", raw.strip()))


def span_paths(span: str) -> list[str]:
    """The paths a code span claims, but only when the whole span is one path."""
    tok = strip_target(span)
    if not tok or "/" not in tok or tok.startswith(NOT_A_PATH_PREFIX):
        return []
    if is_pattern(tok):
        return []
    alts = expand_braces(tok)
    if alts is None:
        return []
    return [a for a in alts if "/" in a and names_a_file(a)]


def check_link(repo: Repo, doc: Path, lineno: int, target: str) -> str | None:
    """A link target is always a claim.

    Mentions have to pass the top-level anchor test because a backticked token is
    ambiguous: `sync/curated-port-20260918` is a branch and `bin/workspace_server.rs` is
    crate-relative shorthand. A Markdown link is not ambiguous -- the author wrote it so a
    reader could click it -- so a sibling link like `[guide](guide.md)` is checked with no
    anchor test, which is what catches the most common broken link of all.
    """
    target = strip_target(target)
    if not target or target.startswith(NOT_A_PATH_PREFIX) or is_pattern(target):
        return None
    if repo.resolves(doc, target):
        return None
    return (
        f"{doc.relative_to(repo.root).as_posix()}:{lineno}: broken link target "
        f"`{target}` resolves to nothing in the repository"
    )


def check_span(repo: Repo, doc: Path, lineno: int, span: str) -> list[str]:
    out: list[str] = []
    for tok in span_paths(span):
        if not repo.claims_repo(tok) or repo.resolves(doc, tok):
            continue
        out.append((tok, f"{doc.relative_to(repo.root).as_posix()}:{lineno}"))
    return out


def scan(repo: Repo) -> tuple[list[tuple[str, str]], list[str]]:
    """Returns (dangling mentions as (path, location), hard problems)."""
    mentions: list[tuple[str, str]] = []
    problems: list[str] = []
    for doc in repo.docs():
        rel = doc.relative_to(repo.root).as_posix()
        try:
            raw = doc.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as exc:
            problems.append(f"{rel}: unreadable: {exc}")
            continue
        masked = strip_fences(raw)
        if len(lines_of(masked)) != len(lines_of(raw)):
            # The masking is position-preserving by construction; if that ever stops
            # being true, every line number this guard prints is wrong, so say so rather
            # than send a reader to the wrong line.
            problems.append(f"{rel}: internal error: fence masking changed the line count")
            continue
        for lineno, line in enumerate(lines_of(masked), 1):
            spans = code_spans(line)
            for m in LINK_RE.finditer(line):
                if enclosing(spans, m.start()):
                    continue  # rule 2: a quoted link is a mention of a link
                if problem := check_link(repo, doc, lineno, m.group(1)):
                    problems.append(problem)
            for _start, _end, inner in spans:
                for tok, where in check_span(repo, doc, lineno, inner):
                    mentions.append((tok, where))
    return mentions, problems


def load_allowlist(path: Path) -> dict[str, tuple[str, str]]:
    out: dict[str, tuple[str, str]] = {}
    if not path.exists():
        return out
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        cols = line.split("\t")
        if len(cols) < 3:
            raise ValueError(
                f"{path.name}:{lineno}: expected 3 tab-separated columns "
                "(path, category, reason)"
            )
        tok, category, reason = (c.strip() for c in cols[:3])
        if category not in CATEGORIES:
            raise ValueError(
                f"{path.name}:{lineno}: category `{category}` is not one of: "
                f"{', '.join(CATEGORIES)}"
            )
        if len(reason) < REASON_MIN:
            raise ValueError(
                f"{path.name}:{lineno}: reason for `{tok}` is under {REASON_MIN} "
                "characters, which is a note rather than a judgement"
            )
        if tok in out:
            raise ValueError(f"{path.name}:{lineno}: duplicate entry for `{tok}`")
        out[tok] = (category, reason)
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=str(Path(__file__).resolve().parents[2]))
    parser.add_argument("--allowlist", default=None,
                        help="override the recorded-exception table (used by the fixtures)")
    parser.add_argument("--list", action="store_true", dest="list_docs",
                        help="print the documents that would be scanned and exit")
    parser.add_argument("--all", action="store_true",
                        help="print dangling mentions even where they are recorded")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    try:
        repo = Repo(root)
    except RuntimeError as exc:
        print(f"check-doc-path-refs: {exc}", file=sys.stderr)
        return 2

    if args.list_docs:
        for doc in repo.docs():
            print(doc.relative_to(root).as_posix())
        return 0

    allow_path = Path(args.allowlist) if args.allowlist else root / "scripts/ci/doc-path-refs-allowlist.tsv"
    try:
        allow = load_allowlist(allow_path)
    except ValueError as exc:
        print(f"check-doc-path-refs: bad allowlist: {exc}", file=sys.stderr)
        return 2

    dangling, problems = scan(repo)
    counts: dict[str, int] = {}
    for tok, where in dangling:
        counts[tok] = counts.get(tok, 0) + 1
        if tok in allow and not args.all:
            continue
        category = allow[tok][0] if tok in allow else "unrecorded"
        problems.append(f"{where}: `{tok}` resolves to nothing in the repository ({category})")

    for tok in sorted(set(allow) - set(counts)):
        category, reason = allow[tok]
        problems.append(
            f"{allow_path.relative_to(root) if allow_path.is_relative_to(root) else allow_path}: "
            f"recorded `{tok}` ({category}) has no dangling reference left; the text was "
            f"fixed, so the entry is stale. Reason was: {reason}"
        )

    for problem in problems:
        print(f"check-doc-path-refs: {problem}", file=sys.stderr)

    summary = (
        f"check-doc-path-refs: {len(repo.docs())} Markdown documents scanned against "
        f"{len(repo.files)} tracked paths, {len(dangling)} dangling mention(s), "
        f"{len(allow)} recorded"
    )
    if problems:
        print(f"{summary}; {len(problems)} problem(s)", file=sys.stderr)
        return 1
    print(f"{summary}; every link resolves and every dangling mention is recorded and live")
    return 0


if __name__ == "__main__":
    sys.exit(main())
