#!/usr/bin/env python3
"""Census of every process-wide cwd change in the Rust sources, with its restore.

Why this exists rather than a comment in a review: on 2026-10-03 a test that
chdir'd into a directory and then had that directory unlinked under it poisoned
every other test in the same process. `gix_discover::is_git` calls
`current_dir()` before it looks at the path it was given, so once the process
cwd is gone, *every* `gix::open` in that process fails as `NotARepository`, no
matter how absolute the path. In `xai-fast-worktree`'s lib test binary that read
as ~33 unrelated failures, and the one test that caused it passed. The full
write-up is `docs/verification/fast-worktree-safety-gate-flake-2026-10-03.log`.

The mechanism is process-global state, so it cannot be caught by the test that
tripped over it -- the poisoned test is never the one at fault. What can be
caught is the shape that creates it: a call to `set_current_dir` that is not
paired with a restore installed in the same function. This script lists those
calls and refuses three kinds of drift.

  * a call site that is not in the baseline (new cwd change, needs a reason);
  * a baseline row whose site is gone or has moved (the line number is checked);
  * a test call site with no guard binding before it (the structural rule).

The structural rule is not overridable from the baseline, because the baseline
is the file someone under time pressure would edit.

What counts as a test site: the file is under `tests/`, `benches/` or
`examples/` (or named `*tests*`), or some file reaches it through a declaration
that is test-gated -- that is how `git/safety_tests/gate.rs` counts, since it
carries no attribute of its own -- or the call sits inside a span this file
marks with `#[test]`, `cfg(test)` or `cfg_attr(test, ...)`. Anything else is
production, where a permanent cwd change is the feature (`--cwd`, the GUI's
"change location") and only the reason is required.

Stated limits, because they are the reason this is a census and not a lint:
the scope is every `.rs` file in the repository except `target/`, `.git/`,
`node_modules/`, `dist/` and `test-results/` -- the workspace has members outside
`crates/` (`prod/mc/cli-chat-proxy-types`, `third_party/*`), so a walk rooted at
`crates/` would quietly grade part of what cargo builds as if it did not exist;
matching happens on source text after comments, doc comments and string literals
are blanked, so a `set_current_dir` assembled by a macro is invisible here; the
span analysis is brace-based, so a `cfg(test)` item whose braces are unbalanced
in the text extends over whatever follows it; and "guard bound before the call"
is matched by name (`CwdGuard`, `RestoreCwd`, or a `Drop` impl for one of them)
rather than by data flow. Each limit could let a new site look ordinary instead
of failing closed, which is why the table lists every site with its line.

    scripts/ci/cwd-change-census.py                      # the table
    scripts/ci/cwd-change-census.py --write-baseline scripts/ci/cwd-change-baseline.tsv
    scripts/ci/cwd-change-census.py --check-baseline scripts/ci/cwd-change-baseline.tsv
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from collections.abc import Callable
from pathlib import Path

DEFAULT_ROOT = Path(__file__).resolve().parents[2]

# `env::set_current_dir(` and a bare `set_current_dir(` after a `use`, but not a
# definition of something named set_current_dir.
SITE = re.compile(r"(?<![A-Za-z0-9_])set_current_dir\s*\(")
MOD_OR_FN = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?(?:default[ \t]+)?(?:const[ \t]+)?"
    r"(?:async[ \t]+)?(?:unsafe[ \t]+)?(?:extern[ \t]+(?:[A-Za-z_]+\s+)?)?(fn|mod)[ \t]+([A-Za-z0-9_]+)",
    re.MULTILINE,
)
GUARD = re.compile(r"\b(CwdGuard|RestoreCwd)\s*\(")
DROP_GUARD = re.compile(r"impl\s+Drop\s+for\s+(?:CwdGuard|RestoreCwd)\b")
MOD_DECL = re.compile(r"\bmod\s+([A-Za-z0-9_]+)\s*[;{]")
PATH_ATTR = re.compile(r'#\[path\s*=\s*"([^"]+)"\]')
TEST_ATTR = re.compile(r"#\[[^\n]*\btest\b")
RAW_STRING = re.compile(r'r(#*)"')
CHAR_LITERAL = re.compile(r"'(?:\\.|[^\\'])'")
ROLE_TEST = "test"
ROLE_PROD = "product"
MIN_REASON = 20


def blank_noise(text: str) -> str:
    """Blank comments and string literals so call sites in prose do not count.

    Character positions are preserved: the line numbers in the table have to be
    the ones an editor would take you to. Newlines are never overwritten, so a
    multi-line raw string costs lines the way the source does.
    """
    out = list(text)
    i = 0
    n = len(text)
    depth = 0
    while i < n:
        if depth:
            if text.startswith("*/", i):
                depth -= 1
                out[i] = out[i + 1] = " "
                i += 2
                continue
            if text[i] != "\n":
                out[i] = " "
            i += 1
            continue
        if text.startswith("/*", i):
            depth += 1
            out[i] = out[i + 1] = " "
            i += 2
            continue
        if text.startswith("//", i):
            while i < n and text[i] != "\n":
                out[i] = " "
                i += 1
            continue
        raw = RAW_STRING.match(text, i)
        if raw:
            terminator = '"' + "#" * len(raw.group(1))
            start = raw.end()
            end = text.find(terminator, start)
            stop = n if end < 0 else end + len(terminator)
            for k in range(i, min(stop, n)):
                if text[k] != "\n":
                    out[k] = " "
            i = max(stop, i + 1)
            continue
        if text[i] == '"':
            j = i + 1
            while j < n and text[j] not in "\n\"":
                j += 2 if text[j] == "\\" else 1
            stop = min(j + 1, n)
            for k in range(i, stop):
                if text[k] != "\n":
                    out[k] = " "
            i = max(stop, i + 1)
            continue
        if text[i] == "'":
            # A char literal or a lifetime. Only blank the char-literal shape:
            # 'x', '\n', '\''.
            literal = CHAR_LITERAL.match(text, i)
            if literal:
                for k in range(i, literal.end()):
                    out[k] = " "
                i = literal.end()
                continue
        i += 1
    return "".join(out)


def test_by_location(path: Path, root: Path) -> bool:
    for part in path.relative_to(root).parts[:-1]:
        if part in ("tests", "benches", "examples"):
            return True
    name = path.name
    return "tests" in name or "tests" in path.parent.name


def brace_end(clean: str, open_brace: int) -> int:
    """Offset just past the `}` matching the `{` at `open_brace`, or the file end."""
    depth = 0
    for i in range(open_brace, len(clean)):
        if clean[i] == "{":
            depth += 1
        elif clean[i] == "}":
            depth -= 1
            if depth == 0:
                return i + 1
    return len(clean)


def test_by_location(path: Path, root: Path) -> bool:
    for part in path.relative_to(root).parts[:-1]:
        if part in ("tests", "benches", "examples"):
            return True
    return "tests" in path.name or "tests" in path.parent.name


def gated_spans(clean: str) -> list[tuple[int, int]]:
    """Spans a `#[test]` / `cfg(test)` attribute in this very file gates.

    Two shapes: an attribute over an item with a body (`#[cfg(test)] mod x { ... }`,
    `#[test] fn t() { ... }`) gates through the matching brace, and an attribute over
    an item without one (`#[cfg(test)] use foo::Bar;`) gates just that item. Without
    the second case a `use` line would look like it gated the rest of the file.
    """
    spans: list[tuple[int, int]] = []
    for attr in TEST_ATTR.finditer(clean):
        rest = clean[attr.end() :]
        next_brace = rest.find("{")
        next_semicolon = rest.find(";")
        if next_semicolon >= 0 and (next_brace < 0 or next_semicolon < next_brace):
            bound = rest[:next_semicolon]
            if "{" not in bound:
                spans.append((attr.start(), attr.end() + next_semicolon + 1))
                continue
        brace = clean.find("{", attr.end())
        if brace < 0:
            continue
        spans.append((attr.start(), brace_end(clean, brace)))
    return spans


def module_declarations(clean: str, raw: str, path: Path) -> list[tuple[Path, bool]]:
    """Files this one brings in, each with whether that declaration is test-gated.

    The attribute belongs to the declaration after it, so the window searched is
    bounded by whatever item ended most recently. A `mod x;` inside a
    `#[cfg(test)] mod tests { ... }` block inherits the gate from that block.

    Structure is read from the blanked text and the `#[path = "..."]` target from the
    raw text: positions are preserved between the two, and the blanked copy has
    already erased the very file name being looked for.
    """
    out: list[tuple[Path, bool]] = []
    spans = gated_spans(clean)
    for decl in MOD_DECL.finditer(clean):
        window_start = max(
            clean.rfind(";", 0, decl.start()),
            clean.rfind("}", 0, decl.start()),
            clean.rfind("{", 0, decl.start()),
            0,
        )
        window = clean[window_start : decl.start()]
        named = PATH_ATTR.search(raw[window_start : decl.start()])
        if named:
            targets = [(path.parent / named.group(1)).resolve()]
        else:
            name = decl.group(1)
            targets = [
                candidate.resolve()
                for candidate in (
                    path.parent / f"{name}.rs",
                    path.parent / name / "mod.rs",
                    path.parent.parent / f"{name}.rs",
                )
                if candidate.exists()
            ]
        gated = bool(TEST_ATTR.search(window)) or any(
            start <= decl.start() < end for start, end in spans
        )
        out.extend((target, gated) for target in targets)
    return out


def file_is_test_only(
    path: Path, root: Path, inbound_of: Callable[[Path], list[tuple[Path, bool]]]
) -> bool:
    """Whether only a test build ever compiles this file.

    Either it sits where only tests are compiled, or some file reaches it through a
    declaration that is itself test-gated -- which is how `git/safety_tests/gate.rs`
    is reached, since it carries no attribute of its own and only its host knows.
    """
    if test_by_location(path, root):
        return True
    seen: set[Path] = set()
    frontier = [path]
    while frontier:
        current = frontier.pop()
        if current in seen:
            continue
        seen.add(current)
        for host, gated in inbound_of(current):
            if gated or test_by_location(host, root):
                return True
            frontier.append(host)
    return False


def enclosing_fn(clean: str, offset: int) -> tuple[int, str]:
    """(start offset, name) of the nearest `fn` header above the offset."""
    best = (-1, "")
    for match in MOD_OR_FN.finditer(clean[:offset]):
        if match.group(1) == "fn":
            best = (match.start(), match.group(2))
    return best


def role_at(
    clean: str, path: Path, spans: list[tuple[int, int]], test_only_file: bool, offset: int
) -> str:
    """Test site when this file is test-only or the call sits in a gated span."""
    if test_only_file:
        return ROLE_TEST
    if any(start <= offset < end for start, end in spans):
        return ROLE_TEST
    return ROLE_PROD


def guard_before_call(clean: str, fn_start: int, offset: int) -> str | None:
    """The guard type bound in the enclosing `fn` before this call, if any.

    A restore implementation (`CwdGuard::drop`) is itself the restore, so it is
    reported as its own guard rather than failing the rule that needs one.
    """
    start = fn_start if fn_start >= 0 else 0
    hits = list(GUARD.finditer(clean[start:offset]))
    if hits:
        return hits[-1].group(1)
    if enclosing_fn(clean, offset)[1] == "drop" and DROP_GUARD.search(clean[max(0, start - 400) : start]):
        return "drop-restore"
    return None


SKIP_DIRS = {"target", ".git", ".cargo", "node_modules", "dist", "test-results"}


def rust_files(root: Path) -> list[Path]:
    """Every Rust file in the repository except build output and vendored checkouts.

    The walk starts at the repository root rather than at `crates/`, because the
    workspace has members outside it (`prod/mc/cli-chat-proxy-types`, the four
    `third_party/*` crates), and a guard that silently does not look at part of the
    workspace is worse than one that is not there.
    """
    found: list[Path] = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
        for name in sorted(filenames):
            if name.endswith(".rs"):
                found.append(Path(dirpath) / name)
    return sorted(found)


def sites(root: Path) -> list[dict[str, object]]:
    """Every cwd change in the workspace, with the role and restore of each.

    Only the files that could matter are parsed in depth: those holding a call site
    and those declaring a module that leads to one. Blanking and parsing all ~3000
    sources to answer a question about seven lines took the guard from a second to
    minutes, and a guard that slow gets skipped.
    """
    all_files = rust_files(root)
    raw: dict[Path, str] = {}
    for path in all_files:
        text = path.read_text(encoding="utf-8", errors="replace")
        if "mod " in text or "set_current_dir" in text:
            raw[path] = text

    clean_cache: dict[Path, str] = {}

    def raw_of(path: Path) -> str:
        return raw.get(path) or path.read_text(encoding="utf-8", errors="replace")

    def cleaned(path: Path) -> str:
        clean = clean_cache.get(path)
        if clean is None:
            clean = blank_noise(raw_of(path))
            clean_cache[path] = clean
        return clean

    inbound_cache: dict[Path, list[tuple[Path, bool]]] = {}

    def inbound_of(target: Path) -> list[tuple[Path, bool]]:
        cached = inbound_cache.get(target)
        if cached is not None:
            return cached
        needles = (f"mod {target.stem}", target.name)
        found_hosts: list[tuple[Path, bool]] = []
        for host, text in raw.items():
            if host == target or not any(needle in text for needle in needles):
                continue
            for declared, gated in module_declarations(cleaned(host), raw_of(host), host):
                if declared == target:
                    found_hosts.append((host, gated))
        inbound_cache[target] = found_hosts
        return found_hosts

    found: list[dict[str, object]] = []
    for path in all_files:
        if "set_current_dir" not in raw.get(path, ""):
            continue
        clean = cleaned(path)
        test_only_file = file_is_test_only(path, root, inbound_of)
        spans = gated_spans(clean)
        for match in SITE.finditer(clean):
            fn_start, fn_name = enclosing_fn(clean, match.start())
            found.append(
                {
                    "path": path.relative_to(root).as_posix(),
                    "line": clean.count("\n", 0, match.start()) + 1,
                    "role": role_at(clean, path, spans, test_only_file, match.start()),
                    "item": fn_name or "(file scope)",
                    "guard": guard_before_call(clean, fn_start, match.start()) or "",
                }
            )
    return sorted(found, key=lambda row: (str(row["path"]), int(row["line"])))


def render(rows: list[dict[str, object]]) -> str:
    lines = ["path\tline\trole\titem\tguard"]
    for row in rows:
        lines.append(
            "\t".join(str(row[k]) for k in ("path", "line", "role", "item", "guard"))
        )
    return "\n".join(lines)


def read_baseline(path: Path) -> dict[tuple[str, int], dict[str, str]]:
    out: dict[tuple[str, int], dict[str, str]] = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        if not raw.strip() or raw.startswith("#"):
            continue
        cols = raw.split("\t")
        if len(cols) < 4:
            raise SystemExit(f"{path.name}: malformed row (need 4 tab columns): {raw!r}")
        out[(cols[0], int(cols[1]))] = {
            "role": cols[2],
            "guard": cols[3],
            "reason": cols[4] if len(cols) > 4 else "",
        }
    return out


def check_baseline(root: Path, baseline: Path) -> int:
    rows = sites(root)
    declared = read_baseline(baseline)
    problems: list[str] = []

    measured = {(str(r["path"]), int(r["line"])): r for r in rows}
    for key in sorted(set(measured) - set(declared)):
        row = measured[key]
        problems.append(
            f"{key[0]}:{key[1]} changes the process cwd but is not in {baseline.name}; "
            f"role={row['role']} guard={row['guard'] or 'none'} -- a test site needs a guard "
            f"bound before the call, and either way the baseline row must say why the change is safe"
        )
    for key in sorted(set(declared) - set(measured)):
        problems.append(
            f"{baseline.name} lists {key[0]}:{key[1]} but no call is there any more "
            f"(moved or removed -- update the row)"
        )

    for key, row in sorted(measured.items()):
        note = declared.get(key)
        if note is None:
            continue
        if note["role"] != row["role"]:
            problems.append(
                f"{key[0]}:{key[1]} is role={row['role']} by the rules in this file but "
                f"{baseline.name} says {note['role']}"
            )
        if note["guard"] != (row["guard"] or "none"):
            problems.append(
                f"{key[0]}:{key[1]} binds {row['guard'] or 'no guard'} before the call, "
                f"{baseline.name} says {note['guard']}"
            )
        if len(note["reason"].strip()) < MIN_REASON:
            problems.append(
                f"{key[0]}:{key[1]} has no reason worth reading in {baseline.name} "
                f"(need at least {MIN_REASON} characters: who puts the cwd back, or why it may stay)"
            )
        if row["role"] == ROLE_TEST and not row["guard"]:
            problems.append(
                f"{key[0]}:{key[1]} is a test call site with no CwdGuard/RestoreCwd bound "
                f"before it; a test that leaves the process cwd inside a directory another "
                f"test deletes breaks every sibling in the binary (see "
                f"docs/verification/fast-worktree-safety-gate-flake-2026-10-03.log)"
            )

    if problems:
        print(f"cwd-change-census: {len(problems)} problem(s)")
        for problem in problems:
            print("  " + problem)
        print("recount with: python3 scripts/ci/cwd-change-census.py --write-baseline " + str(baseline))
        return 1

    prod = sum(1 for r in rows if r["role"] == ROLE_PROD)
    print(
        f"cwd-change-census: {len(rows)} call site(s) match {baseline.name} "
        f"({prod} production, {len(rows) - prod} test), every test site guarded"
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT, help="repository root")
    parser.add_argument("--write-baseline", type=Path, metavar="PATH", help="write the census as a baseline")
    parser.add_argument("--check-baseline", type=Path, metavar="PATH", help="compare the census to a baseline")
    args = parser.parse_args(argv)

    if args.check_baseline:
        return check_baseline(args.root, args.check_baseline)
    rows = sites(args.root)
    if args.write_baseline:
        header = (
            "# Every `set_current_dir` call in the Rust sources, checked by\n"
            "# scripts/ci/cwd-change-census.py. Columns: path, line, role, guard, reason.\n"
            "# The guard column is the restore type bound *before* the call; the reason\n"
            "# column is the human part and is not derived.\n"
        )
        body = "\n".join(
            "\t".join(
                [
                    str(r["path"]),
                    str(r["line"]),
                    str(r["role"]),
                    str(r["guard"] or "none"),
                    "",
                ]
            )
            for r in rows
        )
        args.write_baseline.write_text(header + body + "\n", encoding="utf-8")
        print(f"wrote {len(rows)} row(s) to {args.write_baseline}; fill in the reason column")
        return 0
    print(render(rows))
    return 0


if __name__ == "__main__":
    sys.exit(main())
