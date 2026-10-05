#!/usr/bin/env python3
"""Refuse a `cfg(...)` predicate that can never hold.

The premise is something this repository lived through. `crates/codegen/xai-grok-shell/src/agent/
auth_method.rs` carried 23 `#[test]` functions under a `#[cfg(any())]` module for ten weeks. An
empty `any()` is false in every build of every target, so rustc dropped the module from the token
stream before name resolution:

- the tests never ran, and no runner reported them as skipped -- `cargo test` printed a lower
  count and every observer read that count as the whole suite;
- the `#[ignore]` ledger could not see them either, because it parses `#[ignore]` attributes and
  those sat inside the dropped text;
- the panic-site census counted 7 `.unwrap()`s from that dead text as *production* panic sites,
  because its cfg evaluator answers a different question ("does this require `test`") and an empty
  `any()` requires nothing;
- and the file still read like covered code to everyone who scrolled past it.

Nothing in the toolchain warns about the shape. rustc's `unexpected_cfgs` lint reports condition
*nobody ever sets*, and an empty `any()` is a well-formed condition that simply never matches, so
it compiles clean at every warning level; clippy has nothing to say; `cargo test` cannot report
what was never compiled in.

So the check is here, over the same source roots the census walks (`crates/`, `bin/`). The folding
lives in `scripts/cfg_lib.py`, shared with the census so the two cannot read the same predicate
two ways; only FALSE is refused. A predicate that folds to TRUE (`#[cfg(all())]`,
`any(f, not(f))`) states something needless but compiles exactly what it looks like it compiles,
and that is a style question rather than a hole in the ledgers.

Unreadable predicates are refused too. `#[cfg(any(]` is not valid Rust, but a scanner that quietly
skipped text it could not parse is precisely how a dead gate would survive its own gate.

    python3 scripts/ci/check-dead-cfg.py [--root DIR] [--verbose]

Exit 0 means every predicate in the tree folds to TRUE or UNKNOWN. Exit 1 names each one that does
not. Exit 2 means the scan found no Rust sources, which is a broken invocation, not a clean tree.
"""

from __future__ import annotations

import argparse
import importlib.util
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
# The cfg reading is shared with `panic-site-census.py`; it lives one directory up,
# like `scripts/notices_lib.py`, because a file in `scripts/ci/` is a gate that
# something has to run, and a library imported by two guards is not that.
LIB = HERE.parent / "cfg_lib.py"
if not LIB.is_file():
    raise SystemExit(f"check-dead-cfg: {LIB} is missing, so nothing here can be checked")
_spec = importlib.util.spec_from_file_location("cfg_lib", LIB)
cfg_lib = importlib.util.module_from_spec(_spec)
assert _spec.loader is not None
_spec.loader.exec_module(cfg_lib)

TRUE, FALSE, UNKNOWN = cfg_lib.TRUE, cfg_lib.FALSE, cfg_lib.UNKNOWN
fold = cfg_lib.fold

DEFAULT_ROOT = HERE.parents[1]
SCAN_DIRS = ("crates", "bin")
SKIP_DIR_NAMES = {"target", "__pycache__", ".git", "node_modules", "dist", "build"}

# A whole character literal: `'}'`, `'\n'`, `'\u{1f600}'`, `'\''`. Anything else starting with a
# quote is a lifetime. An unblanked `(` or `)` inside one would unbalance the walk that reads a
# predicate, and an unblanked quote would end a string literal early.
CHAR_LITERAL = re.compile(r"'(?:\\.|[^\\'])'|'\\u\{[0-9a-fA-F]+\}'")


def blank_noise(source: str) -> str:
    """Blank comments and character literals, and the brackets inside string literals.

    Byte positions are kept, because the caller reports line numbers counted against this text.
    Comments go completely; literals keep their letters, because a cfg value is part of the
    predicate -- `all(target_os = "linux", not(target_os = "linux"))` is a contradiction and
    `all(target_os = "linux", not(target_os = "macos"))` is not, and telling those apart is the
    whole difference between refusing dead code and refusing a macOS-only block.
    """
    out = list(source)
    i = 0
    n = len(source)

    def blank(start: int, stop: int) -> None:
        for j in range(start, stop):
            if out[j] != "\n":
                out[j] = " "

    def blank_brackets(start: int, stop: int) -> None:
        """Space out the brackets inside a literal, leaving its letters alone.

        Text in a literal can then no longer look like the start of a predicate (`CFG_USE` needs a
        live `(`) or unbalance the parenthesis walk, while `feature = "gated"` still reads as the
        value it names.
        """
        for j in range(start, stop):
            if out[j] in "()[]{}":
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
                stop = end if end >= 0 else n
                blank_brackets(i, stop)
                i = stop
            else:
                if source[i] == "'":
                    literal = CHAR_LITERAL.match(source, i)
                    if literal:
                        blank(i, literal.end())
                        i = literal.end()
                        continue
                i += 1
    return "".join(out)


def scan_file(path: Path) -> tuple[list[str], list[str], list[str]]:
    """One file's never-true predicates, its unreadable predicates, and what was examined."""
    text = path.read_text(encoding="utf-8", errors="replace")
    clean = blank_noise(text)
    dead: list[str] = []
    unreadable: list[str] = []
    examined: list[str] = []
    for opening in cfg_lib.CFG_USE.finditer(clean):
        read = cfg_lib.predicate_span(clean, opening.start())
        form = opening.group(1) or "cfg"
        line = clean.count("\n", 0, opening.start()) + 1
        if read is None:
            unreadable.append(f"{path}:{line}: {form} predicate never closes")
            continue
        args, _ = read
        if form == "cfg_attr":
            args = cfg_lib.up_to_top_level_comma(args)
        flat = " ".join(args.split())
        if not flat:
            unreadable.append(f"{path}:{line}: {form}() with no predicate")
            continue
        try:
            state = fold(args).state
        except ValueError as err:
            unreadable.append(f"{path}:{line}: {form}({flat}) cannot be parsed: {err}")
            continue
        examined.append(f"{path}:{line}: {form}({flat}) -> {state}")
        if state == FALSE:
            dead.append(f"{path}:{line}: {form}({flat}) can never hold")
    return dead, unreadable, examined


def sources(root: Path) -> list[Path]:
    found: list[Path] = []
    for name in SCAN_DIRS:
        base = root / name
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.rs")):
            if not path.is_file():
                continue
            if SKIP_DIR_NAMES.intersection(path.relative_to(root).parts):
                continue
            found.append(path)
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=DEFAULT_ROOT, type=Path)
    parser.add_argument("--verbose", action="store_true", help="list every predicate examined")
    args = parser.parse_args()

    root = args.root.resolve()
    files = sources(root)
    if not files:
        print(
            f"no .rs files under {root / SCAN_DIRS[0]} or {root / SCAN_DIRS[1]}: nothing was "
            "checked, which is not the same as a clean tree",
            file=sys.stderr,
        )
        return 2

    dead: list[str] = []
    unreadable: list[str] = []
    examined: list[str] = []
    for path in files:
        found_dead, found_bad, seen = scan_file(path)
        dead.extend(found_dead)
        unreadable.extend(found_bad)
        examined.extend(seen)

    if args.verbose:
        for line in examined:
            print(line)
        print(f"{len(examined)} cfg predicates examined across {len(files)} files")
    for line in unreadable:
        print(line)
    if unreadable:
        print(
            f"\n{len(unreadable)} cfg predicate(s) could not be read. A predicate this script "
            "cannot fold is not evidence that nothing is wrong behind it, so it is reported "
            "rather than skipped."
        )
    for line in dead:
        print(line)
    if dead:
        print(
            f"\n{len(dead)} cfg predicate(s) can never hold, so rustc drops whatever they gate "
            "from every build. Test modules behind one are compiled by no build: they neither run "
            "nor appear in any ledger, and the file still reads as covered code. Delete the gated "
            "text, or make the condition one a build can satisfy."
        )
    if dead or unreadable:
        return 1
    if not args.verbose:
        print(f"{len(files)} files, no cfg predicate that can never hold")
    return 0


if __name__ == "__main__":
    sys.exit(main())
