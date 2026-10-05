#!/usr/bin/env python3
"""Refuse a lock acquisition written so that a dead holder reads as an absent value.

`.lock().ok()` turns `Result<MutexGuard<T>, PoisonError<MutexGuard<T>>>` into
`Option<MutexGuard<T>>`. The `Err` thrown away there is not "there is no value" -- it is "the
thread that held this lock died holding it", and that error still *carries the guard*. A caller
that chains `and_then` / `is_some` / `unwrap_or` off the `.ok()` therefore answers a question it
never learned the answer to, and it answers it the same way whether the lock was poisoned, the
value was never set, or the read simply did not happen.

This is the mirror image of the panic-on-poison form that `scripts/ci/panic-site-census.py`
ratchets and that `xai_grok_shell::util::shared_guard` exists to remove: one raises a second
panic in a thread that only wanted to read, the other pretends nothing happened. Both lose the
one fact the lock is reporting.

The rule is a hard zero, with no baseline file, for two reasons. The batch that introduced it
converted every site the whole-file scan finds (19, across 10 locks in 14 files and 4 crates;
11 is what a per-line grep reports, because rustfmt had split 8 of those chains across lines),
so there is nothing to ratchet down from.
And a ceiling here would be a promise to keep tolerating a specific count of a shape whose only
possible purposes are all answered better by something else: read the value anyway with
`unwrap_or_else(PoisonError::into_inner)` (the spelling most of this workspace already uses), take
the trait `LockOrRecover` / `ReadWriteOrRecover`, or `match` on the error, log it, and decide what
a dead holder means for that field. Skipping the locked work when the holder died is sometimes
right -- every one of those is a visible decision, and `.ok()` is not.

It applies to test code too, unlike the test-side scan in `check-unbounded-recv.py`: nothing about
being a test makes "a holder died" indistinguishable from "there is no value" acceptable, and a
test that swallows poisoning cannot observe it either.

Two shapes this deliberately does not touch (`--inventory` counts the second one, so the
number of open decisions comes from this file rather than from a hand-run grep):

  * `try_lock()` / `try_read()` / `try_write()`, whose `Err` means "someone else is holding it
    right now", which is a legitimate reason to do something else;
  * an `if let Ok(guard) = lock.lock()` / `let Ok(..) = .. else { return }` that skips the locked
    work. That reads as a decision to drop an update, and for a best-effort log writer or a
    watcher progress slot it is the correct one. It is still a decision a reviewer should see
    made per lock, and `docs/ci-test-debt.md` records why this gate does not pretend to judge it.

One false positive is possible in principle: a non-lock method named `lock`, `read` or `write`
that takes no arguments and returns a `Result`. None exists in this workspace (`rustfmt`-checked
call sites aside, the gate's message names the receiver so a mismatch is obvious). If one ever
appears, rename the method rather than exempt the call: the shared name is the whole ambiguity.

    python3 scripts/ci/check-lock-poison.py [--root DIR] [--verbose]
    python3 scripts/ci/check-lock-poison.py --inventory   # the allowed shapes, counted

Exit 0 means the tree has no such site. Exit 1 names each one. Exit 2 means no Rust sources were
found, which is a broken invocation rather than a clean tree.
"""

from __future__ import annotations

import argparse
import importlib.util
import os
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
# Reuse the census's string/comment blanking and its reading of `cfg` expressions, so this gate
# and `panic-site-census.py` cannot disagree about what a byte of this tree says.
_CENSUS = HERE / "panic-site-census.py"
if not _CENSUS.is_file():
    raise SystemExit(f"check-lock-poison: {_CENSUS} is missing, so nothing here can be checked")
_census_spec = importlib.util.spec_from_file_location("panic_site_census", _CENSUS)
census = importlib.util.module_from_spec(_census_spec)
assert _census_spec.loader is not None
_census_spec.loader.exec_module(census)

# The acquisition and the swallow, adjacent. `try_lock()` cannot match: `lock` there is preceded
# by `_`, and this needs the `.` of a method call. `.read().await.ok()` cannot match either, and
# must not -- an async lock has no poisoning, so its `Err` is a closed or cancelled wait.
SITE = re.compile(r"\.(?P<acq>lock|read|write)\s*\(\s*\)\s*\.\s*ok\s*\(\s*\)")
# The receiver, for the message: a dotted path immediately before the call.
RECEIVER = re.compile(r"([A-Za-z_]\w*(?:\s*\.\s*[A-Za-z_]\w*)*)\s*$")
SKIP_DIRS = ("target", "node_modules", "third_party", "__pycache__")

# The three spellings of the decision this gate deliberately does not judge: branch on the poison
# error instead of reading through it. `--inventory` counts them so the open TODO row carries a
# number produced here rather than one re-derived by hand each time someone reads it. Each pattern
# requires the acquisition to be the LAST call before the branch opener, which is what keeps a
# `.read(&mut buf)`, a `.lock().ok()` (that one is the gate's own target) and a `.lock().unwrap()`
# out of the count. One honest limit: `match-Err-arm` requires no braces between the `match` head
# and the `Err`, so an `Err` arm written after an arm with a block body is not seen. The inventory
# can therefore under-count, never over-count, and it is quoted as an inventory, never as a bound.
ACQ = r"\.(?:lock|read|write)\s*\(\s*\)"
BRANCHES = {
    "if-let-Ok": re.compile(
        r"if\s+let\s+Ok\s*\([^()]{0,120}?\)\s*=[^{};]{0,240}?" + ACQ + r"\s*\{"
    ),
    "let-Ok-else": re.compile(
        r"let\s+Ok\s*\([^()]{0,120}?\)\s*=[^{};]{0,240}?" + ACQ + r"\s*else"
    ),
    "match-Err-arm": re.compile(
        r"match\s+[^{};]{0,240}?" + ACQ + r"\s*\{[^{}]{0,700}?Err\s*\("
    ),
}
# Where the poison arm's own body starts, so the inventory can tell "takes the guard back" from
# "drops it" instead of calling every branch the same decision.
ERR_ARROW = re.compile(r"\)\s*=>\s*")


def drops_the_guard(clean: str, err_open: int) -> bool:
    """Does the `Err(..)` arm at `err_open` refuse to look at the guard it was handed?

    `PoisonError` carries the guard, so an arm calling `into_inner()` is doing the right thing and
    is bucketed apart from one that invents absence. `if let Ok(..)` and `let Ok(..) else` always
    answer True at the call site: they bind nothing on the failure path, so the value is
    unreachable there by construction.
    """
    m = ERR_ARROW.search(clean, err_open)
    if m is None:
        return True
    brace = clean.find("{", m.end())
    semi = clean.find(";", m.end())
    comma = clean.find(",", m.end())
    if brace >= 0 and (semi < 0 or brace < semi) and (comma < 0 or brace < comma):
        return "into_inner" not in clean[brace : census.brace_end(clean, brace)]
    stop = min(x for x in (semi, comma, len(clean)) if x >= 0)
    return "into_inner" not in clean[m.end() : stop]


def sources(root: Path) -> list[Path]:
    """Every `.rs` under `root`, with the ignored directories pruned on the way down."""
    found: list[Path] = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
        for name in sorted(filenames):
            if name.endswith(".rs"):
                found.append(Path(dirpath) / name)
    return sorted(found)


def line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def receiver_of(text: str, start: int) -> str:
    """The dotted expression the acquisition hangs off, for the message only.

    The window is cut at statement boundaries but NOT at newlines: rustfmt puts `.lock()` and
    `.ok()` on their own lines for long chains, and `RECEIVER` already spans whitespace around
    the dots, so cutting at a newline would leave an empty window and print `?` for exactly the
    wrapped chains the message is supposed to identify.
    """
    tail = text[:start]
    cut = max(tail.rfind(";"), tail.rfind("{"), tail.rfind("}"))
    m = RECEIVER.search(tail[cut + 1 :])
    return re.sub(r"\s+", "", m.group(1)) if m else "?"


def scan(root: Path, files: list[Path]) -> list[str]:
    """One message per site: `crate: path:line: receiver.lock().ok()  <source line>`."""
    found: list[str] = []
    for path in files:
        rel = path.relative_to(root)
        try:
            raw = path.read_text(encoding="utf-8", errors="replace")
        except OSError as exc:  # unreadable is a scan failure, not a clean tree
            sys.exit(f"cannot read {rel}: {exc}")
        clean = census.blank_noise(raw)
        dead = census.never_spans(clean)
        lines = raw.splitlines()
        for m in SITE.finditer(clean):
            if any(a <= m.start() < b for a, b in dead):
                continue  # no build compiles it, so no build can read a poisoned lock through it
            line = line_of(raw, m.start())
            body = lines[line - 1].strip() if line - 1 < len(lines) else ""
            found.append(
                f"{census.crate_of(path, root)}: {rel}:{line}: "
                f"{receiver_of(raw, m.start())}.{m['acq']}().ok()  |  {body}"
            )
    return found


def inventory(root: Path, files: list[Path]) -> list[tuple[str, str, str]]:
    """Every acquisition branched on instead of read through: `(shape, bucket, message)`.

    Not a verdict. These are per-lock decisions and some of them are correct. The buckets are
    `prod-drop` / `prod-hold` / `test-drop` / `test-hold`: drop means the poisoned value is
    unreachable on that path, hold means the arm takes the guard back with `into_inner()`. The
    point of printing them at all is that the count of open decisions comes from the scanner that
    defines the exemption, and that production and test code are separated the way
    `panic-site-census.py` separates them (by location and by `cfg(test)` span, not by a filename
    guess).
    """
    rows: list[tuple[str, str, str]] = []
    for path in files:
        rel = path.relative_to(root)
        try:
            raw = path.read_text(encoding="utf-8", errors="replace")
        except OSError as exc:  # unreadable is a scan failure, not an empty inventory
            sys.exit(f"cannot read {rel}: {exc}")
        clean = census.blank_noise(raw)
        dead = census.never_spans(clean)
        gated = census.test_spans(clean)
        by_location = census.is_test_by_location(path, root)
        lines = raw.splitlines()
        for shape, rx in BRANCHES.items():
            for m in rx.finditer(clean):
                if any(a <= m.start() < b for a, b in dead):
                    continue
                if shape == "match-Err-arm":
                    dropped = drops_the_guard(clean, m.end() - len("Err("))
                else:
                    dropped = True  # nothing is bound on the failure path
                side = "test" if by_location or any(a <= m.start() < b for a, b in gated) else "prod"
                bucket = f"{side}-{'drop' if dropped else 'hold'}"
                line = line_of(raw, m.start())
                body = lines[line - 1].strip() if line - 1 < len(lines) else ""
                rows.append((shape, bucket, f"{census.crate_of(path, root)}: {rel}:{line}:  {body}"))
    return rows


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".", type=Path)
    ap.add_argument("--verbose", action="store_true")
    ap.add_argument(
        "--inventory",
        action="store_true",
        help="count the branched-on acquisitions this gate allows, instead of checking for the "
        "hard-zero shape; always exits 0",
    )
    args = ap.parse_args()

    root = args.root.resolve()
    files = sources(root)
    if not files:
        print(
            f"lock-poison: no .rs files under {root}; refusing to report a clean tree",
            file=sys.stderr,
        )
        return 2

    if args.inventory:
        rows = inventory(root, files)
        if args.verbose:
            print(f"lock-poison: scanned {len(files)} .rs file(s) under {root}")
        for shape in BRANCHES:
            cells = "  ".join(
                f"{b} {sum(1 for s, bk, _ in rows if s == shape and bk == b):4d}"
                for b in ("prod-drop", "prod-hold", "test-drop", "test-hold")
            )
            print(f"  {shape:14s} {cells}")
        for shape, bucket, message in sorted(rows, key=lambda r: (r[1], r[0], r[2])):
            print(f"  [{bucket}] {shape:14s} {message}")
        dropped = sum(1 for _, b, _ in rows if b.endswith("-drop"))
        held = sum(1 for _, b, _ in rows if b.endswith("-hold"))
        prod = sum(1 for _, b, _ in rows if b.startswith("prod"))
        print(
            f"lock-poison inventory: {len(rows)} site(s) branch on the poison error instead of "
            f"reading through it -- {dropped} drop the guard, {held} take it back with "
            f"into_inner(); {prod} of them in production code. Allowed by design; a reviewer "
            "decides per lock."
        )
        return 0

    found = scan(root, files)
    if args.verbose:
        print(f"lock-poison: scanned {len(files)} .rs file(s) under {root}")
    if not found:
        print("lock-poison: 0 site(s); no lock error is read as an absent value")
        return 0

    print(f"lock-poison: {len(found)} site(s) where a poisoned lock is read as `None`:")
    for text in found:
        print(f"  {text}")
    print(
        "\nlock-poison: the discarded `Err` says a holder died, and it still carries the guard. "
        "Take `.unwrap_or_else(PoisonError::into_inner)` (or `lock_or_recover` / "
        "`read_or_recover` in xai-grok-shell) to read the value, or `match` on it and say what a "
        "dead holder means here. `try_lock()` is unaffected."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
