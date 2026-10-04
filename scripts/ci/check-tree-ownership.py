#!/usr/bin/env python3
"""No path in the checkout may be owned by anyone but the owner of the checkout itself.

`scripts/ci/check-container-hygiene.py` keeps the container entry point from writing root-owned
files into the developer's working tree, but it is a static read of shell text. Its own docstring
lists what it cannot see: a mount path assembled at runtime, a `docker run` assembled inside a
variable. This script is the other half. It walks the tree and measures ownership, so it catches
the shape the static rule was written for -- and any other shape that produces the same damage --
at the moment the damage exists, from inside the container that caused it.

Why the owner of the root directory is the thing to compare against: the checkout arrives through
a bind mount, so its root carries the uid of whoever cloned it, while anything the container
creates carries root. Comparing against a literal uid 0 would break on a machine that has no
POSIX ownership at all -- on Windows every path reports uid 0, including the root, so every path
would be an intruder. Comparing against the root's own owner says "nothing here disagrees with the
directory that contains it", which is the actual claim, is silent on Windows without a platform
special case, and is silent on a host where everything legitimately belongs to one user.

Three things are measured and skipped rather than judged, each for a reason that is not convenience:

* Symlinks are statted, never followed. A link out of the tree cannot make this script judge, or
  read, files that are not in the checkout. A dangling link is a path like any other; it has an
  owner and it is judged.
* A directory whose name is in `--prune` (default `target`) is skipped, itself and its contents.
  Build output was never part of the checkout, and inside the container the name is the mount point
  of the cargo volume, so what sits there is legitimately root-owned and judging it is a false
  positive. The leak this carve-out could hide -- a `target/` the runtime left behind in the
  checkout -- is the thing `check-container-hygiene.py` forbids statically, and it is measured from
  the host, where the volume is not mounted over it.
* A mount point under the root is skipped, itself and its subtree. What was deliberately mounted
  there came from somewhere else on purpose, and asking who owns it is the wrong question. A
  checkout that is itself a mount, which is what `/src` is inside the container, is still walked:
  the root is never one of the entries of its own listing.

Findings name the uid and the remedy, because the point of the rule is that the developer cannot
clean this up with the privileges they have.

    python3 scripts/ci/check-tree-ownership.py [--root PATH] [--assume-owner-uid N]
        [--prune NAME] [--mounts PATH] [--max-findings N] [--verbose]

There is no allow list (see `scripts/ci/check-script-portability.py` for the house policy): a
finding here is fixed by removing the path or by stopping whatever created it.
"""

from __future__ import annotations

import argparse
import os
import pwd
import sys
from pathlib import Path

#: Directory names skipped outright by default. `target` is cargo's build output, which the
#: container reaches through a named volume instead of through the checkout.
DEFAULT_PRUNES = ("target",)

#: Where the kernel lists the mounts of the current namespace. Absent on platforms that have no
#: such file, which leaves the rule with nothing to skip.
DEFAULT_MOUNTINFO = "/proc/self/mountinfo"

#: Beyond this, findings are counted and summarised rather than printed: a real leak under a build
#: tree can be thousands of paths, and a verdict nobody can read is not a verdict.
DEFAULT_MAX_FINDINGS = 25


def user_name(uid: int) -> str:
    """`root (0)` rather than `0`, because the message is for a human reading a failed gate."""
    try:
        return f"{pwd.getpwuid(uid).pw_name} ({uid})"
    except KeyError:
        return str(uid)


def unescape_mount_field(value: str) -> str:
    """Undo the octal escapes `mountinfo` uses for the characters that would break its columns."""
    for escaped, plain in (("\\040", " "), ("\\011", "\t"), ("\\012", "\n"), ("\\134", "\\")):
        value = value.replace(escaped, plain)
    return value


def read_mount_points(path: Path) -> frozenset[str]:
    """The set of paths that are mount points, from a `mountinfo`-formatted file.

    Field 5 of each line is the mount point. A file that cannot be read yields an empty set: on a
    platform without it there is nothing to skip, and that is not a failure.
    """
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return frozenset()
    points = set()
    for line in text.splitlines():
        fields = line.split()
        if len(fields) >= 5:
            points.add(unescape_mount_field(fields[4]))
    return frozenset(points)


def walk(root: Path, prunes: tuple[str, ...], mounts: frozenset[str]):
    """Return (paths to judge, paths skipped outright) under root, without following symlinks.

    An entry named in `prunes`, or named by `mounts`, is counted as skipped and not descended into.
    The root of the walk is never among the entries of its own listing, so a checkout that is itself
    a mount -- which is what `/src` is inside the container -- is always walked.
    """
    judged: list[tuple[Path, os.stat_result | None]] = []
    skipped = 0
    stack = [root]
    while stack:
        path = stack.pop()
        try:
            entries = list(os.scandir(path))
        except OSError:
            judged.append((path, None))
            continue
        for entry in entries:
            if entry.name in prunes:
                skipped += 1
                continue
            child = Path(entry.path)
            if str(child) in mounts:
                skipped += 1
                continue
            try:
                judged.append((child, entry.stat(follow_symlinks=False)))
            except OSError:
                judged.append((child, None))
                continue
            if entry.is_dir(follow_symlinks=False):
                stack.append(child)
    return judged, skipped


def scan(root: Path, assume_owner_uid: int | None, prunes: tuple[str, ...],
         mounts: frozenset[str], max_findings: int):
    """Return (walked, foreign, unreadable, skipped, findings).

    `max_findings` caps what is printed, never what is counted: the counts are what the verdict line
    reports, so a leak of four thousand paths is still four thousand.
    """
    try:
        root_stat = root.stat()
    except OSError as exc:
        return 0, 0, 0, 0, [f"tree-ownership: cannot stat {root}: {exc}"]
    expected = root_stat.st_uid if assume_owner_uid is None else assume_owner_uid

    judged, skipped = walk(root, prunes, mounts)
    findings: list[str] = []
    walked = 0
    foreign = 0
    unreadable = 0
    for path, st in judged:
        walked += 1
        rel = str(root) if path == root else str(path.relative_to(root))
        if st is None:
            unreadable += 1
            if len(findings) < max_findings:
                findings.append(
                    f"{rel}: cannot be read, so its contents cannot be judged; a directory the "
                    "container created is unreadable to the owner of the tree in exactly this way"
                )
            continue
        if st.st_uid == expected:
            continue
        foreign += 1
        if len(findings) < max_findings:
            findings.append(
                f"{rel}: owned by {user_name(st.st_uid)}, not by {user_name(expected)}, which owns "
                f"{root.name}; it came from the other side of the mount, and the owner of the "
                "checkout cannot delete a directory the container created -- remove it with a "
                "privileged shell and stop whatever writes through the mount from doing it again"
            )
    hidden = foreign + unreadable - len(findings)
    if hidden > 0:
        findings.append(
            f"... {hidden} more path(s) with the same problem, raise --max-findings to see them"
        )
    return walked, foreign, unreadable, skipped, findings


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=".", type=Path, help="checkout to measure")
    parser.add_argument(
        "--assume-owner-uid",
        type=int,
        default=None,
        help="treat this uid as the expected owner instead of the owner of --root; it exists so "
             "the walk can be tested without a second uid on the machine",
    )
    parser.add_argument(
        "--prune",
        action="append",
        dest="prunes",
        metavar="NAME",
        help="directory name to skip outright, itself and its contents; repeatable, defaults to "
             f"{', '.join(DEFAULT_PRUNES)}",
    )
    parser.add_argument(
        "--mounts",
        default=Path(DEFAULT_MOUNTINFO),
        type=Path,
        metavar="PATH",
        help="mountinfo-formatted file used to skip mounted paths, defaults to "
             f"{DEFAULT_MOUNTINFO}; a file that cannot be read skips nothing",
    )
    parser.add_argument("--max-findings", type=int, default=DEFAULT_MAX_FINDINGS)
    parser.add_argument(
        "--verbose", action="store_true", help="print the expected owner and what was skipped"
    )
    args = parser.parse_args(argv)

    root = args.root.resolve()
    if not root.is_dir():
        print(f"tree-ownership: {root} is not a directory", file=sys.stderr)
        return 1
    prunes = tuple(args.prunes) if args.prunes else DEFAULT_PRUNES
    mounts = read_mount_points(args.mounts)

    walked, foreign, unreadable, skipped, findings = scan(
        root, args.assume_owner_uid, prunes, mounts, args.max_findings
    )
    if walked == 0:
        print(
            f"tree-ownership: nothing walked under {root}; point --root at the checkout",
            file=sys.stderr,
        )
        return 1

    expected = (root.stat().st_uid if args.assume_owner_uid is None else args.assume_owner_uid)
    if args.verbose:
        print(f"  expected owner: {user_name(expected)}")
        print(f"  skipped directory names: {', '.join(prunes) if prunes else 'none'}")
        print(f"  mount points known: {len(mounts)}")

    for finding in findings:
        print(finding, file=sys.stderr)

    if foreign or unreadable:
        print(
            f"tree-ownership: FAIL ({foreign} path(s) not owned by {user_name(expected)}, "
            f"{unreadable} unreadable, {walked} path(s) walked under {root}, "
            f"{skipped} path(s) skipped)",
            file=sys.stderr,
        )
        return 1
    print(
        f"tree-ownership: OK ({walked} path(s) walked under {root.name}, all owned by "
        f"{user_name(expected)}, {skipped} path(s) skipped as pruned or mounted)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
