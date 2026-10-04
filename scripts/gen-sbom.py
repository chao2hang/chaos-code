#!/usr/bin/env python3
"""Write a CycloneDX 1.6 SBOM for the binary this repository ships.

`THIRD-PARTY-NOTICES` answers what this distribution owes its users in license texts. An SBOM
answers a different question -- what is actually in the binary -- and for years nothing here could
answer it at all: `TODO.md` listed "generate a matching SBOM" as open work, and `check-evidence-
paths`-clean prose about it pointed at no tool. This script is that tool. It reads the same
`cargo metadata` graph the notices guards read, so the two artifacts cannot disagree about what
"shipped" means, and it needs no network: the SBOM is derived from `Cargo.lock` and the manifests.

What goes in, and why:

- one component per package in `scripts/notices_lib.shipped_dependencies()`: the third-party
  packages reached without crossing a test-only edge, 1139 of them today (1134 from crates.io, 5
  vendored under `third_party/` and marked as such). That is the set the notices document covers,
  and the guard `scripts/ci/check-sbom.py` asserts the two sets are equal, so an SBOM that quietly
  drops a package fails the same commit.
- one component per workspace member reached by the same walk (88 today), because "what is in the
  binary" includes our own crates, and a scanner asked to check reachability cannot answer it
  without them. They carry `chaos:origin = workspace`, so nothing confuses them with dependencies.
  The crate that builds the binary is `metadata.component`, not a component, so it is not counted.
- `scope: optional` plus `chaos:cargo:edge-kinds = build` for the 16 packages reached only through
  a build script (15 third-party, plus our own `xai-proto-build`). They are in the build and not
  linked into the executable; a flat list of 1139 "dependencies" would say something untrue about
  15 of them.
- the dependency graph itself (`dependencies`), from the same non-dev edges. A flat inventory tells
  you a vulnerable crate is present; the graph tells you whether the binary can reach it.
- `hashes` with `alg = SHA-256` on every registry component, read from `Cargo.lock`, which is where
  cargo records the digest of the `.crate` it resolved (1130 of the 1227 components today, because
  `cargo metadata` itself publishes no checksum). A git or path package gets no `hashes`: the lock
  records none for those, and the bytes they came from are a revision or a directory, not an
  artifact with one canonical digest.

Output is byte-deterministic: components are sorted by (origin, name, semantic version), every list
is sorted, the `serialNumber` is a UUIDv5 re-derived from the document's own canonical bytes, and
`metadata.timestamp` is absent unless you ask for it (`--timestamp`, or `SOURCE_DATE_EPOCH`, which
a release build can set). That is what makes `--check` meaningful: two runs agree, so a difference
means the build changed and nothing else.

Usage:
  python3 scripts/gen-sbom.py                              # SBOM on stdout
  python3 scripts/gen-sbom.py --output dist/chaos.cdx.json  # write it to a file
  python3 scripts/gen-sbom.py --metadata saved.json        # read a saved `cargo metadata`
  python3 scripts/gen-sbom.py --lock other/Cargo.lock      # read the digests from elsewhere
  python3 scripts/gen-sbom.py --check dist/chaos.cdx.json  # fail unless the file is current
  python3 scripts/gen-sbom.py --timestamp 2026-10-05T00:00:00Z
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

MODULE = Path(__file__).resolve().parent / "notices_lib.py"
SPEC_VERSION = "1.6"
BOM_FORMAT = "CycloneDX"
SERIAL_NAMESPACE = uuid.uuid5(uuid.NAMESPACE_URL, "https://github.com/chao2hang/chaos-code/sbom")
PRODUCT_PURL_TYPE = "generic"
# The npm manifest is where this repository records the product's own name and public URL; the
# crate that builds the binary carries neither (its `Cargo.toml` has no `repository` or `homepage`).
PRODUCT_MANIFEST = Path("crates/codegen/xai-grok-pager/npm/chaos/package.json")
TIMESTAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
LOCK_FILE = Path("Cargo.lock")
LOCK_MARKER = "[[package]]"
HASH_ALG = "SHA-256"
# `cargo metadata` carries no checksum, so the digests come from the lock file, which is the only
# place cargo records the digest of the artifact it actually resolved. Python 3.10 has no `tomllib`
# and adding a TOML dependency for four keys is not worth a third-party obligation, so this parses
# the shape cargo writes: `[[package]]` blocks with `name`, `version`, optional `source`, optional
# `checksum`. `scripts/ci/check-sbom.py` parses the same file a second time, differently, so a
# mis-parse on one side is a finding rather than an agreement.
LOCK_DIGEST_RE = re.compile(r'^checksum = "([0-9a-f]{64})"$', re.MULTILINE)
LOCK_FIELD_RE = {
    field: re.compile(r'^%s = "([^"]*)"$' % field, re.MULTILINE) for field in ("name", "version", "source")
}


def lock_digests(path: Path) -> dict[tuple[str, str, str | None], str]:
    """Every `Cargo.lock` package that carries a checksum, keyed by (name, version, source).

    The source is part of the key because the lock can hold one name and version twice, once from
    crates.io and once from a git fork, and only the registry one has a digest.
    """
    if not path.is_file():
        raise SystemExit(f"gen-sbom: {path} is not a file; the SBOM's `hashes` come from the lock "
                         "file, so point --lock at the one that produced this metadata")
    text = path.read_text(encoding="utf-8")
    blocks = text.split(LOCK_MARKER)
    if not blocks[1:]:
        raise SystemExit(f"gen-sbom: {path} has no `{LOCK_MARKER}` entries; that is not a "
                         "cargo lock file")
    digests: dict[tuple[str, str, str | None], str] = {}
    for number, block in enumerate(blocks[1:], 1):
        values = {}
        for field, pattern in LOCK_FIELD_RE.items():
            found = pattern.findall(block)
            if len(found) > 1:
                raise SystemExit(f"gen-sbom: {path} entry #{number} repeats `{field}`, which cargo "
                                 "does not write; refusing to guess which one the digest is for")
            values[field] = found[0] if found else None
        if values["name"] is None or values["version"] is None:
            raise SystemExit(f"gen-sbom: {path} entry #{number} has no name or version; the parser "
                             "expects the shape cargo writes and would silently drop a package")
        found = LOCK_DIGEST_RE.findall(block)
        if len(found) > 1:
            raise SystemExit(f"gen-sbom: {path} entry #{number} has {len(found)} checksum lines")
        if not found:
            continue
        key = (values["name"], values["version"], values["source"])
        if digests.get(key, found[0]) != found[0]:
            raise SystemExit(f"gen-sbom: {path} lists {key[0]} {key[1]} twice with different "
                             "checksums; one of them is stale")
        digests[key] = found[0]
    return digests


def load_lib():
    """Import `scripts/notices_lib.py` by path, the way the other notices tools do."""
    spec = importlib.util.spec_from_file_location("notices_lib", MODULE)
    if spec is None or spec.loader is None:
        raise SystemExit(f"gen-sbom: cannot load {MODULE}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def source_kind(package: dict) -> str:
    """Where a package came from: `registry`, `git`, or `path`."""
    source = package.get("source") or ""
    if source.startswith("registry+"):
        return "registry"
    if source.startswith("git+"):
        return "git"
    return "path"


def purl(name: str, version: str, registry: bool = True) -> str:
    """Package URL for one package.

    Registry packages get `pkg:cargo/`, whose default repository is crates.io. Everything else
    gets `pkg:generic/`, deliberately: `nucleo-mapper` is not the crates.io crate of that name, it
    is a git fork at one pinned revision, and a `pkg:cargo/` purl invites a scanner to resolve it
    against crates.io and score a different artifact. The cargo source string is kept in full as a
    `distribution` external reference either way.
    """
    namespace = "cargo" if registry else "generic"
    return f"pkg:{namespace}/{name.replace('+', '%2B')}@{version.replace('+', '%2B')}"


def ref_for(package: dict) -> str:
    return purl(package["name"], package["version"], source_kind(package) == "registry")


def product_purl(name: str, version: str) -> str:
    return f"pkg:{PRODUCT_PURL_TYPE}/{name}@{version}"


def edge_kinds(lib, metadata: dict, reached: set[str]) -> dict[str, set[str]]:
    """Every non-dev kind that reached a package, over the shipped part of the graph only.

    The kinds live in `resolve.nodes[].deps[].dep_kinds[].kind` and nowhere else; cargo writes no
    boolean `dev` flag, which is the mistake this repository already made once (see
    `scripts/notices_lib.shipped_dependencies`).

    Restricted to edges leaving a shipped package on purpose. A crate can also be a test-only build
    dependency of some other crate's tests, and reading the whole graph would record that edge's
    `build` kind here too, turning `normal` into `normal|build` for packages the release build links
    normally. `dev` is dropped rather than skipped, because the walk already ignored those edges.
    """
    kinds: dict[str, set[str]] = {}
    for node in metadata.get("resolve", {}).get("nodes", []):
        if node["id"] not in reached:
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
    return kinds


def shipped_graph(lib, metadata: dict) -> tuple[list[str], dict, dict[str, list[str]],
                                                dict[str, str], str]:
    """The reachable graph: ordered ids, packages, non-dev edges, origins, and the root id.

    Ids are ordered by (origin, name, version) so the document is byte-stable. The origin split is
    the same one `shipped_dependencies` applies: a workspace member is ours unless it is vendored
    under `third_party/`, in which case it is somebody else's code we modified.
    """
    packages = {package["id"]: package for package in metadata.get("packages", [])}
    if not packages:
        raise SystemExit("gen-sbom: cargo metadata reported no packages")
    members = set(metadata.get("workspace_members", []))
    edges = lib._non_dev_edges(metadata)
    roots = sorted(pid for pid, package in packages.items() if package["name"] == lib.SHIPPED_ROOT)
    if not roots:
        raise SystemExit(f"gen-sbom: no package named {lib.SHIPPED_ROOT} in cargo metadata")
    seen: set[str] = set()
    stack = list(roots)
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        stack.extend(edges.get(pid, []))

    origin: dict[str, str] = {}
    for pid in seen:
        vendored = lib.VENDORED_PREFIX in packages[pid].get("manifest_path", "").replace(os.sep, "/")
        if vendored:
            origin[pid] = "vendored"
        else:
            origin[pid] = "workspace" if pid in members else "dependency"

    ordered = sorted(
        seen,
        key=lambda pid: (
            origin[pid] != "dependency",          # third-party first, our own crates after
            origin[pid],
            packages[pid]["name"],
            lib.version_key(packages[pid]["version"]),
            packages[pid]["version"],
        ),
    )
    children = {pid: sorted(set(edges.get(pid, []))) for pid in seen}
    return ordered, packages, children, origin, roots[0]


def license_field(lib, expression: str) -> tuple[list[dict], list[dict]]:
    """The `licenses` entry for a declared expression, plus properties recording the raw form.

    crates.io still accepts the pre-SPDX `MIT/Apache-2.0` shorthand; the expression is written in
    SPDX form (` OR `) because that is what a scanner parses, and the string cargo reported is kept
    alongside it whenever the two differ.
    """
    declared = (expression or "").strip()
    if not declared:
        return [], [{"name": "chaos:cargo:license", "value": "none declared"}]
    normalized = lib.SLASH_FORM_RE.sub(" OR ", declared).strip()
    properties = []
    if normalized != declared:
        properties.append({"name": "chaos:cargo:license-declared", "value": declared})
    return [{"expression": normalized}], properties


def component(lib, package: dict, origin: str, kinds: set[str], digest: str | None = None) -> dict:
    """One CycloneDX component for one package in the shipped graph."""
    name, version = package["name"], package["version"]
    licenses, extra_properties = license_field(lib, package.get("license") or "")
    reference = ref_for(package)
    entry: dict = {
        "type": "library",
        "bom-ref": reference,
        "name": name,
        "version": version,
        "purl": reference,
        # `optional` is the only value the spec offers for "present, not required for the artifact
        # to run"; a build-script dependency is exactly that, and the property says so in words.
        "scope": "optional" if kinds == {"build"} else "required",
    }
    if digest:
        entry["hashes"] = [{"alg": HASH_ALG, "content": digest}]
    if licenses:
        entry["licenses"] = licenses
    description = (package.get("description") or "").strip()
    if description:
        entry["description"] = " ".join(description.split())
    authors = package.get("authors") or []
    if authors:
        entry["publisher"] = "; ".join(authors)
    external: list[dict] = []
    for key, kind in (("repository", "vcs"), ("homepage", "website")):
        url = (package.get(key) or "").strip()
        if url:
            external.append({"type": kind, "url": url})
    registry = package.get("source") or ""
    if registry:
        external.append({"type": "distribution", "url": registry,
                         "comment": "cargo source the package was resolved from"})
    if external:
        entry["externalReferences"] = external
    properties = [
        {"name": "chaos:origin", "value": origin},
        {"name": "chaos:cargo:edge-kinds", "value": "|".join(sorted(kinds)) if kinds else "unknown"},
        {"name": "chaos:cargo:source-kind", "value": source_kind(package)},
    ] + extra_properties
    if origin == "vendored":
        properties.append({"name": "chaos:vendored-with-local-modifications", "value": "true"})
    entry["properties"] = properties
    return entry


def product_identity(repo: Path, packages: dict, root_id: str) -> dict:
    """The product node: name, version and URLs, all read from files this repository ships."""
    crate = packages[root_id]
    version = crate["version"]
    name, homepage, repository = "chaos", "", ""
    manifest = repo / PRODUCT_MANIFEST
    if manifest.is_file():
        data = json.loads(manifest.read_text(encoding="utf-8"))
        name = data.get("name") or name
        homepage = data.get("homepage") or ""
        repo_field = data.get("repository")
        repository = repo_field.get("url") if isinstance(repo_field, dict) else str(repo_field or "")
    return {"name": name, "version": version, "homepage": homepage, "repository": repository,
            "crate": crate["name"], "license": crate.get("license") or ""}


def build(lib, repo: Path, metadata: dict, timestamp: str | None = None,
          lock: Path | None = None) -> dict:
    """The whole SBOM as a plain dict, in a deterministic key order."""
    ordered, packages, children, origin, root_id = shipped_graph(lib, metadata)
    kinds = edge_kinds(lib, metadata, set(ordered))
    product = product_identity(repo, packages, root_id)
    digests = lock_digests(lock if lock is not None else repo / LOCK_FILE)

    # The root crate is described by `metadata.component`, so it is not repeated in `components`;
    # CycloneDX reads that as "this document describes that artifact", and listing it twice would
    # make a scanner count the product among its own dependencies.
    inventory = [pid for pid in ordered if pid != root_id]
    refs = {pid: ref_for(packages[pid]) for pid in ordered}
    clashed = {}
    for pid, reference in refs.items():
        clashed.setdefault(reference, []).append(pid)
    duplicated = {k: v for k, v in clashed.items() if len(v) > 1}
    if duplicated:
        example = next(iter(duplicated.items()))
        raise SystemExit(f"gen-sbom: {len(duplicated)} purl(s) name more than one package, first "
                         f"{example[0]} from {example[1]}; the purl is not identifying packages "
                         "uniquely and must be made to")
    known = {refs[pid] for pid in inventory}
    root_ref = product_purl(product["name"], product["version"])
    components, without_digest, stray_digest = [], [], []
    for pid in inventory:
        package = packages[pid]
        key = (package["name"], package["version"], package.get("source"))
        digest = digests.get(key)
        if source_kind(package) == "registry":
            # A registry package with no lock digest means the lock and the metadata describe
            # different builds; publishing a component a reader cannot verify is worse than failing.
            if digest is None:
                without_digest.append(f"{key[0]} {key[1]}")
        else:
            # Cargo writes no checksum for a git or path package. If one appears, the claim in this
            # file's header that only registry components carry `hashes` has become false.
            if digest is not None:
                stray_digest.append(f"{key[0]} {key[1]} from {key[2]}")
            digest = None
        components.append(component(lib, package, origin[pid], kinds.get(pid, set()), digest))
    if without_digest:
        raise SystemExit(f"gen-sbom: {len(without_digest)} registry package(s) have no `checksum` in "
                         f"{lock if lock is not None else repo / LOCK_FILE}, first "
                         f"{without_digest[:3]}; the lock and the metadata are not the same build")
    if stray_digest:
        raise SystemExit(f"gen-sbom: {len(stray_digest)} non-registry package(s) carry a lock "
                         f"checksum, first {stray_digest[:3]}; this script's rule that only "
                         "crates.io packages get `hashes` needs revisiting against that data")

    metadata_block: dict = {}
    if timestamp:
        metadata_block["timestamp"] = timestamp
    metadata_block["tools"] = {"components": [{
        "type": "application",
        "bom-ref": "chaos:tool:gen-sbom",
        "name": "gen-sbom.py",
        "version": "1",
        "description": "scripts/gen-sbom.py in the chaos repository",
    }]}
    metadata_block["component"] = {
        "type": "application",
        "bom-ref": root_ref,
        "name": product["name"],
        "version": product["version"],
        "purl": root_ref,
        "description": "Chaos coding agent CLI, the binary built from "
                       f"{product['crate']}",
        "properties": [
            {"name": "chaos:cargo:root-crate", "value": f"{product['crate']} {product['version']}"},
            {"name": "chaos:notices", "value": "THIRD-PARTY-NOTICES"},
        ],
    }
    external: list[dict] = []
    if product["license"]:
        metadata_block["component"]["licenses"] = [{"expression": product["license"]}]
    if product["repository"]:
        external.append({"type": "vcs", "url": product["repository"]})
    if product["homepage"]:
        external.append({"type": "website", "url": product["homepage"]})
    if external:
        metadata_block["component"]["externalReferences"] = external

    def depends(pid: str) -> list[str]:
        # Every child of a reachable package is itself reachable, so `refs` always knows it; the
        # root crate is the one package named by a ref other than its purl.
        return sorted({root_ref if child == root_id else refs[child] for child in children[pid]})

    dependencies = [{"ref": root_ref, "dependsOn": depends(root_id)}]
    for pid in inventory:
        dependencies.append({"ref": refs[pid], "dependsOn": depends(pid)})
    missing = sorted({ref for entry in dependencies for ref in entry["dependsOn"]} - known
                     - {root_ref})
    if missing:
        raise SystemExit(f"gen-sbom: {len(missing)} dependency refs name no component, "
                         f"first {missing[:3]}")

    document = {
        "$schema": "http://cyclonedx.org/schema/bom-1.6.schema.json",
        "bomFormat": BOM_FORMAT,
        "specVersion": SPEC_VERSION,
        "version": 1,
        "metadata": metadata_block,
        "components": components,
        "dependencies": dependencies,
    }
    document["serialNumber"] = serial_number(canonical(document))
    return document


def canonical(document: dict) -> bytes:
    """Bytes the serial number is derived from: the document without the serial number."""
    body = {key: value for key, value in document.items() if key != "serialNumber"}
    return json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def serial_number(payload: bytes) -> str:
    return f"urn:uuid:{uuid.uuid5(SERIAL_NAMESPACE, hashlib.sha256(payload).hexdigest())}"


def dumps(document: dict) -> str:
    return json.dumps(document, indent=2, ensure_ascii=False, sort_keys=False) + "\n"


def resolve_timestamp(args: argparse.Namespace) -> str | None:
    if args.timestamp:
        if not TIMESTAMP_RE.match(args.timestamp):
            raise SystemExit(f"gen-sbom: --timestamp {args.timestamp!r} is not YYYY-MM-DDTHH:MM:SSZ")
        return args.timestamp
    epoch = os.environ.get("SOURCE_DATE_EPOCH")
    if epoch:
        import datetime

        try:
            seconds = int(epoch)
        except ValueError as exc:
            raise SystemExit(f"gen-sbom: SOURCE_DATE_EPOCH {epoch!r} is not an integer") from exc
        return datetime.datetime.fromtimestamp(seconds, datetime.timezone.utc).strftime(
            "%Y-%m-%dT%H:%M:%SZ")
    return None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="write a CycloneDX 1.6 SBOM for the shipped binary")
    parser.add_argument("--repo", type=Path, default=Path("."), help="repository root")
    parser.add_argument("--metadata", type=Path, default=None,
                        help="a saved `cargo metadata` file instead of running cargo")
    parser.add_argument("--output", type=Path, default=None, help="file to write (default stdout)")
    parser.add_argument("--check", type=Path, default=None,
                        help="fail unless this file already holds exactly what would be written")
    parser.add_argument("--timestamp", default=None,
                        help="stamp metadata.timestamp; absent by default so runs are comparable")
    parser.add_argument("--lock", type=Path, default=None,
                        help="cargo lock to read `hashes` from (default <repo>/Cargo.lock)")
    args = parser.parse_args(argv)

    lib = load_lib()
    if args.metadata is not None:
        if not args.metadata.is_file():
            raise SystemExit(f"gen-sbom: {args.metadata} is not a file; save one with "
                             f"`cargo metadata --frozen --format-version 1 > {args.metadata}`")
        try:
            metadata = json.loads(args.metadata.read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise SystemExit(f"gen-sbom: {args.metadata} could not be read as "
                             f"`cargo metadata` JSON: {exc}") from exc
    else:
        try:
            metadata = lib.load_metadata(args.repo)
        except lib.Unreadable as exc:
            raise SystemExit(f"gen-sbom: {exc}") from exc

    document = build(lib, args.repo, metadata, resolve_timestamp(args), args.lock)
    text = dumps(document)
    counts = {"dependency": 0, "workspace": 0, "vendored": 0}
    hashed = 0
    for component_entry in document["components"]:
        if component_entry.get("hashes"):
            hashed += 1
        for prop in component_entry["properties"]:
            if prop["name"] == "chaos:origin":
                counts[prop["value"]] += 1
    summary = (f"components: {len(document['components'])} "
               f"(third-party {counts['dependency']}, vendored {counts['vendored']}, "
               f"workspace {counts['workspace']}); "
               f"dependency edges: {sum(len(d['dependsOn']) for d in document['dependencies'])}; "
               f"SHA-256 on {hashed}; serial {document['serialNumber']}")

    if args.check is not None:
        if not args.check.is_file():
            print(f"gen-sbom: {args.check} does not exist; run without --check to write it",
                  file=sys.stderr)
            return 1
        current = args.check.read_text(encoding="utf-8")
        if current == text:
            print(f"gen-sbom: {args.check} is current; {summary}", file=sys.stderr)
            return 0
        print(f"gen-sbom: {args.check} is stale; {summary}\n"
              f"  stored {len(current)} bytes, this build {len(text)} bytes", file=sys.stderr)
        return 1

    if args.output is not None:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(text, encoding="utf-8")
        print(f"gen-sbom: wrote {args.output}; {summary}", file=sys.stderr)
        return 0
    sys.stdout.write(text)
    print(f"gen-sbom: {summary}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
