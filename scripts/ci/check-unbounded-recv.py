#!/usr/bin/env python3
"""Refuse a test that waits for a channel event with no deadline and no failure path.

The shape is `.recv().await.expect(..)` or `.recv().await.unwrap(..)`. It reads as "this event
arrives", and when it does not, nothing about that assumption is checked:

- `expect`/`unwrap` can only fire on a *closed* channel. A channel that stays open while the
  event never comes leaves the call pending forever;
- `#[tokio::test]` runs a current-thread runtime with no test-level deadline, and the CI job's
  only ceiling is `.github/workflows/ci.yml`'s `timeout-minutes`.

So the test neither passes nor fails: it holds the job open until the runner kills it. Run
37259667663 died exactly that way. The last completed test is timestamped `04:00:01`, libtest's
watchdog names `session::workflow::manager::tests::active_run_admission_is_bounded_per_session`
at `04:00:55`, and at `04:30:09` the runner terminates `cargo` and reports `cancelled` with no
failing test in the log -- 29 silent minutes, and the other seven jobs green, so nothing
correlated the failure to that suite. A failure the pipeline reports as a timeout is a failure
nobody can read, which is why the idiom was converted rather than the one hanging call.

Production code is out of scope on purpose: an unbounded `recv()` in a long-lived actor is the
design, and `.expect(..)` there is a decision about a closed channel rather than a test that
will not terminate. The scan therefore only counts sites inside test code, and "test code" is
the same verdict `scripts/ci/panic-site-census.py` reaches, from which it borrows the string
and comment blanking, the `cfg(test)` span walk and the crate attribution: a file under
`tests/`/`benches`/`examples`, a `*_test.rs`/`*_tests.rs` file (this workspace includes those
from a `#[cfg(test)] mod name;` declaration), any item whose `cfg` requires `test`, any item
carrying a test attribute, minus anything a provably-false `cfg` drops from every build.

`third_party/` is not scanned: those sources are vendored, and a hang in one is an upstream
problem to fix on the way in, not a debt this repo ratchets.

The conversion is `xai_grok_test_support::recv_wait`: `recv_bounded("what")` on a tokio
receiver, `within_option(..)` for a harness type with its own `recv()`, and a crate's own
bounded helper where the file already had one (that last form nests the `recv()` inside a call,
so it does not match the pattern below and needs no exemption).

    python3 scripts/ci/check-unbounded-recv.py [--root DIR] [--verbose] [--write-baseline]
        [--baseline PATH]

Exit 0 means every crate is at or under its recorded count in
`scripts/ci/unbounded-recv-baseline.tsv`. Exit 1 names each site above it -- and also each crate
*under* it, because a ratchet that never tightens is a ledger nobody reads. Exit 2 means the
scan found no Rust sources, which is a broken invocation, not a clean tree.
"""

from __future__ import annotations

import argparse
import importlib.util
import os
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
# The panic census already answers "is this byte inside test code?" for this workspace,
# including the cfg-expression reading that `scripts/cfg_lib.py` owns. One answer, two guards.
_CENSUS = HERE / "panic-site-census.py"
if not _CENSUS.is_file():
    raise SystemExit(f"check-unbounded-recv: {_CENSUS} is missing, so nothing here can be checked")
_census_spec = importlib.util.spec_from_file_location("panic_site_census", _CENSUS)
census = importlib.util.module_from_spec(_census_spec)
assert _census_spec.loader is not None
_census_spec.loader.exec_module(census)

# The idiom: an unbounded wait whose only failure path is a closed channel. rustfmt may break
# the chain across lines, so the scan runs over the whole text and derives line numbers from
# match offsets rather than going line by line.
SITE = re.compile(r"\.recv\s*\(\s*\)\s*\.\s*await\s*\.\s*(?:expect|unwrap)\s*\(")
# Attributes that make the item below them a test. Only the forms this workspace uses.
TEST_ATTR = re.compile(
    r"^#\[\s*(?:tokio::test|test_log::test|serial_test::serial|rstest\b|case\b|test\b)", re.M
)
# Files this workspace includes from a `#[cfg(test)] mod name;` declaration by name alone.
# Matched with `search`: the suffix is what identifies them, not the whole file name.
TEST_FILE_NAME = re.compile(r"_tests?\.rs$")
BASELINE_NAME = "unbounded-recv-baseline.tsv"
SKIP_DIRS = ("target", "node_modules", "third_party", "__pycache__")


def attr_test_spans(clean: str) -> list[tuple[int, int]]:
    """Ranges opened by an item carrying a test attribute, matched on blanked text."""
    spans: list[tuple[int, int]] = []
    for m in TEST_ATTR.finditer(clean):
        line_end = clean.find("\n", m.end())
        if line_end < 0:
            continue
        # the item: past any further attribute lines and doc comments
        cursor = line_end + 1
        while True:
            nxt = clean.find("\n", cursor)
            body = clean[cursor : nxt if nxt >= 0 else len(clean)]
            if body.strip().startswith(("#", "///", "//!", "//", "/*", "*", "]")):
                if nxt < 0:
                    break
                cursor = nxt + 1
                continue
            break
        brace = clean.find("{", cursor)
        semicolon = clean.find(";", cursor)
        if semicolon >= 0 and (brace < 0 or semicolon < brace):
            continue  # a declaration of something that lives in another file
        if brace < 0:
            continue
        spans.append((cursor, census.brace_end(clean, brace)))
    return spans


def test_spans_for(path: Path, root: Path, clean: str) -> tuple[list[tuple[int, int]], list[tuple[int, int]]]:
    """The byte ranges that are test code, and the ranges no build contains.

    Both are returned rather than the first minus the second, because the difference has to be
    taken at the site's own offset: a `#[cfg(any())]` module parked inside a `#[cfg(test)] mod`
    sits *within* a test range, and dropping only ranges that are wholly inside a dead one left
    every site in it counted.
    """
    dead = census.never_spans(clean)
    if census.is_test_by_location(path, root) or TEST_FILE_NAME.search(path.name):
        whole = [(0, len(clean))]
    else:
        whole = []
    return whole + census.test_spans(clean) + attr_test_spans(clean), dead


def in_span(spans: list[tuple[int, int]], offset: int) -> bool:
    return any(a <= offset < b for a, b in spans)


def line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def sources(root: Path) -> list[Path]:
    """Every `.rs` under `root`, with the ignored directories pruned on the way down.

    `rglob` would descend into `target/`, which in a built workspace is hundreds of
    thousands of files, and this gate runs on every CI job.
    """
    found: list[Path] = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
        for name in sorted(filenames):
            if name.endswith(".rs"):
                found.append(Path(dirpath) / name)
    return sorted(found)


def scan(root: Path) -> dict[str, list[tuple[int, str]]]:
    """crate -> [(line number, source text)] for every violating site in test code."""
    found: dict[str, list[tuple[int, str]]] = {}
    for path in sources(root):
        rel = path.relative_to(root)
        try:
            raw = path.read_text(encoding="utf-8", errors="replace")
        except OSError as exc:  # unreadable is a scan failure, not a clean tree
            sys.exit(f"cannot read {rel}: {exc}")
        clean = census.blank_noise(raw)
        spans, dead = test_spans_for(path, root, clean)
        for m in SITE.finditer(clean):
            if not in_span(spans, m.start()) or in_span(dead, m.start()):
                continue
            line = line_of(raw, m.start())
            crate = census.crate_of(path, root)
            found.setdefault(crate, []).append((line, f"{rel}:{line}: {raw.splitlines()[line - 1].strip()}"))
    return found


def read_baseline(path: Path) -> dict[str, int]:
    limits: dict[str, int] = {}
    if not path.exists():
        return limits
    for raw in path.read_text(encoding="utf-8").split("\n"):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = raw.split("\t")
        if len(parts) < 2:
            sys.exit(f"{path.name}: malformed row: {raw!r}")
        limits[parts[0]] = int(parts[1])
    return limits


def write_baseline(path: Path, counts: dict[str, int]) -> None:
    rows = [
        "# unbounded `.recv().await.expect(...)`/`.unwrap()` waits in test code, per crate,",
        "# from scripts/ci/check-unbounded-recv.py. Lower the number when a batch converts",
        "# more sites; the guard refuses a crate that sits under its own row as well, so a",
        "# converted site cannot leave the ceiling behind.",
    ]
    rows += [f"{crate}\t{counts.get(crate, 0)}" for crate in sorted(counts)]
    rows.append(f"TOTAL\t{sum(counts.values())}")
    path.write_text("\n".join(rows) + "\n", encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".", type=Path)
    ap.add_argument("--baseline", default=None, type=Path)
    ap.add_argument("--verbose", action="store_true")
    ap.add_argument("--write-baseline", action="store_true")
    args = ap.parse_args()

    root = args.root.resolve()
    if not sources(root):
        print(
            f"unbounded-recv: no .rs files under {root}; refusing to report a clean tree",
            file=sys.stderr,
        )
        return 2

    baseline = args.baseline or HERE / BASELINE_NAME
    found = scan(root)
    counts = {crate: len(sites) for crate, sites in found.items()}

    if args.write_baseline:
        write_baseline(baseline, counts)
        print(f"unbounded-recv: wrote {baseline} ({sum(counts.values())} sites)")
        return 0

    limits = read_baseline(baseline)
    names = sorted(set(counts) | {c for c, n in limits.items() if c != "TOTAL" and n})
    over: list[str] = []
    under: list[str] = []
    for crate in names:
        got, want = counts.get(crate, 0), limits.get(crate, 0)
        if got > want:
            over.append(crate)
            if not args.verbose:
                for _, text in found[crate]:
                    print(f"  {crate}: {text}")
        elif got < want:
            under.append(f"{crate} ({want} -> {got})")

    total = sum(counts.values())
    if args.verbose or total:
        print(
            f"unbounded-recv: {total} test-side `.recv().await.expect/unwrap` site(s) in "
            f"{len(counts)} crate(s); baseline {baseline.name} allows "
            f"{sum(n for c, n in limits.items() if c != 'TOTAL')}"
        )
    if args.verbose:
        for crate, sites in sorted(found.items()):
            for _, text in sites:
                print(f"  {crate}: {text}")

    if over:
        print(
            f"\nunbounded-recv: {len(over)} crate(s) above baseline. Each site above waits for a "
            "channel event with no deadline; the test cannot fail, only the CI job can. "
            "Route it through xai_grok_test_support::recv_wait (recv_bounded / within_option) or "
            "the file's own bounded helper."
        )
        return 1
    if under:
        print(
            f"\nunbounded-recv: {len(under)} crate(s) below baseline: {', '.join(under)}. "
            f"Lower the row in {baseline.name} so the ceiling keeps the floor you earned."
        )
        return 1
    print(f"unbounded-recv: {total} site(s), every crate at or under its recorded ceiling")
    return 0


if __name__ == "__main__":
    sys.exit(main())
