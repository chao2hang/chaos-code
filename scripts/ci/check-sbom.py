#!/usr/bin/env python3
"""Reject an SBOM that says anything untrue about the binary this repository ships.

`scripts/gen-sbom.py` writes the artifact, and a generator cannot be its own witness: the reading of
`cargo metadata` it is built on is the same reading that produced the document, so a wrong reading
would produce a self-consistent wrong SBOM. This guard does not import the generator. It reads the
build again through `scripts/notices_lib.py` -- the module that answers "what is in the binary" for
the notices guards too -- and compares that against the file in front of it.

What has to be true, and what a failure means:

1. The envelope is CycloneDX 1.6 and the `serialNumber` re-derives from the document's own bytes. A
   serial that does not re-derive means either the file was edited after it was written or the
   derivation changed, and either way two builds of the same tree no longer produce the same
   artifact, which is the one property worth more than the rest.
2. `metadata.component` is the product, named and versioned the way `package.json` and the root
   crate name and version it, and the root crate is not also listed among the components.
3. The component inventory is exactly the reachable graph: every package the release build can link
   appears once, nothing else appears at all. Names, versions and the split between our own crates,
   vendored code and registry dependencies are taken from `cargo metadata`, not from the document.
4. The third-party subset is exactly the set `THIRD-PARTY-NOTICES` is generated from
   (`scripts/notices_lib.shipped_dependencies`), so the two artifacts an auditor is handed cannot
   disagree about what "shipped" means. This is the check that matters most and it is two-way: a
   dropped package and an invented one fail alike.
5. A component's license is the license its package declares. An SBOM is read by people deciding
   whether a distribution can ship, and a wrong license in it is worse than a missing one.
6. `scope: optional` marks exactly the packages the build reaches through a build script and nothing
   else; `pkg:cargo/` marks exactly the packages that came from crates.io. Both are claims a
   scanner acts on: the first is what it tells a user is not in the binary, the second is the
   registry it goes looking in.
7. The dependency graph is the build's own non-dev edge set, both directions. Reachability questions
   ("is the crate with the new CVE in the path to the thing we run?") are the reason to include a
   graph at all, so a graph missing an edge is not an incomplete report but a wrong one.
8. A registry component carries exactly the SHA-256 `Cargo.lock` records for it, and nothing else in
   the document carries a hash at all. This is the one claim a reader can verify without trusting
   either script, so it is checked against the lock file rather than against the value's shape: a
   digest that is well-formed but belongs to another package would let a scanner certify the wrong
   bytes. The lock is parsed here independently of how `gen-sbom.py` parses it, and the file itself
   is read as evidence too: a checksum on a package that came from no registry, or one package
   listed twice with two digests, means cargo did not write this lock, so the digests in it are not
   evidence about anything.

Like `check-notices-coverage.py`, this needs `cargo metadata`, which the CI container has no registry
for, so it is listed in `scripts/ci/docker-entry-ci-only.tsv` and runs on the host and in the `rust`
CI job; `--metadata FILE` reads a saved `cargo metadata` instead, which is how
`scripts/ci/test-check-sbom.py` exercises every branch with no toolchain at all. A document that
cannot be read, or a build that cannot be computed, is a failure and never a skip.

Usage: python3 scripts/ci/check-sbom.py --sbom dist/chaos.cdx.json [--root DIR] [--metadata FILE]
                                        [--lock Cargo.lock]
Exit: 0 = the document describes this build, completely and accurately, and its serial reproduces.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import re
import sys
import uuid
from pathlib import Path

GUARD = "check-sbom"
LIB = "notices_lib.py"
SAMPLE = 8
SPEC_VERSION = "1.6"
SERIAL_RE = re.compile(r"^urn:uuid:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
# The same namespace the generator derives from; a document carrying a serial computed against a
# different one is a document whose derivation changed, which check 1 is meant to catch.
SERIAL_NAMESPACE = uuid.uuid5(uuid.NAMESPACE_URL, "https://github.com/chao2hang/chaos-code/sbom")
PRODUCT_MANIFEST = Path("crates/codegen/xai-grok-pager/npm/chaos/package.json")
ORIGINS = ("dependency", "vendored", "workspace")
PURL_RE = re.compile(r"^pkg:(cargo|generic)/([^/@]+)@([^/]+)$")
LOCK_FILE = "Cargo.lock"
LOCK_SECTION = "[[package]]"
HASH_ALG = "SHA-256"
HEX64_RE = re.compile(r"^[0-9a-f]{64}$")
LOCK_LINE_RE = re.compile(r'^(name|version|source|checksum) = "([^"]*)"$')


def lock_digests(path: Path) -> tuple[dict[tuple[str, str, str | None], str], list[str]]:
    """Every digest `Cargo.lock` records, plus anything odd about the file itself.

    Deliberately a different parse from the generator's: this walks the file line by line and closes
    a package record at the next table header, where `gen-sbom.py` splits on the section marker and
    searches within each block. Two shapes, one file: if they ever disagree about which digest
    belongs to which package, the disagreement is reported rather than agreed upon.
    """
    if not path.is_file():
        return {}, [f"{path} is not a file, so the document's `hashes` cannot be checked against "
                    "the lock they are supposed to come from"]
    digests: dict[tuple[str, str, str | None], str] = {}
    problems: list[str] = []
    record: dict[str, str] = {}

    def close(line_number: int) -> None:
        if "name" not in record or "version" not in record:
            return
        if "checksum" not in record:
            return
        digest = record["checksum"]
        if not HEX64_RE.match(digest):
            problems.append(f"{path}:{line_number}: checksum for {record['name']} "
                            f"{record['version']} is {digest!r}, not 64 lowercase hex digits")
            return
        key = (record["name"], record["version"], record.get("source"))
        if not (record.get("source") or "").startswith("registry+"):
            # Cargo only writes `checksum` for bytes it downloaded from an index. A digest on a
            # path or git package means this file was edited, and every digest in it is suspect.
            problems.append(f"{path}:{line_number}: {record['name']} {record['version']} records a "
                            "checksum but does not come from a registry, so cargo did not write "
                            "this lock file")
        if key in digests and digests[key] != digest:
            problems.append(f"{path}:{line_number}: {key[0]} {key[1]} is listed twice with "
                            f"different checksums ({digests[key]} and {digest})")
        digests[key] = digest

    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if line.startswith("["):
            close(number)
            record = {}
            continue
        matched = LOCK_LINE_RE.match(line)
        if matched:
            record[matched.group(1)] = matched.group(2)
    close(number)
    return digests, problems


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
    return ", ".join(items[:limit]) + f" (and {len(items) - limit} more)"


def normalize(expression: str) -> str:
    """An SPDX-ish license string in the form both sides of a comparison should use."""
    return " ".join(re.sub(r"\s*/\s*", " OR ", expression or "").split())


def canonical(document: dict) -> bytes:
    """The bytes a serial number is derived from: the document with `serialNumber` removed.

    Written here rather than imported from the generator on purpose. The check is that the serial in
    the file follows from the bytes of the file, and importing the code that wrote it would make
    that check true by construction.
    """
    body = {key: value for key, value in document.items() if key != "serialNumber"}
    return json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def expected_serial(document: dict) -> str:
    return f"urn:uuid:{uuid.uuid5(SERIAL_NAMESPACE, hashlib.sha256(canonical(document)).hexdigest())}"


def properties(entry: dict) -> dict[str, str]:
    return {item["name"]: item["value"] for item in entry.get("properties", [])
            if isinstance(item, dict) and "name" in item and isinstance(item.get("value"), str)}


def graph(lib: object, metadata: dict) -> tuple[dict, dict[str, set[str]], dict[str, list[str]],
                                                dict[str, str], str]:
    """The build's own answer: packages by id, edge kinds, non-dev edges, origins, and the root id.

    The walk and the origin split are `notices_lib`'s, so this guard and the two notices guards read
    one definition of "shipped" rather than three.
    """
    packages = {package["id"]: package for package in metadata.get("packages", [])}
    if not packages:
        raise SystemExit(f"{GUARD}: cargo metadata reported no packages")
    members = set(metadata.get("workspace_members", []))
    edges = lib._non_dev_edges(metadata)
    roots = sorted(pid for pid, package in packages.items() if package["name"] == lib.SHIPPED_ROOT)
    if not roots:
        raise SystemExit(f"{GUARD}: no package named {lib.SHIPPED_ROOT} in cargo metadata")
    root_id = roots[0]
    seen: set[str] = set()
    stack = [root_id]
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        stack.extend(edges.get(pid, []))

    kinds: dict[str, set[str]] = {}
    children = {pid: sorted(set(edges.get(pid, []))) for pid in seen}
    for node in metadata.get("resolve", {}).get("nodes", []):
        if node["id"] not in seen:
            continue
        for edge in node.get("deps", []):
            child = edge.get("pkg")
            if child is None:
                continue
            entries = edge.get("dep_kinds") or []
            if lib._dev_only(entries):
                continue
            got = {(kind.get("kind") or "normal") for kind in entries} - {"dev"}
            kinds.setdefault(child, set()).update(got or {"normal"})

    origin: dict[str, str] = {}
    for pid in seen:
        manifest = packages[pid].get("manifest_path", "").replace(os.sep, "/")
        if lib.VENDORED_PREFIX in manifest:
            origin[pid] = "vendored"
        else:
            origin[pid] = "workspace" if pid in members else "dependency"
    return packages, kinds, children, origin, root_id


def check(document: object, lib: object, metadata: dict, repo: Path, limit: int,
          lock: Path | None = None) -> tuple[list[str], str]:
    """Every assertion, collected rather than short-circuited, so one run reports all of it."""
    problems: list[str] = []
    sample = max(1, limit)

    # ---- 1. envelope ---------------------------------------------------------------------------
    for key, want in (("bomFormat", "CycloneDX"), ("specVersion", SPEC_VERSION), ("version", 1)):
        if document.get(key) != want:
            problems.append(f"{key} is {document.get(key)!r}, expected {want!r}")
    schema = document.get("$schema")
    if schema != f"http://cyclonedx.org/schema/bom-{SPEC_VERSION}.schema.json":
        problems.append(f"$schema is {schema!r}, expected the {SPEC_VERSION} schema URL")
    serial = document.get("serialNumber")
    if not isinstance(serial, str) or not SERIAL_RE.match(serial):
        problems.append(f"serialNumber {serial!r} is not a `urn:uuid:` UUID")
    elif serial != expected_serial(document):
        problems.append(
            f"serialNumber {serial} does not re-derive from this document's own bytes "
            f"(expected {expected_serial(document)}); the file was edited after it was written, "
            f"or the derivation changed, and either way two builds of one tree stop agreeing"
        )

    # ---- 2. the product node -------------------------------------------------------------------
    packages, kinds, children, origin, root_id = graph(lib, metadata)
    root = packages[root_id]
    root_name, root_version = root["name"], root["version"]
    build_ids: dict[tuple[str, str], list[str]] = {}
    for pid in origin:
        build_ids.setdefault((packages[pid]["name"], packages[pid]["version"]), []).append(pid)
    product = (document.get("metadata") or {}).get("component") or {}
    manifest = repo / PRODUCT_MANIFEST
    product_name, product_version = product.get("name"), product.get("version")
    if manifest.is_file():
        data = json.loads(manifest.read_text(encoding="utf-8"))
        if data.get("name") and data["name"] != product_name:
            problems.append(f"metadata.component.name is {product_name!r}, but the product this "
                            f"repository ships is named {data['name']!r} in {PRODUCT_MANIFEST}")
    if product.get("type") != "application":
        problems.append(f"metadata.component.type is {product.get('type')!r}; the thing being "
                        f"described is an application, not a library")
    if product_version != root_version:
        problems.append(f"metadata.component.version is {product_version!r}, but "
                        f"{root_name} {root_version} is the crate that builds the binary")
    product_ref = product.get("bom-ref")
    if product_ref != product.get("purl") or not str(product_ref or "").startswith("pkg:generic/"):
        problems.append(f"metadata.component bom-ref/purl should both be the product's own "
                        f"`pkg:generic/` purl, got {product_ref!r} / {product.get('purl')!r}")
    if properties(product).get("chaos:cargo:root-crate") != f"{root_name} {root_version}":
        problems.append("metadata.component does not carry a chaos:cargo:root-crate property "
                        f"naming {root_name} {root_version}, so the crate behind the product is "
                        "not recorded")

    # ---- 3. the component inventory against the build ------------------------------------------
    components = document.get("components")
    if not isinstance(components, list) or not components:
        problems.append("components is missing or empty, so the document inventories nothing")
        return problems, ""

    refs: dict[str, str] = {}          # bom-ref -> package id
    seen_ids: set[str] = set()
    ambiguous: list[str] = []
    bad_ref: list[str] = []
    mismatched_ref: list[str] = []
    wrong_namespace: list[str] = []
    wrong_origin: list[str] = []
    wrong_license: list[str] = []
    wrong_scope: list[str] = []
    wrong_hash: list[str] = []
    digests, lock_problems = lock_digests(lock if lock is not None else repo / LOCK_FILE)
    problems.extend(lock_problems)
    hashed = 0
    declared_refs: dict[tuple[str, str], str] = {}
    claimed_origin: dict[str, str] = {}
    for entry in components:
        reference = entry.get("bom-ref")
        name, version = entry.get("name"), entry.get("version")
        if not name or not version:
            bad_ref.append(f"{reference!r} is missing a name or a version")
            continue
        if reference != entry.get("purl"):
            mismatched_ref.append(f"{name} {version}: bom-ref {reference!r} is not its purl "
                                  f"{entry.get('purl')!r}")
        if (name, version) in declared_refs:
            bad_ref.append(f"{name} {version} is listed twice ({declared_refs[(name, version)]} "
                           f"and {reference})")
            continue
        declared_refs[(name, version)] = str(reference)
        candidates = build_ids.get((name, version), [])
        if not candidates:
            bad_ref.append(f"{name} {version} is in the SBOM but the build does not reach it")
            continue
        if len(candidates) > 1:
            ambiguous.append(f"{name} {version} is reached from "
                             f"{len(candidates)} sources, which one purl cannot tell apart")
            seen_ids.update(candidates)
            continue
        pid = candidates[0]
        if pid == root_id:
            bad_ref.append(f"{name} {version} is the crate that builds the binary and should be "
                           f"metadata.component, not a component")
            continue
        seen_ids.add(pid)
        refs[reference] = pid
        entry_flags = properties(entry)
        claimed_origin[str(reference)] = entry_flags.get("chaos:origin") or ""
        # Both strings are checked, because consumers use one or the other: `bom-ref` to walk the
        # graph and `purl` to look the package up somewhere else.
        from_registry = (packages[pid].get("source") or "").startswith("registry+")
        for label, value in (("bom-ref", reference), ("purl", entry.get("purl"))):
            parsed = PURL_RE.match(str(value or ""))
            if not parsed:
                bad_ref.append(f"{name} {version}: its {label} {value!r} is not a `pkg:cargo/` or "
                               f"`pkg:generic/` purl")
                continue
            namespace, purl_name, purl_version = parsed.groups()
            if (purl_name, purl_version) != (name.replace("+", "%2B"), version.replace("+", "%2B")):
                bad_ref.append(f"{name} {version}: its {label} names {purl_name}@{purl_version}")
            if (namespace == "cargo") != from_registry:
                wrong_namespace.append(
                    f"{name} {version} uses {namespace!r} in its {label} but came from "
                    f"{packages[pid].get('source') or 'a local path'!r}"
                )
        if entry_flags.get("chaos:origin") != origin[pid]:
            wrong_origin.append(f"{name} {version}: says {entry_flags.get('chaos:origin')!r}, the "
                                f"build says {origin[pid]!r}")
        declared = normalize(packages[pid].get("license") or "")
        expressions = [item.get("expression") for item in entry.get("licenses", [])
                       if isinstance(item, dict) and item.get("expression")]
        if len(entry.get("licenses", [])) != 1 or len(expressions) != 1:
            wrong_license.append(f"{name} {version}: licenses should be one expression, got "
                                 f"{json.dumps(entry.get('licenses', []), ensure_ascii=False)[:80]}")
        elif normalize(expressions[0]) != declared:
            wrong_license.append(f"{name} {version}: SBOM says {expressions[0]!r}, the package "
                                 f"declares {packages[pid].get('license')!r}")
        build_only = kinds.get(pid, set()) == {"build"}
        if (entry.get("scope") == "optional") != build_only:
            wrong_scope.append(
                f"{name} {version}: scope {entry.get('scope')!r} for edges "
                f"{sorted(kinds.get(pid, set())) or ['<none>']!r}"
            )
        # The digest is the one claim in this document that a reader can check without trusting us,
        # so it is checked against the lock rather than against its own shape.
        recorded = entry.get("hashes")
        expected = digests.get((name, version, packages[pid].get("source")))
        if recorded:
            hashed += 1
        if not from_registry:
            if recorded:
                wrong_hash.append(f"{name} {version}: carries {json.dumps(recorded, ensure_ascii=False)[:60]} "
                                  f"but came from {packages[pid].get('source') or 'a local path'!r}, "
                                  "for which cargo records no digest")
        elif not isinstance(recorded, list) or len(recorded) != 1:
            wrong_hash.append(f"{name} {version}: hashes should be the one SHA-256 in "
                              f"{LOCK_FILE}, got {json.dumps(recorded, ensure_ascii=False)[:60]}")
        else:
            item = recorded[0]
            alg = item.get("alg") if isinstance(item, dict) else None
            content = item.get("content") if isinstance(item, dict) else None
            if not isinstance(item, dict) or alg != HASH_ALG:
                wrong_hash.append(f"{name} {version}: hash alg is {alg!r}, expected {HASH_ALG!r}")
            elif not isinstance(content, str) or not HEX64_RE.match(content):
                wrong_hash.append(f"{name} {version}: hash {content!r} is not 64 lowercase hex digits")
            elif expected is None:
                wrong_hash.append(f"{name} {version}: carries {content} but {LOCK_FILE} records no "
                                  "checksum for it, so the two are not the same build")
            elif content != expected:
                wrong_hash.append(f"{name} {version}: hash is {content}, {LOCK_FILE} says {expected}")

    absent = sorted(
        f"{packages[pid]['name']} {packages[pid]['version']}"
        for pid in set(origin) - seen_ids - {root_id}
    )
    if absent:
        problems.append(f"{len(absent)} package(s) the build reaches are not in the SBOM. "
                        f"First: {listed(absent, sample)}")
    for label, findings in (("purl does not identify one package the build reaches", bad_ref),
                            ("build reaches it from two sources and one purl cannot say which",
                             ambiguous),
                            ("bom-ref and purl disagree", mismatched_ref),
                            ("purl namespace does not match where the package came from",
                             wrong_namespace),
                            ("chaos:origin does not match the build", wrong_origin),
                            ("license does not match the package", wrong_license),
                            ("scope does not match the dependency edge kinds", wrong_scope),
                            ("SHA-256 is not the digest Cargo.lock records", wrong_hash)):
        if findings:
            problems.append(f"{len(findings)} component(s) whose {label}. First: "
                            f"{listed(findings, sample)}")

    # ---- 4. the third-party subset against the notices document --------------------------------
    # Third-party here means "the document says somebody else wrote it", which is what the notices
    # document is a set of, and what a license auditor reads. Deriving it from the build instead
    # would make this check a restatement of the coverage check above and unable to notice a document
    # that calls a dependency its own.
    shipped = {(dependency.name, dependency.version)
               for dependency in lib.shipped_dependencies(repo, metadata)}
    third_party = {(packages[pid]["name"], packages[pid]["version"])
                   for reference, pid in refs.items() if claimed_origin.get(reference) != "workspace"}
    notices_only = sorted(f"{name} {version}" for name, version in shipped - third_party)
    sbom_only = sorted(f"{name} {version}" for name, version in third_party - shipped)
    if notices_only or sbom_only:
        problems.append(
            f"the SBOM and THIRD-PARTY-NOTICES describe different third-party sets: "
            f"{len(notices_only)} covered by a notice but absent from the SBOM "
            f"[{listed(notices_only, sample)}], {len(sbom_only)} in the SBOM with no notice "
            f"[{listed(sbom_only, sample)}]"
        )

    # ---- 5. the dependency graph against the build's edges -------------------------------------
    dependencies = document.get("dependencies")
    if not isinstance(dependencies, list):
        problems.append("dependencies is missing, so the document carries no graph")
        return problems, ""
    product_reference = str(product_ref)
    known = set(refs) | {product_reference}
    objects: dict[str, list[str]] = {}
    duplicates: list[str] = []
    dangling: list[str] = []
    for entry in dependencies:
        reference = entry.get("ref")
        if reference not in known:
            dangling.append(f"a dependency object names {reference!r}, which is not a component")
            continue
        if reference in objects:
            duplicates.append(str(reference))
            continue
        children_refs = entry.get("dependsOn") or []
        for child in children_refs:
            if child not in known:
                dangling.append(f"{reference} depends on {child!r}, which is not a component")
        objects.setdefault(reference, list(children_refs))
    for reference in sorted(known - set(objects)):
        dangling.append(f"no dependency object for {reference}, so a reader cannot tell "
                        f"'has no dependencies' from 'not described'")
    if duplicates:
        problems.append(f"{len(duplicates)} component(s) with more than one dependency object. "
                        f"First: {listed(duplicates, sample)}")
    if dangling:
        problems.append(f"{len(dangling)} dangling or missing reference(s) in the graph. "
                        f"First: {listed(dangling, sample)}")

    actual_edges = {(reference, child) for reference, children_refs in objects.items()
                    for child in children_refs}
    # Package ids become refs the same way the document does it, with the root crate standing under
    # the product's reference; `None` marks an endpoint the document never named, which cannot match
    # any edge and therefore always shows up as a missing one.
    ref_of = {pid: reference for reference, pid in refs.items()}

    def edge_endpoint(pid: str) -> str | None:
        if pid == root_id:
            return product_reference
        return ref_of.get(pid)

    expected_edges = {(edge_endpoint(parent), edge_endpoint(child))
                      for parent, children_list in children.items() for child in children_list}
    expected_edges = {edge for edge in expected_edges if None not in edge}
    missing_edges = sorted(edge for edge in expected_edges if edge not in actual_edges)
    extra_edges = sorted(edge for edge in actual_edges if edge not in expected_edges)
    if missing_edges or extra_edges:
        def show(edge: tuple[str, str]) -> str:
            return f"{edge[0]} -> {edge[1]}"
        problems.append(
            f"the dependency graph is not the build's: {len(missing_edges)} edge(s) the build has "
            f"are absent [{listed([show(e) for e in missing_edges], sample)}], "
            f"{len(extra_edges)} edge(s) the build does not have are present "
            f"[{listed([show(e) for e in extra_edges], sample)}]"
        )

    counts = {name: 0 for name in ORIGINS}
    for pid in refs.values():
        counts[origin[pid]] += 1
    summary = (f"{len(refs)} component(s): third-party {counts['dependency'] + counts['vendored']} "
               f"(matching THIRD-PARTY-NOTICES), workspace {counts['workspace']}; "
               f"{len(actual_edges)} dependency edges; SHA-256 on {hashed}; serial {serial}")
    return problems, summary


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--sbom", required=True, type=Path,
                        help="the SBOM file to check (or `-` for stdin)")
    parser.add_argument("--root", default=".", type=Path)
    parser.add_argument("--metadata", default=None, type=Path,
                        help="a saved `cargo metadata --format-version 1` JSON, in place of cargo")
    parser.add_argument("--max-list", default=SAMPLE, type=int,
                        help="how many examples of each finding to print")
    parser.add_argument("--lock", default=None, type=Path,
                        help=f"cargo lock the `hashes` come from (default <root>/{LOCK_FILE})")
    ns = parser.parse_args(argv)

    lib = load_lib()
    repo = ns.root.resolve()
    if str(ns.sbom) == "-":
        text = sys.stdin.read()
        source = "stdin"
    elif ns.sbom.is_file():
        text = ns.sbom.read_text(encoding="utf-8")
        source = str(ns.sbom)
    else:
        print(f"{GUARD}: {ns.sbom} is not a file; generate it with scripts/gen-sbom.py --output",
              file=sys.stderr)
        return 2
    try:
        document = json.loads(text)
    except json.JSONDecodeError as exc:
        print(f"{GUARD}: FAIL (1 problem(s))")
        print(f"  {source} is not JSON: {exc}")
        return 1
    if not isinstance(document, dict):
        print(f"{GUARD}: FAIL (1 problem(s))")
        print(f"  {source} is a JSON {type(document).__name__}, expected an object")
        return 1
    try:
        if ns.metadata:
            metadata = json.loads(ns.metadata.read_text(encoding="utf-8"))
        else:
            metadata = lib.load_metadata(repo)
    except (lib.Unreadable, OSError, json.JSONDecodeError) as exc:
        print(f"{GUARD}: FAIL (1 problem(s))")
        print(f"  the shipped dependency set could not be computed: {exc}")
        return 1

    try:
        problems, summary = check(document, lib, metadata, repo, ns.max_list, ns.lock)
    except SystemExit as exc:
        print(f"{GUARD}: FAIL (1 problem(s))")
        print(f"  {exc}")
        return 1
    if problems:
        print(f"{GUARD}: FAIL ({len(problems)} problem(s))")
        for problem in problems:
            print(f"  {problem}")
        return 1
    print(f"{GUARD}: OK ({summary})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
