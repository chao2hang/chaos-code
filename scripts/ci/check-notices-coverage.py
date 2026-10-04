#!/usr/bin/env python3
"""Reject a third-party notices document that does not cover what the shipped binary contains.

`THIRD-PARTY-NOTICES` is maintained against a moving target. The released binary is
`cargo build -p xai-grok-pager-bin`, and everything that crate reaches through dependency edges
that are not test-only is code inside it -- 1139 third-party packages as of 2026-10-05, of which 101
names appear at two versions, because a binary that links both is linked against both. Each one is a separate
work of authorship whose license and copyright notice have to travel with the binary.

Dependency bumps are routine and none of them touches this file. So the failure is not that the
document becomes wrong in a way someone can see: a crate goes from 1.2.3 to 1.2.4, the entry says
1.2.3, and the only observable effect is that a released binary carries a notice for a version it
does not contain while carrying nothing for the one it does. `check-notices-document.py` cannot
catch that, because the stale entry reads perfectly well. Only comparing against what the build
actually resolves can.

That is what this guard does, in one direction only where it matters:

1. Compute the shipped set: `cargo metadata --frozen`, follow the non-dev dependency edges out of
   `xai-grok-pager-bin`, drop this workspace's own crates, keep the vendored ones under
   `third_party/` because those are compiled into the binary too. The declared-edge closure is
   taken over every target and feature combination, so it is a superset of what any single release
   build links -- being absent from it is therefore a strong statement, and being present is only
   the obligation to look.
2. A shipped package with no entry: the binary carries code with no notice anywhere in the
   distribution. Failure.
3. A package name whose entry's version is not the version shipped: the notice on the reader's
   screen is for different source than the code in front of them. Failure.
4. An entry for a `(name, version)` the build does not reach: coverage of nothing, and usually the
   sign that a removal went in without anyone deciding the notice could go too. Failure.
5. An entry that records a declaration upstream does not make, or makes differently: the parenthetical
   `(upstream declares: ...)` is a claim about the package, so it is checked against the package.
6. A license expression this project's rules cannot decide a term for. Failure.

`cargo metadata` needs the registry, which the CI container does not have, so this guard runs on
the host and in the `rust` CI job (after `cargo check`, when the registry is warm) and is listed in
`scripts/ci/docker-entry-ci-only.tsv`. `--metadata FILE` reads a saved `cargo metadata` JSON
instead of running cargo: that is how the fixtures below exercise every branch, and it is what lets
someone audit this guard's reading of a real workspace without a toolchain.

Failing to compute the shipped set is a failure, never a skip: a guard that goes quiet when cargo
is missing reports exactly the run in which it proved nothing.

Usage: python3 scripts/ci/check-notices-coverage.py [--root DIR] [--metadata FILE] [--max-list N]
Exit: 0 = every package in the binary has a current entry, and no entry describes a package that
      is not in the binary.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from pathlib import Path

GUARD = "check-notices-coverage"
LIB = "notices_lib.py"
SAMPLE = 12


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


def listed(items: list[str], limit: int) -> str:
    if len(items) <= limit:
        return ", ".join(items)
    head = ", ".join(items[:limit])
    return f"{head} (and {len(items) - limit} more)"


def check(document: object, dependencies: list[object], lib: object,
          limit: int) -> tuple[list[str], int]:
    problems: list[str] = []
    result = lib.triage(document, dependencies)
    sample = max(1, limit)

    if result.missing:
        problems.append(
            f"{len(result.missing)} shipped package(s) with no entry; run "
            f"scripts/gen-third-party-notices.py --write. First: "
            f"{listed([f'{dep.name} {dep.version}' for dep in result.missing], sample)}"
        )
    if result.drifted:
        moved = [
            f"{dep.name}: entry says {entry.version}, the build resolves {dep.version}"
            for dep, entry in result.drifted
        ]
        problems.append(
            f"{len(result.drifted)} entry(ies) naming a version that is not what ships, so the "
            f"notice on the page describes different source than the code in front of the reader. "
            f"First: {listed(moved, sample)}"
        )
    if result.unshipped:
        problems.append(
            f"{len(result.unshipped)} entry(ies) for a package version the build does not reach. "
            f"First: {listed([f'{entry.name} {entry.version}' for entry in result.unshipped], sample)}"
        )
    if result.refused:
        problems.append(
            f"{len(result.refused)} shipped package(s) whose license this tool cannot decide a term "
            f"for. First: {listed([f'{name}: {why}' for name, why in result.refused], sample)}"
        )

    # The parenthetical is a claim about the package, so it is compared against the package rather
    # than against the document: an entry can be internally consistent and still describe a
    # declaration upstream never made.
    shipped = {dep.key: dep for dep in dependencies}
    stale = []
    for entry in document.entries:
        dep = shipped.get(entry.key)
        if dep is None:
            continue
        recorded = entry.declared()
        if not recorded:
            continue
        if " ".join(recorded.split()) != " ".join(dep.expression.split()):
            stale.append(f"{entry.name} {entry.version}: entry says {recorded!r}, "
                         f"the package declares {dep.expression!r}")
    if stale:
        problems.append(
            f"{len(stale)} entry(ies) recording a license declaration that differs from the "
            f"package's own. First: {listed(stale, sample)}"
        )
    return problems, len(dependencies)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=".", type=Path)
    parser.add_argument("--metadata", default=None, type=Path,
                        help="a saved `cargo metadata --format-version 1` JSON, in place of cargo")
    parser.add_argument("--max-list", default=SAMPLE, type=int,
                        help="how many examples of each finding to print")
    ns = parser.parse_args(argv)

    lib = load_lib()
    repo = ns.root.resolve()
    document_path = repo / lib.DOCUMENT
    if not document_path.is_file():
        print(f"{GUARD}: {document_path} is not a file; run this from the repository root",
              file=sys.stderr)
        return 2

    # Read separately, and reported separately: a document that does not parse is not a build that
    # changed, and a maintainer told the dependency set could not be computed goes looking at cargo.
    try:
        document = lib.Document(document_path.read_text(encoding="utf-8"))
    except (lib.Unreadable, OSError, UnicodeDecodeError) as exc:
        print(f"{GUARD}: FAIL (1 problem(s))")
        print(f"  {lib.DOCUMENT} could not be read: {exc}")
        return 1
    try:
        if ns.metadata:
            metadata = json.loads(ns.metadata.read_text(encoding="utf-8"))
        else:
            metadata = lib.load_metadata(repo)
        dependencies = lib.shipped_dependencies(repo, metadata)
    except (lib.Unreadable, OSError, json.JSONDecodeError) as exc:
        print(f"{GUARD}: FAIL (1 problem(s))")
        print(f"  the shipped dependency set could not be computed: {exc}", file=sys.stdout)
        return 1
    if not dependencies:
        print(f"{GUARD}: FAIL (1 problem(s))")
        print(f"  the build reaches no third-party package, which means the reading of "
              f"`cargo metadata` is wrong rather than that the binary has no dependencies")
        return 1

    problems, total = check(document, dependencies, lib, ns.max_list)
    if problems:
        print(f"{GUARD}: FAIL ({len(problems)} problem(s))")
        for problem in problems:
            print(f"  {problem}")
        return 1
    print(f"{GUARD}: OK ({total} shipped package(s), every one of them covered by a current "
          f"entry, and no entry left behind)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
