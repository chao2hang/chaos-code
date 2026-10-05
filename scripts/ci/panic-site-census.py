#!/usr/bin/env python3
"""Census of panic-capable and unsafe sites in the Rust sources, production apart.

`docs/audit-followup-report.md` has carried a production-unwrap table measured by
hand in 2026-08. Every governance batch since then has removed sites, and the note
under each of them said the same thing: the count has to be taken from the current
source, not inherited. This is that measurement, so the next batch inherits a
number it can re-run instead of one it has to trust.

What counts as production: a `.rs` file under a crate's `src/`, minus the spans the
file itself marks as test. A span is test when

  * the file is under `tests/`, `benches/` or `examples/`, or begins `#![cfg(test)]`;
  * it sits inside an item whose `cfg` expression *requires* `test`, including a
    `mod tests { ... }` found that way. `all` inherits the requirement from any
    part and `any` only from every part, so `all(unix, test)` is a test gate while
    `any(target_os = "linux", all(unix, test))` is not;
  * it sits in a file pulled in by a `#[cfg(test)] mod name;` declaration, which is
    how `src/handle_tests.rs` and friends are reached.

Anything the scanner cannot attribute is reported as production. That direction is
deliberate: an uncounted test panic makes the audit look better than it is, and a
counted one only costs whoever reads it a look at the file.

Limits, stated because they change the numbers: a raw string holding Rust source
(`r#"..."#`) is blanked like any string, so an `.unwrap()` written inside one is
invisible here; a `mod tests` without a `cfg(test)` attribute is counted as
production, which is what it looks like to the compiler as well. A backslash
escapes the next character inside a non-raw literal, which matters because a
literal ending in `\"` would otherwise end at the escaped quote and hand its real
terminator to the next literal, taking the `cfg(test)` attribute in between with
it.

    scripts/ci/panic-site-census.py                  # tables, sorted by production unwraps
    scripts/ci/panic-site-census.py --json           # the same rows as JSON
    scripts/ci/panic-site-census.py --write-baseline scripts/ci/panic-site-baseline.tsv
    scripts/ci/panic-site-census.py                  # exits 1 if a crate got worse
"""

import argparse
import importlib.util
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
# Shared with `check-dead-cfg.py`; one directory up, like `scripts/notices_lib.py`,
# because a file in `scripts/ci/` has to be a gate something runs.
_CFG_LIB = HERE.parent / "cfg_lib.py"
if not _CFG_LIB.is_file():
    raise SystemExit(f"panic-site-census: {_CFG_LIB} is missing, so nothing here can be checked")
_cfg_spec = importlib.util.spec_from_file_location("cfg_lib", _CFG_LIB)
cfg_lib = importlib.util.module_from_spec(_cfg_spec)
assert _cfg_spec.loader is not None
_cfg_spec.loader.exec_module(cfg_lib)

# The opening of a `cfg` attribute. The argument list is read by balancing
# parentheses from here, because `cfg(all(test, not(unix)))` nests and a
# character-class regex cannot see the `test` inside it.
CFG_OPEN = re.compile(r"#\s*!?\s*\[\s*cfg(?:_attr)?\s*\(")
# Reading a `cfg(...)` predicate is not done here: `scripts/cfg_lib.py` owns it, because
# `scripts/ci/check-dead-cfg.py` has to reach the same verdict about the same text.
# A whole character literal: `'}'`, `'\n'`, `'\u{1f600}'`, `'\''`. Anything else
# starting with `'` is a lifetime.
CHAR_LITERAL = re.compile(r"'(?:\\.|[^\\'])'|'\\u\{[0-9a-fA-F]+\}'")

UNWRAP = re.compile(r"\.unwrap\s*\(")
EXPECT = re.compile(r"\.expect\s*\(")
PANIC = re.compile(r"\b(?:panic|unreachable|todo|unimplemented)\s*!")
UNSAFE_BLOCK = re.compile(r"\bunsafe\s*\{")
UNSAFE_FN = re.compile(r"\bunsafe\s+fn\b")
UNSAFE_IMPL = re.compile(r"\bunsafe\s+impl\b")
UNSAFE_EXTERN = re.compile(r"\bunsafe\s+extern\b")
MOD_DECL = re.compile(r"\bmod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;")
# `#[path = "..."]` is read from the original text, not the blanked one, because
# blanking erases the file name. Offsets survive blanking, so a window located in
# the blanked text can be sliced out of the original at the same positions.
PATH_ATTR = re.compile(r'#\[path\s*=\s*"([^"]+)"\s*\]')
ONLY_INNER_ATTRS = re.compile(r"(?:\s*#!\[[^\]]*\]\s*)*", re.S)
# `[[bin]] path = "src/bin/cli.rs"` and friends: Cargo names compilation units
# outside its conventions all over this workspace, and each of those is a root.
CARGO_TARGET_PATH = re.compile(r'path\s*=\s*"([^"]+\.rs)"')
# Directories whose compilation units are never in a released binary.
NON_SHIPPING_DIRS = ("tests", "benches", "examples")

# What to look for: the pattern, the key it adds to, and the key for the part of it
# that survives once the file's `cfg(test)` spans have been taken out. `unsafe` is
# reported four ways because the four carry different obligations, and the
# production figure is one number across all of them.
SITES = [
    (UNWRAP, "unwrap", "unwrap_prod"),
    (EXPECT, "expect", "expect_prod"),
    (PANIC, "panic", "panic_prod"),
    (UNSAFE_BLOCK, "unsafe_block", "unsafe_prod"),
    (UNSAFE_FN, "unsafe_fn", "unsafe_prod"),
    (UNSAFE_IMPL, "unsafe_impl", "unsafe_prod"),
    (UNSAFE_EXTERN, "unsafe_extern", "unsafe_prod"),
]


def blank_noise(source: str) -> str:
    """Blank out comments and string literals, keeping every byte position.

    Positions are kept so that offsets found here can be compared with offsets in
    the original text, which is what the test-span logic works in.
    """
    out = list(source)
    i = 0
    n = len(source)

    def blank(start: int, stop: int) -> None:
        for j in range(start, stop):
            if out[j] != "\n":
                out[j] = " "

    while i < n:
        two = source[i : i + 2]
        if two == "//":
            end = source.find("\n", i)
            end = n if end < 0 else end
            blank(i, end)
            i = end
        elif two == "/*":
            depth, j = 1, i + 2
            while j < n and depth:
                if source.startswith("/*", j):
                    depth += 1
                    j += 2
                elif source.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            blank(i, j)
            i = j
        else:
            # `r#"..."#`, `b"..."`, `br##"..."##` and plain `"..."` all start a
            # literal at the quote; only the prefix tells us how it ends.
            quote = source.find('"', i, i + 4)
            starts_literal = quote > i and all(c in "rb#" for c in source[i:quote])
            if source[i] == '"' or starts_literal:
                first = quote if starts_literal else i
                hashes = 0
                k = first - 1
                while k >= 0 and source[k] == "#":
                    hashes += 1
                    k -= 1
                terminator = '"' + "#" * hashes
                # A `\"` inside a literal is not the end of it. Stopping at the
                # first quote would leave the rest of the literal in the scanned
                # text, where a `{` or `}` in a message unbalances the brace walk
                # that closes a test module.
                raw = "r" in source[i:first]
                j = first + 1
                end = -1
                while j < n:
                    if source.startswith(terminator, j):
                        end = j + len(terminator)
                        break
                    if not raw and source[j] == "\\":
                        j += 2
                        continue
                    j += 1
                if end < 0:
                    end = n
                blank(i, end)
                i = end
            else:
                # `'{'` and `'}'` are characters, not braces: left alone they
                # unbalance the brace walk that closes a test module. A `'` that
                # does not open a whole character literal is a lifetime.
                if source[i] == "'":
                    literal = CHAR_LITERAL.match(source, i)
                    if literal:
                        blank(i, literal.end())
                        i = literal.end()
                        continue
                i += 1
    return "".join(out)


def brace_end(clean: str, open_brace: int) -> int:
    """Index just past the `}` matching the `{` at `open_brace`."""
    depth = 0
    for i in range(open_brace, len(clean)):
        if clean[i] == "{":
            depth += 1
        elif clean[i] == "}":
            depth -= 1
            if depth == 0:
                return i + 1
    return len(clean)


def module_files(host: Path, name: str, at_crate_root: bool) -> list[Path]:
    """Where `mod name;` inside `host` looks for its file.

    The answer depends on whether `host` is itself a compilation unit: a crate root
    keeps its submodules beside it (`src/main.rs` + `mod x;` is `src/main/x.rs`'s
    *sibling* `src/x.rs`), while an ordinary module gets a directory of its own
    (`src/foo.rs` + `mod x;` is `src/foo/x.rs`). Guessing this from the file name
    alone is what makes a `src/bin/tool.rs` submodule look uncompiled.
    """
    if at_crate_root or host.stem == "mod":
        base = host.parent
    else:
        base = host.parent / host.stem
    return [base / f"{name}.rs", base / name / "mod.rs"]


def declared_path(text: str, window_start: int, decl_start: int) -> str | None:
    """The file a `#[path = "..."] mod name;` names, if it carries one.

    This repository writes test modules the second way almost everywhere —
    `#[path = "handle_tests.rs"] mod tests;` — so a rule that only knows the
    `mod name;` convention files would call most of its test code production.
    The window is the text between the previous item and the declaration.
    """
    found = None
    for match in PATH_ATTR.finditer(text, window_start, decl_start):
        found = match.group(1)
    return found


def cfg_attribute(clean: str, open_at: int) -> tuple[str, int] | None:
    """The argument text of the `cfg(...)` starting at `open_at`, and where its `]` is.

    Returns `None` for an attribute malformed enough to have no closing bracket,
    which the caller treats as "no span" rather than guessing at one.
    """
    depth = 0
    index = clean.index("(", open_at)
    start = index + 1
    while index < len(clean):
        char = clean[index]
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0:
                break
        index += 1
    else:
        return None
    args = clean[start:index]
    closing = clean.find("]", index)
    if closing < 0:
        return None
    return args, closing + 1


def gated_by_test(args: str) -> bool:
    """Does a `cfg(...)` argument list apply only to a test build?

    `scripts/cfg_lib.py` answers it, and its docstring carries the reasoning: the question is
    whether `test` is *required*, not whether the word appears.
    """
    return cfg_lib.requires_test(args)


def dead_by_cfg(args: str) -> bool:
    """Is this predicate false in every build of every target, so rustc drops the item?

    An unreadable predicate is answered "no" on purpose: the census's blind direction is to count
    text as production (see the module docstring), and `check-dead-cfg.py` is the gate that
    refuses to read a predicate it cannot fold.
    """
    try:
        return cfg_lib.never_holds(args)
    except ValueError:
        return False


def cfg_item_spans(clean: str, keep) -> list[tuple[int, int]]:
    """Spans of the items whose `cfg(...)` predicate `keep` accepts.

    A `#[cfg(test)] mod x;` declaration gets a span over its own text too, so the
    caller can find it the same way it finds a declaration nested in a test module.
    """
    spans: list[tuple[int, int]] = []
    for opening in CFG_OPEN.finditer(clean):
        if "_attr" in opening.group(0):
            # `cfg_attr(cond, attrs)` always builds the item and only swaps the
            # listed attributes in or out, so it never puts anything under the
            # test cfg, whatever the condition says.
            continue
        read = cfg_attribute(clean, opening.start())
        if read is None:
            continue
        args, after = read
        if not keep(args):
            continue
        head = clean[: opening.start()]
        if "!" in opening.group(0):
            # `#![cfg(test)]` at the top of a file, past any other inner attribute.
            if ONLY_INNER_ATTRS.fullmatch(head):
                return [(0, len(clean))]
            continue
        # The item the attribute applies to: everything up to its first `{`, or a
        # `;` if it is a declaration of something that lives in another file.
        brace = clean.find("{", after)
        semicolon = clean.find(";", after)
        if semicolon >= 0 and (brace < 0 or semicolon < brace):
            spans.append((after, semicolon + 1))
            continue
        if brace < 0:
            continue
        spans.append((after, brace_end(clean, brace)))
    return spans


def test_spans(clean: str) -> list[tuple[int, int]]:
    """Ranges of `clean` that a `cfg(test)` attribute puts under the test cfg."""
    return cfg_item_spans(clean, gated_by_test)


def never_spans(clean: str) -> list[tuple[int, int]]:
    """Ranges that a provably-false `cfg` removes from *every* build.

    `#[cfg(any())]` is false whatever the target, the feature tree or the profile, so rustc drops
    the item before name resolution. That text is neither production nor test code: it is
    compiled by nobody. Counting it as production is what happened here for ten weeks -- 7
    `.unwrap()`s in a dropped `mod tests` were reported as production panic sites in
    `xai-grok-shell`, and the tests inside them appeared in no test run and no ledger.
    """
    return cfg_item_spans(clean, dead_by_cfg)


def crate_dir_of(path: Path, root: Path) -> Path:
    """The directory whose `Cargo.toml` owns this file."""
    for parent in path.parents:
        if parent == root:
            return root
        if (parent / "Cargo.toml").exists():
            return parent
    return root


def crate_of(path: Path, root: Path) -> str:
    """The crate a source file belongs to: the nearest ancestor with a Cargo.toml."""
    for parent in path.parents:
        if parent == root:
            break
        if (parent / "Cargo.toml").exists():
            return parent.name
    return "<outside the workspace>"


def is_test_by_location(path: Path, root: Path) -> bool:
    rel = path.relative_to(root).parts
    return any(part in ("tests", "benches", "examples") for part in rel)


def sources(root: Path) -> list[Path]:
    return sorted(
        path
        for crate_dir in sorted((root / "crates").iterdir())
        for path in crate_dir.rglob("*.rs")
        if path.is_file() and "__pycache__" not in path.parts
    )


def census(root: Path) -> dict[str, dict[str, int]]:
    """Per-crate counts of panic-capable and unsafe sites, production and total."""
    counts, _, _ = measure(root)
    return counts


KINDS = {
    "unwrap": ".unwrap()",
    "expect": ".expect()",
    "panic": "panic!/unreachable!/todo!/unimplemented!",
    "unsafe_block": "unsafe block",
    "unsafe_fn": "unsafe fn",
    "unsafe_impl": "unsafe impl",
    "unsafe_extern": "unsafe extern",
}


def module_declarations(
    clean: str,
    text: str,
    path: Path,
    spans: list[tuple[int, int]],
    at_crate_root: bool,
    dead: list[tuple[int, int]] = (),
) -> tuple[list[Path], list[Path]]:
    """Files this one brings in, split into test-gated and ordinarily compiled.

    Two shapes matter for the test half. A `#[cfg(test)] mod x;` declaration gates
    its target directly, and a `mod x;` written inside a `mod tests { ... }` block
    inherits the gate from the block. Either way the target file carries no
    attribute of its own, so the only place the information exists is the host.

    The ordinary half is returned for the same reason: one file can be declared
    twice, once by `pub mod x;` and again by `#[cfg(test)] #[path = "x.rs"] mod
    x_in_tests;`. The declaration that ships wins, because the file is in the
    released binary and its panics are reachable there.
    """
    gated: list[Path] = []
    ordinary: list[Path] = []
    for decl in MOD_DECL.finditer(clean):
        # Attributes belong to the declaration just after them, so the window to
        # search is bounded by whatever item ended most recently.
        window_start = max(
            clean.rfind(";", 0, decl.start()),
            clean.rfind("}", 0, decl.start()),
            clean.rfind("{", 0, decl.start()),
            0,
        )
        named = declared_path(text, window_start, decl.start())
        targets = (
            [(path.parent / named).resolve()]
            if named
            else [c for c in module_files(path, decl.group(1), at_crate_root) if c.exists()]
        )
        if any(start <= decl.start() < end for start, end in dead):
            # The declaration itself is dropped from every build, so the file it names is
            # compiled by nobody. Naming it in either edge list would put it back in a build.
            continue
        if any(start <= decl.start() < end for start, end in spans):
            gated.extend(targets)
        else:
            ordinary.extend(targets)
    return gated, ordinary


def compile_roots(crate_dir: Path) -> tuple[set[Path], set[Path]]:
    """(roots a released binary is built from, roots only a test/bench/example build is).

    Rust does not have a whole-directory compile: a file is in the program only if
    something names it, starting from one of these. Without this, a file nobody
    declares is indistinguishable from a file that ships.
    """
    shipped: set[Path] = set()
    testing: set[Path] = set()
    for candidate in (
        crate_dir / "src" / "lib.rs",
        crate_dir / "src" / "main.rs",
        crate_dir / "build.rs",
    ):
        if candidate.is_file():
            shipped.add(candidate.resolve())
    bin_dir = crate_dir / "src" / "bin"
    if bin_dir.is_dir():
        for path in bin_dir.rglob("*.rs"):
            # `src/bin/tool.rs` is a root; so is `src/bin/tool/main.rs`; anything
            # else under there is an ordinary module of one of those.
            if path.parent == bin_dir or path.name == "main.rs":
                shipped.add(path.resolve())
    for directory in NON_SHIPPING_DIRS:
        base = crate_dir / directory
        if not base.is_dir():
            continue
        for path in base.rglob("*.rs"):
            if path.parent == base or path.name == "main.rs":
                testing.add(path.resolve())
    manifest = crate_dir / "Cargo.toml"
    if manifest.is_file():
        for named in CARGO_TARGET_PATH.finditer(manifest.read_text(encoding="utf-8")):
            target = (manifest.parent / named.group(1)).resolve()
            if not target.is_file():
                continue
            rel = target.relative_to(crate_dir.resolve()).parts
            (testing if rel and rel[0] in NON_SHIPPING_DIRS else shipped).add(target)
    return shipped, testing


def reachable(
    roots: list[Path], edges: dict[Path, list[Path]]
) -> set[Path]:
    """Every file one of these roots names, directly or through a chain of modules."""
    seen: set[Path] = set()
    frontier = list(roots)
    while frontier:
        path = frontier.pop()
        if path in seen:
            continue
        seen.add(path)
        frontier.extend(edges.get(path, ()))
    return seen


def measure(
    root: Path,
) -> tuple[dict[str, dict[str, int]], list[tuple[str, str, int, str]], list[Path]]:
    """The counts, every production site as (crate, path, line, kind), and the
    files no build compiles.

    The site list exists so a number in the report can be walked to the line it
    came from; a governance batch that cannot find its own work has no way to
    reduce the count it was handed.
    """
    totals: dict[str, dict[str, int]] = {}
    sites: list[tuple[str, str, int, str]] = []
    files = sources(root)
    # Blanking the noise is the expensive part of reading a file, so each one is
    # read once and reused by the test-module walk and the counting pass.
    blanked: dict[Path, str] = {}
    spans_of: dict[Path, tuple[list[tuple[int, int]], list[tuple[int, int]]]] = {}
    text_of: dict[Path, str] = {}

    def spans_for(path: Path) -> tuple[list[tuple[int, int]], list[tuple[int, int]]]:
        """The file's test-gated spans and the spans no build compiles at all."""
        found = spans_of.get(path)
        if found is None:
            text = path.read_text(encoding="utf-8", errors="replace")
            clean = blank_noise(text)
            found = (test_spans(clean), never_spans(clean))
            text_of[path] = text
            blanked[path] = clean
            spans_of[path] = found
        return found

    def dead_spans_for(path: Path) -> list[tuple[int, int]]:
        return spans_for(path)[1]

    declared_test: set[Path] = set()
    shipped: set[Path] = set()
    # The module graph, from every file, so the reachability walk below can follow
    # a chain of declarations. Both halves are needed: the ordinary edges say what a
    # released binary contains, and the test-gated edges say what a `cargo test`
    # build adds. A file nobody reaches from either is compiled by nothing.
    ordinary_edges: dict[Path, list[Path]] = {}
    gated_edges: dict[Path, list[Path]] = {}
    all_edges: dict[Path, list[Path]] = {}
    # Roots come first: whether a file is a compilation unit decides where its own
    # `mod x;` declarations look for a file, so the graph cannot be built without it.
    shipped_roots: list[Path] = []
    test_roots: list[Path] = []
    for crate_dir in sorted({crate_dir_of(path, root) for path in files}):
        found_shipped, found_testing = compile_roots(crate_dir)
        shipped_roots.extend(sorted(found_shipped))
        test_roots.extend(sorted(found_testing))
    roots = {*shipped_roots, *test_roots}
    for path in files:
        spans, _dead = spans_for(path)
        gated, ordinary = module_declarations(
            blanked[path],
            text_of[path],
            path,
            spans,
            path.resolve() in roots,
            dead_spans_for(path),
        )
        key = path.resolve()
        ordinary_edges[key] = ordinary
        gated_edges[key] = gated
    all_edges: dict[Path, list[Path]] = {}
    for key, gated in gated_edges.items():
        all_edges[key] = [*ordinary_edges.get(key, []), *gated]
    shipped = reachable(shipped_roots, ordinary_edges)
    # `cargo test` compiles the same sources with the test cfg on, and a test
    # target brings its own tree with it; that is every file any build sees.
    compiled_by_any_build = reachable(
        [*shipped_roots, *test_roots], all_edges
    )
    uncompiled = [
        path for path in files if path.resolve() not in compiled_by_any_build
    ]
    uncompiled_set = {path.resolve() for path in uncompiled}

    for path in files:
        clean = blanked.get(path)
        if clean is None:
            clean = blank_noise(path.read_text(encoding="utf-8", errors="replace"))
            blanked[path] = clean
        spans, dead = spans_for(path)
        ships = path.resolve() in shipped
        whole_file_test = (
            not ships
            or is_test_by_location(path, root)
            or any(start == 0 and end == len(clean) for start, end in spans)
        )
        # A file whose whole body sits behind a predicate that can never hold ships nothing, even
        # when a crate root reaches it: `#![cfg(any())]` in a `lib.rs` is compiled away entirely.
        no_build_compiles_it = any(
            start == 0 and end == len(clean) for start, end in dead
        )
        crate = crate_of(path, root)
        row = totals.setdefault(
            crate,
            dict.fromkeys(
                [
                    "unwrap",
                    "unwrap_prod",
                    "expect",
                    "expect_prod",
                    "panic",
                    "panic_prod",
                    "unsafe",
                    "unsafe_prod",
                    "unsafe_block",
                    "unsafe_fn",
                    "unsafe_impl",
                    "unsafe_extern",
                    "files",
                    "test_files",
                    "uncompiled_files",
                ],
                0,
            ),
        )
        row["files"] += 1
        if path.resolve() in uncompiled_set:
            # No crate root reaches this file, so no build of any kind compiles it.
            # Its `.unwrap()`s cannot fire in a released binary and are not the
            # audit's work; the file itself is reported, because a test file in this
            # state is coverage somebody believes they have and do not.
            row["uncompiled_files"] += 1
            continue
        for pattern, key, prod_key in SITES:
            hits = list(pattern.finditer(clean))
            row[key] += len(hits)
            if whole_file_test:
                continue
            live = (
                []
                if no_build_compiles_it
                else [
                    hit
                    for hit in hits
                    if not any(
                        start <= hit.start() < end for start, end in [*spans, *dead]
                    )
                ]
            )
            row[prod_key] += len(live)
            for hit in live:
                line = clean.count("\n", 0, hit.start()) + 1
                sites.append((crate, str(path.relative_to(root)), line, KINDS[key]))
        row["unsafe"] = (
            row["unsafe_block"]
            + row["unsafe_fn"]
            + row["unsafe_impl"]
            + row["unsafe_extern"]
        )
        if whole_file_test:
            row["test_files"] += 1
    return totals, sites, uncompiled


def production_total(row: dict[str, int]) -> int:
    return row["unwrap_prod"] + row["expect_prod"] + row["panic_prod"]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=".", type=Path)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--write-baseline", type=Path)
    parser.add_argument("--check-baseline", type=Path)
    parser.add_argument(
        "--list",
        metavar="CRATE",
        help="print every production site of one crate as path:line kind",
    )
    parser.add_argument(
        "--write-uncompiled",
        type=Path,
        metavar="PATH",
        help="record the files no build compiles, so a new one is a failure",
    )
    parser.add_argument(
        "--check-uncompiled",
        type=Path,
        metavar="PATH",
        help="exit 1 if a file joined or left that list",
    )
    args = parser.parse_args()

    root = args.root.resolve()
    if not (root / "crates").is_dir():
        print(f"error: {root} has no crates/ directory", file=sys.stderr)
        return 2
    rows, sites, uncompiled = measure(root)

    if args.list:
        shown = [site for site in sites if site[0] == args.list]
        if not shown:
            known = ", ".join(sorted(rows))
            print(
                f"no production sites recorded for {args.list!r}; "
                f"crates here are: {known}",
                file=sys.stderr,
            )
            return 2
        for _, path, line, kind in shown:
            print(f"{path}:{line}: {kind}")
        print(f"# {len(shown)} production sites in {args.list}", file=sys.stderr)
        return 0

    if args.write_baseline:
        lines = [
            "# production panic-capable sites per crate, from scripts/ci/panic-site-census.py",
            "# crate<TAB>unwrap<TAB>expect<TAB>panic<TAB>unsafe  -- see the script for the definition",
        ]
        for crate in sorted(rows):
            row = rows[crate]
            lines.append(
                "\t".join(
                    str(value)
                    for value in (
                        crate,
                        row["unwrap_prod"],
                        row["expect_prod"],
                        row["panic_prod"],
                        row["unsafe_prod"],
                    )
                )
            )
        args.write_baseline.write_text("\n".join(lines) + "\n", encoding="utf-8")
        print(f"wrote {args.write_baseline} ({len(rows)} crates)")
        return 0

    if args.check_baseline:
        recorded = {}
        for line in args.check_baseline.read_text(encoding="utf-8").splitlines():
            if not line.strip() or line.startswith("#"):
                continue
            fields = line.split("\t")
            if len(fields) != 5:
                print(f"error: {args.check_baseline}:{line!r} is not 5 fields", file=sys.stderr)
                return 2
            recorded[fields[0]] = [int(field) for field in fields[1:]]
        worse, missing = [], sorted(set(recorded) - set(rows))
        for crate in sorted(rows):
            now = [
                rows[crate]["unwrap_prod"],
                rows[crate]["expect_prod"],
                rows[crate]["panic_prod"],
                rows[crate]["unsafe_prod"],
            ]
            was = recorded.get(crate)
            if was is None:
                continue
            if any(n > w for n, w in zip(now, was)):
                worse.append((crate, was, now))
        for crate, was, now in worse:
            print(
                f"{crate}: production sites grew {was} -> {now} "
                "(unwrap, expect, panic!, unsafe)"
            )
        for crate in missing:
            print(f"{crate}: in the baseline, no longer a crate here")
        if worse or missing:
            print(
                "\nA crate gained a way to panic in code that ships. Remove it, or "
                "re-run `scripts/ci/panic-site-census.py --write-baseline "
                f"{args.check_baseline}` having decided to keep it."
            )
            return 1
        improved = sum(
            max(0, sum(w) - sum(n))
            for crate in rows
            if (w := recorded.get(crate))
            for n in [
                [
                    rows[crate]["unwrap_prod"],
                    rows[crate]["expect_prod"],
                    rows[crate]["panic_prod"],
                    rows[crate]["unsafe_prod"],
                ]
            ]
        )
        print(
            f"baseline holds: {len(rows)} crates, {improved} fewer production sites "
            "than recorded"
        )
        return 0

    if args.write_uncompiled:
        lines = [
            "# Files under crates/ that no build compiles, from scripts/ci/panic-site-census.py.",
            "# Each is a module no crate root reaches. Every line is a decision somebody",
            "# has to revisit: wire it up, or delete the file and its tests with it.",
        ]
        lines.extend(sorted(str(path.relative_to(root)) for path in uncompiled))
        args.write_uncompiled.write_text("\n".join(lines) + "\n", encoding="utf-8")
        print(f"wrote {args.write_uncompiled} ({len(uncompiled)} files)")
        return 0

    if args.check_uncompiled:
        recorded = sorted(
            line
            for line in args.check_uncompiled.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.startswith("#")
        )
        now = sorted(str(path.relative_to(root)) for path in uncompiled)
        new = [path for path in now if path not in recorded]
        gone = [path for path in recorded if path not in now]
        for path in new:
            print(f"{path}: now compiled by nothing (new)")
        for path in gone:
            print(f"{path}: recorded as compiled by nothing, but it is not any more")
        if new or gone:
            print(
                "\nEither wire the module up, delete the file, or re-run "
                f"`scripts/ci/panic-site-census.py --write-uncompiled {args.check_uncompiled}` "
                "having decided to keep it as it is."
            )
            return 1
        print(f"uncompiled set holds: {len(now)} files, exactly as recorded")
        return 0

    if args.json:
        print(json.dumps(rows, indent=1, sort_keys=True))
        return 0

    order = sorted(rows.items(), key=lambda item: -production_total(item[1]))
    name_width = max(len(crate) for crate, _ in order) + 2
    print(f"{'crate':<{name_width}}{'unwrap':>8}{'expect':>8}{'panic!':>8}{'unsafe':>8}{'all .unwrap':>13}")
    for crate, row in order:
        print(
            f"{crate:<{name_width}}"
            f"{row['unwrap_prod']:>8}{row['expect_prod']:>8}"
            f"{row['panic_prod']:>8}{row['unsafe_prod']:>8}{row['unwrap']:>13}"
        )
    grand = {
        key: sum(row[key] for _, row in order)
        for key in order[0][1]
    }
    print(
        f"{'TOTAL':<{name_width}}{grand['unwrap_prod']:>8}{grand['expect_prod']:>8}"
        f"{grand['panic_prod']:>8}{grand['unsafe_prod']:>8}{grand['unwrap']:>13}"
    )
    print(
        f"\n{grand['unwrap']} .unwrap() calls in all, {grand['unwrap_prod']} of them "
        f"outside any cfg(test) span ({grand['unwrap_prod'] * 100 // max(1, grand['unwrap'])}%); "
        f"{grand['unsafe']} unsafe sites, {grand['unsafe_prod']} in production."
    )
    print(
        f"unsafe by kind, every build: {grand['unsafe_block']} block, "
        f"{grand['unsafe_fn']} fn, {grand['unsafe_impl']} impl, "
        f"{grand['unsafe_extern']} extern"
    )
    if uncompiled:
        print(
            f"\n{len(uncompiled)} .rs files under crates/ are compiled by nothing: no "
            "crate root reaches them, so every site in them is counted nowhere."
        )
        for path in uncompiled:
            print(f"   {path.relative_to(root)}")
        print(
            "A test file in that state is coverage someone believes they have; "
            "wire the module up or delete the file."
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
