#!/usr/bin/env python3
"""Read what this repository ships, and read the notices document that is supposed to cover it.

`THIRD-PARTY-NOTICES` is a legal artifact: the copy of every third-party license text and copyright
notice that a user of the `chaos` binary is owed. It is a committed file, written by hand and by a
one-off script upstream, and it is 18 738 lines long. Nothing in this repository could previously
answer the only question that matters about it: does it still cover the dependencies this binary is
built from? `scripts/ci/check-notices-coverage.py` answers that question against the build, and
`scripts/ci/check-notices-document.py` checks the document's own internal rules;
`scripts/gen-third-party-notices.py` writes the entries that the first says are missing.

Both of them need the same two readings, and they have to agree exactly -- a gate that computes the
shipped set a different way from the generator would audit a different product than the one
packaging ships. That is why the reading lives here instead of inside either tool:

- the dependency set: `cargo metadata`, then walk the dependency graph from the one crate that
  builds a shipped binary, following the edges that are not dev-dependencies. Dev-only crates are
  not in the binary and are not owed a notice; this repository's own crates are not third-party,
  except the ones vendored under `third_party/`, which are third-party code we redistributed with
  modifications and therefore need the notice more than most.
  The walk is target-agnostic on purpose: `cargo metadata` reports every platform-gated edge, and
  the release builds several platforms, so a Windows-only crate is owed its notice by the Windows
  artifact. That is why the set is larger than `cargo tree -e normal` on one host -- measured on
  this checkout, 177 of the 998 shipped names are absent from that host-target tree, 12 of them
  because they are reached only through a build script. The comparison is worth running the other
  way too, and it is the useful direction: no third-party package on this host's normal edges is
  missing from the set.
- the document itself: a Part I of per-package entries and a Part II of license texts. Entry bodies
  carry prose that no tool should overwrite -- upstream `NOTICE` files reproduced verbatim, and
  `VENDORED WITH LOCAL MODIFICATIONS:` blocks describing what we changed in `third_party/` -- so the
  reader returns bodies verbatim and nobody rewrites an entry it did not create.

The license a package is distributed under is a choice when upstream declares alternatives, and the
rule for that choice was derived from the committed document rather than invented here: it reproduced
the license recorded by all 692 entries that declared alternatives in the hand-written document it was
derived from, and `scripts/ci/check-notices-document.py` re-checks every one of them on every commit.
See LICENSE_PREFERENCE below and `docs/verification/` for the measurement.

Usage: imported by the two tools named above; `python3 scripts/notices_lib.py [REPO]` prints a
summary of both readings.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
from pathlib import Path

DOCUMENT = "THIRD-PARTY-NOTICES"
# The only crate that produces a shipped binary: `xai-grok-pager-bin` builds `chaos`, which is what
# the npm platform packages and the GitHub Release assets contain. A notice is owed for the
# dependencies of what we ship, so this is the root of the walk.
SHIPPED_ROOT = "xai-grok-pager-bin"
# Workspace members are ours, not third-party. Members under this prefix are the exception: they
# are upstream code vendored into the tree, and their notices say so.
VENDORED_PREFIX = "/third_party/"
PART_I = "PART I — PER-PACKAGE ENTRIES"
PART_II = "PART II — LICENSE TEXTS"
RULE = "-" * 80
BANNER = "=" * 80

# When upstream declares several licenses, the document distributes under one of them and says so.
# This order reproduces the choice recorded for every one of the 25 distinct declared expressions in
# the committed document, which is the only authority available for a rule like this: MIT over
# Apache-2.0 (90 entries), BSD over Apache-2.0 when MIT is absent, Apache-2.0 over BSL-1.0, and
# neither CC0-1.0 nor MIT-0 nor Unlicense preferred over any of those. A declared license outside
# ALLOWED_LICENSES is a finding wherever it appears, so the tail of this list only ever decides the
# order of an expression whose members are all allowed.
LICENSE_PREFERENCE = (
    "MIT",
    "BSD-3-Clause",
    "BSD-2-Clause",
    "Apache-2.0",
    "ISC",
    "Zlib",
    "BSL-1.0",
    "0BSD",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "MPL-2.0",
    "CDLA-Permissive-2.0",
    "EPL-2.0",
    "WTFPL",
)
# The licenses this repository has decided it ships under. A license absent from this set is not
# silently accepted: `choose_licenses` refuses it and the caller has to say what happened.
# Everything here except MPL-2.0, EPL-2.0 and WTFPL is permissive. The three that are not were not
# chosen freely: MPL-2.0 is what `smartstring` (as `MPL-2.0+`) and the cssparser/nucleo family
# declare on edges that reach the released binary, so leaving its text out would not remove the
# obligation, only break it; EPL-2.0 and WTFPL are the terms `colored_json` and `terminfo` ask for,
# and those two reach this workspace through test edges only, which is why no entry in the shipped
# document names them today. They stay in the set because the decision being recorded is what this
# project is willing to ship under, not what today's graph happens to contain. Both weak copyleft
# licenses are bounded by the module or the file they cover, and WTFPL asks for nothing at all; none
# of the three reaches the code around it, which is why keeping them beat replacing the crate.
ALLOWED_LICENSES = frozenset(
    (
        "MIT",
        "MIT-0",
        "Apache-2.0",
        "ISC",
        "Zlib",
        "BSD-2-Clause",
        "BSD-3-Clause",
        "0BSD",
        "BSL-1.0",
        "Unlicense",
        "CC0-1.0",
        "Unicode-3.0",
        "Unicode-DFS-2016",
        "MPL-2.0",
        "CDLA-Permissive-2.0",
        "Apache-2.0 WITH LLVM-exception",
        "EPL-2.0",
        "WTFPL",
    )
)
# `MPL-2.0+` says "this version or any later one", so the text owed is MPL-2.0's and the choice of
# section to point at is not open. Canonicalising here keeps the document's Part II ids closed.
CANONICAL_LICENSES = {"MPL-2.0+": "MPL-2.0"}

# Files a crate may carry its license and attribution in. Names are matched case-insensitively and
# only at the package root: a test fixture called `LICENSE` inside `tests/` is not the license.
LICENSE_FILE_RE = re.compile(r"^(license|copying|copyright|notice|authors|unlicense)([.\-].*)?$", re.I)
COPYRIGHT_HEAD_RE = re.compile(r"^copyright\s*(?:\(c\)|©|copyright)?[:\s-]*", re.I)
# A real attribution line names whoever holds the copyright: a year, a name, an organisation, an
# email. The license boilerplate that every crate ships repeats the word "copyright" dozens of times
# -- Apache-2.0 has a line beginning "copyright license to reproduce", CC0 has "Copyright and
# related" -- so the word alone proves nothing and the token after it is what decides. Placeholders
# (`Copyright [yyyy] [name of copyright owner]`, `Copyright <year>`) are rejected: they name nobody.
PLACEHOLDER_START = ("[", "<", "{")


def _is_copyright_line(line: str) -> bool:
    if not COPYRIGHT_HEAD_RE.match(line):
        return False
    rest = COPYRIGHT_HEAD_RE.sub("", line, count=1).strip()
    if not rest:
        return False
    first = rest.split()[0]
    if first.startswith(PLACEHOLDER_START):
        return False
    if re.match(r"^\d{4}", first) or "@" in first:
        return True
    return first[:1].isupper() or first[:2].isupper()


# `spdx-license-identifier: MIT` is a machine annotation, not an attribution.
SPDX_ONLY_RE = re.compile(r"^spdx-license-identifier", re.I)


class Unreadable(Exception):
    """Something this module could not read. Never a pass: the caller reports and exits non-zero."""


class Dependency:
    """One third-party package inside a shipped binary."""

    __slots__ = ("name", "version", "expression", "url", "authors", "vendored", "manifest")

    def __init__(
        self,
        name: str,
        version: str,
        expression: str,
        url: str,
        authors: tuple[str, ...],
        vendored: bool,
        manifest: str,
    ) -> None:
        self.name = name
        self.version = version
        self.expression = expression
        self.url = url
        self.authors = authors
        self.vendored = vendored
        self.manifest = manifest

    @property
    def key(self) -> tuple[str, str]:
        return (self.name, self.version)

    def licenses(self) -> list[str]:
        return choose_licenses(self.expression)


def load_metadata(repo: Path) -> dict:
    """`cargo metadata` for this workspace, resolved offline and without touching Cargo.lock.

    `--frozen` is `--locked --offline`: the resolution in Cargo.lock is what a release build uses,
    so it is the resolution the notices have to describe, and a gate that could re-resolve would
    both dirty the working tree and describe a dependency set nobody shipped.
    """
    if not (repo / "Cargo.lock").is_file():
        raise Unreadable(f"{repo / 'Cargo.lock'} is not a file, so this is not the repository root")
    try:
        proc = subprocess.run(
            ["cargo", "metadata", "--frozen", "--format-version", "1"],
            cwd=repo,
            capture_output=True,
            text=True,
            timeout=600,
            check=False,
        )
    except FileNotFoundError as exc:  # pragma no cover - cargo is on PATH in every documented runner
        raise Unreadable("cargo is not on PATH, so the shipped dependency set cannot be computed") from exc
    except subprocess.TimeoutExpired as exc:  # pragma no cover - 600s is far beyond the 3s it takes
        raise Unreadable("cargo metadata did not return within 600 seconds") from exc
    if proc.returncode != 0:
        detail = next((line for line in reversed(proc.stderr.splitlines()) if line.strip()), "")
        raise Unreadable(
            "cargo metadata --offline failed, so the shipped dependency set cannot be computed"
            + (f": {detail}" if detail else "")
        )
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise Unreadable(f"cargo metadata printed something that is not JSON: {exc}") from exc


def _dev_only(kinds: list[dict]) -> bool:
    """Whether every way this edge is used is a test or benchmark.

    `cargo metadata` records the kind of an edge in `resolve.nodes[].deps[].dep_kinds[].kind`, as
    `null` for a normal dependency, `"dev"` for a test/bench dependency and `"build"` for a build
    script dependency; that is the only place the information appears, because the same crate can be
    both a normal and a dev dependency of one package and then carries one entry per use. `build`
    edges are kept: build-script code is compiled from the same sources and it is the conservative
    direction, since an edge dropped by mistake only ever shrinks the set the notices have to cover.
    """
    return bool(kinds) and all(kind.get("kind") == "dev" for kind in kinds)


def _non_dev_edges(metadata: dict) -> dict[str, list[str]]:
    """Dependency edges that a release build can link, keyed by package id."""
    graph: dict[str, list[str]] = {}
    for node in metadata.get("resolve", {}).get("nodes", []):
        kept: list[str] = []
        for edge in node.get("deps", []):
            kinds = edge.get("dep_kinds") or []
            # A dep with no dep_kinds at all is a rustc/cargo internal edge; keeping it is the
            # conservative direction, because dropping an edge can only ever shrink the set.
            if _dev_only(kinds):
                continue
            if edge.get("pkg") is None:
                continue
            kept.append(edge["pkg"])
        graph[node["id"]] = kept
    return graph


def shipped_dependencies(repo: Path, metadata: dict | None = None) -> list[Dependency]:
    """Every third-party package reachable from the shipped binary without going through a test."""
    metadata = metadata or load_metadata(repo)
    packages = {package["id"]: package for package in metadata.get("packages", [])}
    if not packages:
        raise Unreadable("cargo metadata reported no packages")
    members = set(metadata.get("workspace_members", []))
    graph = _non_dev_edges(metadata)
    roots = [pid for pid, package in packages.items() if package["name"] == SHIPPED_ROOT]
    if not roots:
        raise Unreadable(f"no package named {SHIPPED_ROOT}, the crate that builds the shipped binary")
    seen: set[str] = set()
    stack = list(roots)
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        stack.extend(graph.get(pid, []))
    found: dict[tuple[str, str], Dependency] = {}
    for pid in seen:
        package = packages[pid]
        manifest = package.get("manifest_path", "")
        member = pid in members
        vendored = VENDORED_PREFIX in manifest.replace(os.sep, "/")
        if member and not vendored:
            continue
        expression = (package.get("license") or "").strip()
        url = package.get("repository") or package.get("homepage") or ""
        dependency = Dependency(
            name=package["name"],
            version=package["version"],
            expression=expression,
            url=url,
            authors=tuple(package.get("authors") or ()),
            vendored=vendored,
            manifest=manifest,
        )
        # Two versions of one crate in one binary is ordinary here -- 101 names do right now -- and
        # each version is a separate work of authorship that is owed its own notice.
        # The key is therefore the pair, and an entry is only coverage if its version matches.
        found.setdefault(dependency.key, dependency)
    return sorted(found.values(), key=lambda dep: (dep.name, version_key(dep.version), dep.version))


def version_key(version: str) -> tuple[int, str]:
    """Sort 0.9 before 0.10, which a plain string sort gets backwards."""
    parts = re.split(r"[.\-+]", version)
    return tuple((0, int(part)) if part.isdigit() else (1, part) for part in parts)


# The pre-SPDX convention crates.io still accepts: `MIT/Apache-2.0`, which means `MIT OR
# Apache-2.0`. 78 shipped packages declare it that way, 3 of them with spaces around the slash, so
# it is normalised rather than refused. The lookarounds keep a leading or trailing slash from being
# read as an operator.
SLASH_FORM_RE = re.compile(r"(?<=[A-Za-z0-9.+-])\s*/\s*(?=[A-Za-z0-9])")
TOKEN_RE = re.compile(
    r"\(|\)|\band\b|\bor\b|"
    r"[A-Za-z0-9][A-Za-z0-9.+-]*(?:\s+[Ww][Ii][Tt][Hh]\s+[A-Za-z0-9][A-Za-z0-9.+-]*)?"
)


def parse_expression(expression: str) -> tuple:
    """A declared license expression as a node tree: ("id", x), ("or", [...]) or ("and", [...]).

    `+` postfixes (`MPL-2.0+`, "or any later version") are kept as written, because the text we
    owe is the text of the version named; the slash form is folded into `OR` first. Parentheses are
    honoured at both depths, so `ISC AND (Apache-2.0 OR ISC)` and `(MIT OR Apache-2.0) AND
    Unicode-3.0` -- 6 shipped packages between them -- are read rather than refused.
    """
    if not expression:
        raise Unreadable("no license expression to read")
    tokens = TOKEN_RE.findall(SLASH_FORM_RE.sub(" OR ", expression))
    tokens = [token.upper() if token.upper() in ("AND", "OR") else token for token in tokens]
    if not tokens:
        raise Unreadable(f"license expression {expression!r} contains nothing readable")
    position = 0

    def atom() -> tuple:
        nonlocal position
        if position >= len(tokens):
            raise Unreadable(f"license expression {expression!r} ends mid-expression")
        if tokens[position] == "(":
            position += 1
            inner = and_group()
            if position >= len(tokens) or tokens[position] != ")":
                raise Unreadable(f"unbalanced parentheses in license expression {expression!r}")
            position += 1
            return inner
        value = tokens[position]
        position += 1
        if value in (")", "AND", "OR"):
            raise Unreadable(f"license expression {expression!r} has a stray {value!r}")
        return ("id", value)

    def or_group() -> tuple:
        nonlocal position
        alternatives = [atom()]
        while position < len(tokens) and tokens[position] == "OR":
            position += 1
            alternatives.append(atom())
        return alternatives[0] if len(alternatives) == 1 else ("or", alternatives)

    def and_group() -> tuple:
        nonlocal position
        conjuncts = [or_group()]
        while position < len(tokens) and tokens[position] == "AND":
            position += 1
            conjuncts.append(or_group())
        return conjuncts[0] if len(conjuncts) == 1 else ("and", conjuncts)

    tree = and_group()
    if position != len(tokens):
        raise Unreadable(f"license expression {expression!r} has trailing input at {tokens[position]!r}")
    return tree


def sort_licenses(license_ids) -> list[str]:
    """Deterministic display order for a set of ids: LICENSE_PREFERENCE first, then alphabetical.

    The 11 multi-term entries in the committed document follow no single ordering -- this one matches
    4 of them, plain alphabetical 6 -- so a writer has to pick one, and this is the order that also
    decides the license choice.
    """
    return sorted(set(license_ids), key=_rank)


def expression_terms(expression: str) -> set[str]:
    """Every license id named anywhere in a declared expression, canonicalised.

    `choose_licenses` says what we distribute under; this says what upstream could ever have meant,
    which is the set an entry may legitimately record. An entry naming a license outside it has
    invented a term upstream never declared.
    """
    node = parse_expression(expression)
    found: set[str] = set()
    stack = [node]
    while stack:
        current = stack.pop()
        if current[0] == "id":
            found.add(CANONICAL_LICENSES.get(current[1], current[1]))
        else:
            stack.extend(current[1])
    return found


def _rank(license_id: str) -> tuple[int, str]:
    try:
        return (LICENSE_PREFERENCE.index(license_id), license_id)
    except ValueError:
        return (len(LICENSE_PREFERENCE), license_id)


def resolve_licenses(node: tuple) -> list[str]:
    """The licenses to distribute under for a parsed expression.

    `AND` means both texts apply, so its resolution is the union. `OR` means we may choose, and the
    choice this document makes is the least restrictive one -- measured by the *least* preferred
    license an alternative forces, so `MIT AND (Apache-2.0 OR MIT)` resolves to MIT alone rather
    than to MIT plus whatever the first alternative dragged in. Ties go to the alternative that
    names fewer licenses, then to the better-ranked set.
    """
    kind = node[0]
    if kind == "id":
        return [node[1]]
    if kind == "and":
        out: list[str] = []
        for child in node[1]:
            for license_id in resolve_licenses(child):
                if license_id not in out:
                    out.append(license_id)
        return out
    candidates = [resolve_licenses(child) for child in node[1]]
    return min(
        candidates,
        key=lambda choice: (
            max(_rank(license_id) for license_id in choice),
            len(choice),
            tuple(sorted(_rank(license_id) for license_id in choice)),
        ),
    )


def choose_licenses(expression: str) -> list[str]:
    """The license ids to distribute under for a declared expression, best-preferred first."""
    picks: list[str] = []
    for license_id in resolve_licenses(parse_expression(expression)):
        canonical = CANONICAL_LICENSES.get(license_id, license_id)
        if canonical not in picks:
            picks.append(canonical)
    outside = [pick for pick in picks if pick not in ALLOWED_LICENSES]
    if outside:
        raise Unreadable(
            f"license {', '.join(sorted(outside))} declared by {expression!r} is not in "
            "ALLOWED_LICENSES; a permissive-only distribution has to say what changed"
        )
    return sorted(picks, key=_rank)


class Entry:
    """One Part I entry, with its body kept byte-for-byte as the document has it."""

    __slots__ = ("name", "version", "body", "start", "end")

    def __init__(self, name: str, version: str, body: str, start: int, end: int) -> None:
        self.name = name
        self.version = version
        self.body = body
        self.start = start
        self.end = end

    @property
    def key(self) -> tuple[str, str]:
        return (self.name, self.version)

    def declared(self) -> str:
        match = re.search(r"^License:.*?\(upstream declares:\s*(.*?)\)\s*$", self.body, re.M)
        return match.group(1).strip() if match else ""

    def chosen(self) -> str:
        match = re.search(r"^License:\s*(.+?)(?:\s*\(upstream declares:.*\))?\s*$", self.body, re.M)
        return match.group(1).strip() if match else ""

    def references(self) -> list[str]:
        """Every Part II license id this entry points at, in the order written."""
        line = re.findall(r"^License text: see Part II\s*—\s*(.+)$", self.body, re.M)
        return [part.strip() for match in line for part in match.split(";") if part.strip()]

    def upstream_text(self) -> tuple[str, str]:
        """`(url, license id)` when the entry points at upstream for the text instead of Part II."""
        match = re.search(
            r"^License text: see upstream\s+(\S+)\s+\(license:\s*(.+?)\)\s*$", self.body, re.M
        )
        return (match.group(1), match.group(2).strip()) if match else ("", "")

    def recorded_terms(self) -> list[str] | None:
        """The license ids this entry says apply, or None when it does not say so cleanly.

        Two shapes are legitimate: a single id on the `License:` line, or the declared expression on
        that line with the ids spelled out in a following `(applicable terms: ...)` line. Anything
        else -- a compound `License:` line with nothing spelling it out -- is not readable, and a
        reader of a legal document should not have to guess which of three licenses applied.
        """
        match = re.search(r"^\s*\(applicable terms:\s*(.*?)\)\s*$", self.body, re.M)
        if match:
            return [term.strip() for term in match.group(1).split(",") if term.strip()]
        chosen = self.chosen()
        if not chosen:
            return None
        if re.search(r"\bAND\b|\bOR\b|/", chosen, re.I):
            return None
        return [CANONICAL_LICENSES.get(chosen, chosen)]

    def has_copyright(self) -> bool:
        return re.search(r"^Copyright notice:\s*$", self.body, re.M) is not None


class Document:
    """The committed notices file, split the way the file is split."""

    def __init__(self, text: str) -> None:
        self.text = text
        if PART_I not in text or PART_II not in text:
            raise Unreadable(
                f"{DOCUMENT} has neither {PART_I!r} nor {PART_II!r}; the reader refuses to guess "
                "at a format it was not written for"
            )
        self.part1_head, rest = text.split(PART_I, 1)
        part1, self.part2 = rest.split(PART_II, 1)
        # Offsets are into the whole document, so a writer can splice an entry back in.
        self._part1_start = len(self.part1_head) + len(PART_I)
        self.entries = self._read_entries(part1, self._part1_start)
        self.license_sections, self.section_spans = self._read_sections(
            self.part2, len(text) - len(self.part2)
        )

    @staticmethod
    def _read_entries(part1: str, offset: int) -> list[Entry]:
        # An entry is `<rule>\n<name> <version>\n<rule>\n<body>` up to the next rule or banner.
        pattern = re.compile(
            r"^-{60,}\n(\S.*?) (\d[^\s]*)\n-{60,}\n(.*?)(?=^-{60,}\n|^[=-]{60,}|\Z)",
            re.M | re.S,
        )
        entries: list[Entry] = []
        for match in pattern.finditer(part1):
            entries.append(
                Entry(
                    name=match.group(1).strip(),
                    version=match.group(2).strip(),
                    body=match.group(3),
                    start=offset + match.start(),
                    end=offset + match.end(),
                )
            )
        if not entries:
            raise Unreadable(f"{PART_I} of {DOCUMENT} contains no readable package entries")
        return entries

    @staticmethod
    def _read_sections(part2: str, offset: int) -> tuple[dict[str, str], dict[str, tuple[int, int]]]:
        # A section is `#`*80, an `# <id>` heading, `#`*80, then the text.
        pattern = re.compile(r"^#{20,}\n# (.+?)\n#{20,}\n", re.M)
        sections: dict[str, str] = {}
        spans: dict[str, tuple[int, int]] = {}
        matches = list(pattern.finditer(part2))
        for index, match in enumerate(matches):
            stop = matches[index + 1].start() if index + 1 < len(matches) else len(part2)
            if index + 1 == len(matches):
                # The last section runs to the end of the file, where the closing `=` banner is not
                # part of it. Part II holds exactly one `=` rule, so this cannot land inside a text.
                banner = re.compile(r"\n\n={60,}\n").search(part2[match.start() :])
                if banner is not None:
                    stop = match.start() + banner.start() + 2
            section = match.group(1).strip()
            sections[section] = part2[match.end() : stop].strip()
            # The span keeps the blank line that separates this section from the next one, so a
            # writer that deletes the section deletes its separator too and the rest stays spaced
            # exactly as it was.
            spans[section] = (offset + match.start(), offset + stop)
        return sections, spans

    def by_key(self) -> dict[tuple[str, str], Entry]:
        return {entry.key: entry for entry in self.entries}


def unreachable_sections(document: Document) -> list[str]:
    """Part II license texts that no reader can reach from an entry.

    A section is reachable when an entry asks for it by name on its `License text:` line, or when its
    id is written in prose outside the section itself; `libgit2-sys` takes that route for GPL-2.0-only,
    whose upstream `COPYING` is described in the entry instead of pointed at by id. Anything else is a
    text nobody asked for, so both the generator that writes Part II and the gate that checks it use
    this one reading of reachability rather than each keeping their own.
    """
    referenced = {section for entry in document.entries for section in entry.references()}
    dead: list[str] = []
    for section, (start, end) in document.section_spans.items():
        if section in referenced:
            continue
        if section in document.text[:start] + document.text[end:]:
            continue
        dead.append(section)
    return sorted(dead)


def copyright_lines(source_dir: Path) -> list[str]:
    """Copyright lines out of the license and attribution files at a package's root."""
    found: list[str] = []
    for name in sorted(os.listdir(source_dir)):
        path = source_dir / name
        if not path.is_file() or not LICENSE_FILE_RE.match(name):
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        # Split on newlines only: `str.splitlines()` also breaks on the form feeds that some
        # license texts carry, which would cut a line in half here.
        for line in text.split("\n"):
            line = line.strip()
            if not line or len(line) > 200 or SPDX_ONLY_RE.match(line):
                continue
            if _is_copyright_line(line):
                if line not in found:
                    found.append(line)
        if found:
            break
    return found


def registry_source(dep: Dependency, repo: Path) -> Path | None:
    """The unpacked source of a crates.io package, or None when it has not been fetched.

    Vendored packages are read from the tree instead, which is where their license files live.
    """
    manifest = Path(dep.manifest)
    if dep.vendored and manifest.is_file():
        root = manifest.parent
        if root.is_dir():
            return root
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    for source in sorted(cargo_home.glob("registry/src/*")):
        candidate = source / f"{dep.name}-{dep.version}"
        if candidate.is_dir():
            return candidate
    return None


class Triage:
    """The difference between what is shipped and what the document covers, in four categories.

    The categories are separate because the fix is different for each: a missing package needs an
    entry written, a drifted entry needs its version updated, an entry for nothing shipped needs
    deleting (or a recorded reason for keeping it), and an unparsable license needs a human.
    """

    def __init__(self) -> None:
        self.missing: list[Dependency] = []
        self.drifted: list[tuple[Dependency, Entry]] = []
        self.unshipped: list[Entry] = []
        self.refused: list[tuple[str, str]] = []

    def counts(self) -> str:
        return (
            f"{len(self.missing)} package(s) with no entry, "
            f"{len(self.drifted)} entry(ies) whose version moved, "
            f"{len(self.unshipped)} entry(ies) for nothing shipped, "
            f"{len(self.refused)} license(s) this tool cannot judge"
        )


def triage(document: Document, dependencies: list[Dependency]) -> Triage:
    exact = document.by_key()
    shipped_keys = {dep.key for dep in dependencies}
    # An entry can only stand as the older record of a version move if it is not itself exact
    # coverage for some other shipped version. Two versions of one crate in one binary is ordinary
    # here, and matching a shipped version against "the first entry with that name" would report
    # `leaf 0.9.0` as a move of the entry that correctly documents `leaf 1.0.0`.
    free: dict[str, list[Entry]] = {}
    for entry in document.entries:
        if entry.key not in shipped_keys:
            free.setdefault(entry.name, []).append(entry)
    out = Triage()
    for dep in dependencies:
        if dep.key in exact:
            continue
        candidates = free.get(dep.name)
        if candidates:
            # Version moves are normally upgrades, so the entry that most plausibly describes the
            # move is the highest one still older than what ships; failing that, the highest there is.
            wanted = version_key(dep.version)
            ordered = sorted(candidates, key=lambda entry: version_key(entry.version))
            older = [entry for entry in ordered if version_key(entry.version) < wanted]
            out.drifted.append((dep, older[-1] if older else ordered[-1]))
            continue
        out.missing.append(dep)
    for entry in document.entries:
        if entry.key in shipped_keys:
            continue
        out.unshipped.append(entry)
    for dep in dependencies:
        try:
            dep.licenses()
        except Unreadable as exc:
            out.refused.append((f"{dep.name} {dep.version}", str(exc)))
    return out


def summarize(repo: Path) -> str:
    """A one-screen read of both sides, which is also what `--help`-less invocation prints."""
    document = Document((repo / DOCUMENT).read_text(encoding="utf-8"))
    dependencies = shipped_dependencies(repo)
    result = triage(document, dependencies)
    return "\n".join(
        [
            f"shipped third-party packages: {len(dependencies)}",
            f"documented entries: {len(document.entries)}",
            f"license texts in {PART_II}: {len(document.license_sections)}",
            result.counts(),
        ]
    )


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="summarize the notices document against what is shipped")
    parser.add_argument("repo", nargs="?", default=".", type=Path)
    args = parser.parse_args()
    try:
        print(summarize(args.repo.resolve()))
    except Unreadable as exc:
        print(f"notices-lib: {exc}")
        raise SystemExit(2)
