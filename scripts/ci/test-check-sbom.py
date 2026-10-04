#!/usr/bin/env python3
"""Fixtures for `check-sbom.py`, and with them for `scripts/gen-sbom.py`'s reading of cargo metadata.

The thing these cases have to prove is not that a JSON parser works. It is that the SBOM describes
*this* build: that a package reachable only through a test edge is absent, that a vendored crate is
present and not called our own, that a build-script-only crate says so, and that the graph an analyst
would trace a vulnerability through is the build's edge set rather than a plausible picture of one.

Three disciplines make that provable, and the first two come from the notices fixtures:

- the baseline document is produced by running the real generator on a synthetic `cargo metadata`,
  never written by hand. A hand-written document would test the guard against a picture of the
  generator's output, and the two could drift apart with nothing failing.
- every membership decision is pinned from both sides. The case that says "a dev-only dependency is
  not in the SBOM" also adds a component for it and expects the guard to refuse it, because a guard
  that read no graph at all would report nothing missing and pass the first half alone.
- mutations go to the generated document, which is what an editor or a bad rebase would produce, and
  the guard is then run over the same metadata it is accused of describing.

The last class runs both scripts over this repository's real workspace, which is the only case here
that needs a toolchain, and the reason this file joins `check-sbom.py` in
`scripts/ci/docker-entry-ci-only.tsv`.

    python3 scripts/ci/test-check-sbom.py
"""

import importlib.util
import json
import hashlib
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

CHECK = Path(__file__).with_name("check-sbom.py")
GEN = CHECK.parents[1] / "gen-sbom.py"
REPO = CHECK.parents[2]

_spec = importlib.util.spec_from_file_location("notices_lib", CHECK.parents[1] / "notices_lib.py")
notices = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(notices)

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
MANIFEST = Path("crates/codegen/xai-grok-pager/npm/chaos/package.json")
PRODUCT = "chaos-code-test"


def package(name: str, version: str, *, license_id: str = "MIT", member: bool = False,
            vendored: bool = False, git: bool = False) -> dict:
    """One `cargo metadata` package entry, in the shape cargo actually writes."""
    if member or vendored:
        identifier = f"{name} {version} (path+file:///w/{name})"
        manifest = f"/w/third_party/{name}/Cargo.toml" if vendored else f"/w/{name}/Cargo.toml"
        source = None
    elif git:
        source = f"git+https://example.invalid/{name}.git?rev=dead#dead"
        identifier = f"{name} {version} ({source})"
        manifest = f"/cargo/git/{name}-1/dead/Cargo.toml"
    else:
        source = REGISTRY
        identifier = f"{name} {version} ({source})"
        manifest = f"/registry/src/index.crates.io/{name}-{version}/Cargo.toml"
    return {
        "id": identifier,
        "name": name,
        "version": version,
        "license": license_id,
        "source": source,
        "manifest_path": manifest,
        "description": f"the {name} crate",
        "authors": [f"{name} authors <hi@example.invalid>"],
        "repository": f"https://example.invalid/{name}",
    }


def metadata(packages: list[dict], edges: dict[str, list[tuple[str, str]]]) -> dict:
    """A `cargo metadata` document whose resolve graph carries the given kind on every edge."""
    nodes = [{"id": pid,
              "deps": [{"pkg": child, "dep_kinds": [{"kind": kind, "target": None}]}
                       for child, kind in kids]}
             for pid, kids in edges.items()]
    return {
        "packages": packages,
        "workspace_members": [pkg["id"] for pkg in packages if "(path+file:" in pkg["id"]],
        "resolve": {"nodes": nodes},
    }


def workspace(*, dev_leaf: bool = False, build_leaf: bool = False, vendored_leaf: bool = False,
              member_leaf: bool = False, git_leaf: bool = False,
              two_versions: bool = False) -> dict:
    """A root binary, one direct dependency, and one leaf whose shape each case changes.

    `dev_leaf` is reached only through a test edge, `build_leaf` only through a build script,
    `vendored_leaf` is somebody else's code under `third_party/`, `member_leaf` is our own crate,
    `git_leaf` comes from a fork rather than crates.io, and `two_versions` adds a second version of
    the leaf, which is how 101 of the 998 names in the shipped set arrive.
    """
    root = package(notices.SHIPPED_ROOT, "0.1.0", member=True, license_id="Apache-2.0")
    middle = package("middle", "1.0.0", license_id="MIT OR Apache-2.0")
    leaf_kind = "dev" if dev_leaf else ("build" if build_leaf else "normal")
    leaf = package("leaf", "1.0.0", member=member_leaf, vendored=vendored_leaf, git=git_leaf)
    packages = [root, middle, leaf]
    edges = {
        root["id"]: [(middle["id"], "normal")],
        middle["id"]: [(leaf["id"], leaf_kind)],
        leaf["id"]: [],
    }
    if two_versions:
        other = package("leaf", "0.9.0")
        packages.append(other)
        edges[middle["id"]].append((other["id"], "normal"))
        edges[other["id"]] = []
    return metadata(packages, edges)


def digest_for(name: str, version: str = "1.0.0") -> str:
    """A stand-in for the `.crate` digest a registry would publish for one package."""
    return hashlib.sha256(f"{name}@{version}".encode("utf-8")).hexdigest()


def load_module(name: str, path: Path):
    """Import one of the two scripts under test, by path, for the checks that need their functions."""
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def writer():
    return load_module("gen_sbom_under_test", GEN)


def reader():
    return load_module("check_sbom_under_test", CHECK)


def lock_text(meta: dict) -> str:
    """A `Cargo.lock` for a metadata document: a checksum on the registry packages only.

    That is the shape cargo writes -- `checksum` appears for registry sources and nowhere else -- and
    it is what makes "no `hashes` on the git and path components" a tested claim rather than a hope.
    """
    blocks = []
    for pkg in meta["packages"]:
        lines = ["[[package]]", f'name = "{pkg["name"]}"', f'version = "{pkg["version"]}"']
        source = pkg.get("source")
        if source:
            lines.append(f'source = "{source}"')
        if (source or "").startswith("registry+"):
            lines.append(f'checksum = "{digest_for(pkg["name"], pkg["version"])}"')
        lines.append('dependencies = ["dep 1.0.0 (registry+https://github.com/rust-lang/crates.io-index)"]')
        blocks.append("\n".join(lines))
    return "# Test lock\nversion = 3\n\n" + "\n\n".join(blocks) + "\n"


def add_lock_checksum(text: str, name: str) -> str:
    """Put a `checksum` line on one package's lock block, registry or not.

    Cargo would not write that line for a path or git package, so the result is the shape a hand
    edited lock has, which is what the two scripts are supposed to refuse.
    """
    blocks = text.split("\n\n")
    marked = False
    for index, block in enumerate(blocks):
        if block.startswith("[[package]]") and f'name = "{name}"\n' in block + "\n":
            blocks[index] = block + f'\nchecksum = "{digest_for(name)}"'
            marked = True
    if not marked:
        raise AssertionError(f"no lock block for {name} in:\n{text}")
    return "\n\n".join(blocks)


def build(meta, *, manifest: bool = True) -> Path:
    """A throwaway repository holding `meta` and the npm manifest the product identity reads."""
    tmp = Path(tempfile.mkdtemp())
    (tmp / "metadata.json").write_text(json.dumps(meta) if not isinstance(meta, str) else meta,
                                       encoding="utf-8")
    if isinstance(meta, dict):
        (tmp / "Cargo.lock").write_text(lock_text(meta), encoding="utf-8")
    if manifest:
        target = tmp / MANIFEST
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps({"name": PRODUCT, "version": "9.9.9",
                                      "homepage": "https://example.invalid/chaos",
                                      "repository": {"url": "git+https://example.invalid/c.git"}}),
                          encoding="utf-8")
    return tmp


def run_generator(tmp: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, str(GEN), "--repo", str(tmp),
                           "--metadata", str(tmp / "metadata.json"), *args],
                          capture_output=True, text=True)


def generate(tmp: Path, name: str = "bom.json") -> str:
    proc = run_generator(tmp, "--output", str(tmp / name))
    assert proc.returncode == 0, proc.stdout + proc.stderr
    return (tmp / name).read_text(encoding="utf-8")


def fixture(*, manifest: bool = True, meta: dict | None = None, **kwargs) -> tuple[Path, str]:
    """A generated baseline document plus the directory it was written into."""
    tmp = build(meta if meta is not None else workspace(**kwargs), manifest=manifest)
    return tmp, generate(tmp)


def check(tmp: Path, text: str, *args: str) -> subprocess.CompletedProcess:
    path = tmp / "under-test.json"
    path.write_text(text, encoding="utf-8")
    return subprocess.run(
        [sys.executable, str(CHECK), "--sbom", str(path), "--root", str(tmp),
         "--metadata", str(tmp / "metadata.json"), *args],
        capture_output=True, text=True)


def edit(text: str, change) -> str:
    """Apply `change(document)` to a parsed copy and re-serialize the way the generator does."""
    document = json.loads(text)
    change(document)
    return json.dumps(document, indent=2, ensure_ascii=False) + "\n"


def find(document: dict, name: str, version: str | None = None) -> dict:
    for component in document["components"]:
        if component["name"] == name and (version is None or component["version"] == version):
            return component
    raise KeyError(f"{name} {version or ''}".strip())


def drop(document: dict, name: str, version: str | None = None) -> None:
    """Remove a component and every statement that mentions it, leaving coverage as the only gap."""
    reference = find(document, name, version)["bom-ref"]
    document["components"] = [c for c in document["components"] if c["bom-ref"] != reference]
    document["dependencies"] = [d for d in document["dependencies"] if d["ref"] != reference]
    for entry in document["dependencies"]:
        entry["dependsOn"] = [child for child in entry.get("dependsOn", [])
                              if child != reference]


def flags(component: dict) -> dict[str, str]:
    return {item["name"]: item["value"] for item in component["properties"]}


def set_flag(component: dict, key: str, value: str) -> None:
    for item in component["properties"]:
        if item["name"] == key:
            item["value"] = value
            return
    component["properties"].append({"name": key, "value": value})


class SbomCase(unittest.TestCase):
    """Shared assertion: the guard refuses, and says the named thing while doing it."""

    def assert_refused(self, tmp: Path, text: str, expected: str) -> None:
        proc = check(tmp, text)
        self.assertEqual(proc.returncode, 1, f"expected {expected!r}, got:\n{proc.stdout}"
                                             f"{proc.stderr}")
        self.assertIn(expected, proc.stdout)

    def assert_accepted(self, tmp: Path, text: str) -> None:
        proc = check(tmp, text)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)


class Baseline(SbomCase):
    """The generator's own output has to pass, or every failure below is meaningless."""

    def test_a_small_workspace_passes(self) -> None:
        tmp, text = fixture()
        proc = check(tmp, text)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        # Two components because the crate that builds the binary is the product, not a component.
        # The digest count is pinned too: it is the figure the release notes quote, and a walk that
        # silently stopped reading the lock would still produce a valid-looking document.
        self.assertIn("check-sbom: OK (2 component(s): third-party 2 (matching "
                      "THIRD-PARTY-NOTICES), workspace 0; 2 dependency edges; SHA-256 on 2",
                      proc.stdout)

    def test_the_product_is_named_from_the_manifest_it_ships(self) -> None:
        tmp, text = fixture()
        document = json.loads(text)
        self.assertEqual(document["metadata"]["component"]["name"], PRODUCT)
        self.assertEqual(document["metadata"]["component"]["version"], "0.1.0",
                         "the version is the root crate's, not the npm manifest's")
        self.assert_accepted(tmp, text)
        renamed = edit(text, lambda doc: doc["metadata"]["component"].__setitem__("name", "other"))
        self.assert_refused(tmp, renamed, "the product this repository ships is named")

    def test_the_binarys_own_crate_is_the_product_not_a_component(self) -> None:
        tmp, text = fixture()
        self.assertNotIn(notices.SHIPPED_ROOT,
                         [c["name"] for c in json.loads(text)["components"]])
        listed_twice = edit(text, lambda doc: doc["components"].append({
            "type": "library", "bom-ref": f"pkg:cargo/{notices.SHIPPED_ROOT}@0.1.0",
            "name": notices.SHIPPED_ROOT, "version": "0.1.0",
            "purl": f"pkg:cargo/{notices.SHIPPED_ROOT}@0.1.0", "scope": "required",
            "licenses": [{"expression": "Apache-2.0"}],
            "properties": [{"name": "chaos:origin", "value": "workspace"}],
        }))
        self.assert_refused(tmp, listed_twice, "should be metadata.component, not a component")


class Coverage(SbomCase):
    """Which packages are in the document, pinned from both sides each."""

    def test_a_dev_only_dependency_is_absent_and_must_stay_absent(self) -> None:
        meta = workspace(dev_leaf=True)
        tmp, text = fixture(meta=meta)
        self.assertNotIn("leaf", [c["name"] for c in json.loads(text)["components"]],
                         "a package reached only through a test edge is not in the binary")
        self.assert_accepted(tmp, text)
        # The other half: were the generator to include it, the guard would have to say so.
        added = json.loads(text)
        added["components"].append({
            "type": "library", "bom-ref": "pkg:cargo/leaf@1.0.0", "name": "leaf",
            "version": "1.0.0", "purl": "pkg:cargo/leaf@1.0.0", "scope": "required",
            "licenses": [{"expression": "MIT"}],
            "properties": [{"name": "chaos:origin", "value": "dependency"},
                           {"name": "chaos:cargo:edge-kinds", "value": "dev"},
                           {"name": "chaos:cargo:source-kind", "value": "registry"}],
        })
        self.assert_refused(tmp, json.dumps(added, indent=2, ensure_ascii=False) + "\n",
                            "the build does not reach it")

    def test_a_dropped_component_is_reported_by_name(self) -> None:
        tmp, text = fixture()
        proc = check(tmp, edit(text, lambda doc: drop(doc, "leaf")))
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("1 package(s) the build reaches are not in the SBOM", proc.stdout)
        self.assertIn("leaf 1.0.0", proc.stdout)

    def test_a_transitive_dependency_is_its_own_component(self) -> None:
        # `leaf` arrives through `middle`, so a document that lists only `middle` is missing a
        # package rather than summarising one.
        tmp, text = fixture()
        self.assertIn("leaf 1.0.0", check(tmp, edit(text, lambda doc: drop(doc, "leaf"))).stdout)

    def test_two_versions_of_one_name_are_two_components(self) -> None:
        meta = workspace(two_versions=True)
        tmp, text = fixture(meta=meta)
        self.assert_accepted(tmp, text)
        leaves = [c for c in json.loads(text)["components"] if c["name"] == "leaf"]
        self.assertEqual(sorted(c["version"] for c in leaves), ["0.9.0", "1.0.0"])
        # Collapsing them onto one name is the reading this repository already got wrong once.
        collapsed = edit(text, lambda doc: drop(doc, "leaf", "0.9.0"))
        self.assert_refused(tmp, collapsed, "leaf 0.9.0")

    def test_this_workspaces_own_crates_are_not_third_party(self) -> None:
        meta = workspace(member_leaf=True)
        tmp, text = fixture(meta=meta)
        self.assert_accepted(tmp, text)
        document = json.loads(text)
        self.assertEqual(flags(find(document, "leaf"))["chaos:origin"], "workspace")
        # Only `middle` is third-party here, which is what the notices count in the verdict says.
        self.assertIn("third-party 1 (matching THIRD-PARTY-NOTICES), workspace 1",
                      check(tmp, text).stdout)

    def test_a_vendored_crate_is_third_party_and_says_so(self) -> None:
        # Five of the 1139 shipped packages are like this, which is the whole reason it is tested.
        meta = workspace(vendored_leaf=True)
        tmp, text = fixture(meta=meta)
        self.assert_accepted(tmp, text)
        document = json.loads(text)
        self.assertEqual(flags(find(document, "leaf"))["chaos:origin"], "vendored")
        self.assertEqual(flags(find(document, "leaf"))["chaos:vendored-with-local-modifications"],
                         "true")
        self.assertIn("third-party 2 (matching THIRD-PARTY-NOTICES)", check(tmp, text).stdout)

    def test_a_git_dependency_is_not_claimed_to_come_from_crates_io(self) -> None:
        meta = workspace(git_leaf=True)
        tmp, text = fixture(meta=meta)
        self.assert_accepted(tmp, text)
        self.assertTrue(find(json.loads(text), "leaf")["purl"].startswith("pkg:generic/"),
                        "a fork resolved against crates.io would score the wrong artifact")
        self.assertEqual(flags(find(json.loads(text), "leaf"))["chaos:cargo:source-kind"], "git")
        moved = edit(text, lambda doc: find(doc, "leaf").__setitem__(
            "purl", find(doc, "leaf")["purl"].replace("pkg:generic/", "pkg:cargo/")))
        self.assert_refused(tmp, moved, "purl namespace does not match where the package came from")

    def test_a_registry_package_moved_to_the_other_namespace_is_reported(self) -> None:
        tmp, text = fixture()
        moved = edit(text, lambda doc: find(doc, "middle").__setitem__(
            "purl", find(doc, "middle")["purl"].replace("pkg:cargo/", "pkg:generic/")))
        self.assert_refused(tmp, moved, "purl namespace does not match where the package came from")


class Truthfulness(SbomCase):
    """Each field that is a claim about a package, checked against the package."""

    def test_a_license_that_is_not_the_packages_is_reported(self) -> None:
        tmp, text = fixture()
        wrong = edit(text, lambda doc: find(doc, "middle").__setitem__(
            "licenses", [{"expression": "GPL-3.0-only"}]))
        self.assert_refused(tmp, wrong, "license does not match the package")
        self.assertIn("GPL-3.0-only", check(tmp, wrong).stdout)

    def test_a_missing_license_entry_is_reported(self) -> None:
        tmp, text = fixture()
        self.assert_refused(tmp, edit(text, lambda doc: find(doc, "middle").pop("licenses")),
                            "licenses should be one expression")

    def test_the_slash_shorthand_survives_as_an_spdx_expression(self) -> None:
        meta = workspace()
        for entry in meta["packages"]:
            if entry["name"] == "middle":
                entry["license"] = "MIT/Apache-2.0"
        tmp, text = fixture(meta=meta)
        document = json.loads(text)
        self.assertEqual(find(document, "middle")["licenses"],
                         [{"expression": "MIT OR Apache-2.0"}])
        self.assert_accepted(tmp, text)
        # The string cargo reported is kept, because "we normalised it" has to be auditable.
        self.assertIn({"name": "chaos:cargo:license-declared", "value": "MIT/Apache-2.0"},
                      find(document, "middle")["properties"])

    def test_a_build_script_only_dependency_is_scope_optional(self) -> None:
        meta = workspace(build_leaf=True)
        tmp, text = fixture(meta=meta)
        self.assertEqual(find(json.loads(text), "leaf")["scope"], "optional")
        self.assertEqual(flags(find(json.loads(text), "leaf"))["chaos:cargo:edge-kinds"], "build")
        self.assert_accepted(tmp, text)
        lying = edit(text, lambda doc: find(doc, "leaf").__setitem__("scope", "required"))
        self.assert_refused(tmp, lying, "scope does not match the dependency edge kinds")

    def test_a_normal_dependency_is_not_marked_optional(self) -> None:
        tmp, text = fixture()
        lying = edit(text, lambda doc: find(doc, "middle").__setitem__("scope", "optional"))
        self.assert_refused(tmp, lying, "scope does not match the dependency edge kinds")

    def test_a_wrong_origin_is_reported(self) -> None:
        tmp, text = fixture()
        lying = edit(text, lambda doc: set_flag(find(doc, "middle"), "chaos:origin", "workspace"))
        self.assert_refused(tmp, lying, "chaos:origin does not match the build")

    def test_the_origin_and_the_notices_membership_are_two_assertions(self) -> None:
        # Calling a third-party package our own moves it out of the set THIRD-PARTY-NOTICES covers,
        # so both readings object. Calling a registry package vendored does not, and only the
        # per-package check objects -- which is what makes them two checks rather than one.
        tmp, text = fixture()
        as_ours = edit(text, lambda doc: set_flag(find(doc, "middle"), "chaos:origin", "workspace"))
        output = check(tmp, as_ours).stdout
        self.assertIn("chaos:origin does not match the build", output)
        self.assertIn("describe different third-party sets", output)
        as_vendored = edit(text, lambda doc: set_flag(find(doc, "middle"), "chaos:origin",
                                                      "vendored"))
        output = check(tmp, as_vendored).stdout
        self.assertIn("chaos:origin does not match the build", output)
        self.assertNotIn("describe different third-party sets", output)

    def test_a_component_listed_twice_is_reported(self) -> None:
        tmp, text = fixture()
        duplicated = edit(text, lambda doc: doc["components"].append(
            json.loads(json.dumps(find(doc, "middle")))))
        self.assert_refused(tmp, duplicated, "is listed twice")

    def test_a_bom_ref_that_is_not_the_purl_is_reported(self) -> None:
        tmp, text = fixture()
        split = edit(text, lambda doc: find(doc, "middle").__setitem__("bom-ref",
                                                                       "pkg:cargo/middle@1.0.0#1"))
        self.assert_refused(tmp, split, "is not its purl")

    def test_a_purl_naming_some_other_package_is_reported(self) -> None:
        tmp, text = fixture()
        wrong = edit(text, lambda doc: find(doc, "middle").update(
            {"bom-ref": "pkg:cargo/other@2.0.0", "purl": "pkg:cargo/other@2.0.0"}))
        self.assert_refused(tmp, wrong, "its purl names other@2.0.0")


class Graph(SbomCase):
    """The reachability statement, which is the reason an SBOM beats an inventory list."""

    def test_the_product_hangs_off_the_binarys_own_crate(self) -> None:
        tmp, text = fixture()
        document = json.loads(text)
        first = document["dependencies"][0]
        self.assertEqual(first["ref"], document["metadata"]["component"]["bom-ref"])
        self.assertEqual(first["dependsOn"], ["pkg:cargo/middle@1.0.0"])
        self.assertEqual([d["ref"] for d in document["dependencies"]],
                         [document["metadata"]["component"]["bom-ref"]]
                         + [c["bom-ref"] for c in document["components"]])

    def test_an_edge_the_build_has_must_be_in_the_document(self) -> None:
        tmp, text = fixture()

        def cut(document: dict) -> None:
            find_dependency(document, "middle")["dependsOn"] = []

        self.assert_refused(tmp, edit(text, cut), "edge(s) the build has are absent")
        self.assertIn("pkg:cargo/middle@1.0.0 -> pkg:cargo/leaf@1.0.0",
                      check(tmp, edit(text, cut)).stdout)

    def test_an_edge_the_build_does_not_have_must_not_be(self) -> None:
        tmp, text = fixture()

        def add(document: dict) -> None:
            document["dependencies"][0]["dependsOn"].append("pkg:cargo/leaf@1.0.0")

        self.assert_refused(tmp, edit(text, add), "edge(s) the build does not have are present")

    def test_a_dependency_on_a_component_that_is_not_there_is_reported(self) -> None:
        tmp, text = fixture()

        def phantom(document: dict) -> None:
            document["dependencies"][0]["dependsOn"].append("pkg:cargo/phantom@9.9.9")

        self.assert_refused(tmp, edit(text, phantom), "which is not a component")

    def test_every_component_needs_exactly_one_dependency_object(self) -> None:
        tmp, text = fixture()
        duplicated = edit(text, lambda doc: doc["dependencies"].__setitem__(
            1, json.loads(json.dumps(doc["dependencies"][2]))))
        self.assertIn("more than one dependency object", check(tmp, duplicated).stdout)
        removed = edit(text, lambda doc: doc["dependencies"].pop(1))
        self.assertIn("no dependency object for", check(tmp, removed).stdout)

    def test_a_dependency_object_naming_nothing_is_reported(self) -> None:
        tmp, text = fixture()
        stray = edit(text, lambda doc: doc["dependencies"].append(
            {"ref": "pkg:cargo/stray@1.0.0", "dependsOn": []}))
        self.assert_refused(tmp, stray, "a dependency object names")


def find_dependency(document: dict, name: str) -> dict:
    reference = find(document, name)["bom-ref"]
    return next(entry for entry in document["dependencies"] if entry["ref"] == reference)


class Digests(SbomCase):
    """`hashes` is the one claim a reader can verify without trusting either script.

    So the fixture asserts it against the lock file on disk: the generator must publish the digest
    the lock records, the guard must refuse any other value, and a local package must carry none,
    because cargo records no digest for the bytes it read out of a directory.
    """

    def test_registry_components_carry_the_digest_the_lock_records(self) -> None:
        tmp, text = fixture()
        document = json.loads(text)
        for name in ("middle", "leaf"):
            self.assertEqual(find(document, name).get("hashes"),
                             [{"alg": "SHA-256", "content": digest_for(name)}],
                             f"{name} came from crates.io, so its digest should be the lock's")
        self.assert_in_success("SHA-256 on 2", check(tmp, text))

    def test_a_local_or_git_component_carries_no_digest(self) -> None:
        for kwargs in ({"member_leaf": True}, {"vendored_leaf": True}, {"git_leaf": True}):
            with self.subTest(**kwargs):
                document = json.loads(fixture(**kwargs)[1])
                local = find(document, "leaf")
                self.assertNotIn("hashes", local,
                                 "cargo records no checksum for a path or git package, so the SBOM "
                                 "must not invent one")
                self.assertEqual(find(document, "middle")["hashes"][0]["content"],
                                 digest_for("middle"), "the registry component still has its own")

    def test_a_digest_borrowed_from_another_package_is_reported(self) -> None:
        tmp = build(workspace())
        document = json.loads(generate(tmp))
        find(document, "leaf")["hashes"] = find(document, "middle")["hashes"]
        self.assert_refused(tmp, json.dumps(document, indent=2, ensure_ascii=False) + "\n",
                            "leaf 1.0.0: hash is")

    def test_a_dropped_digest_is_reported(self) -> None:
        tmp, text = fixture()
        stolen = edit(text, lambda doc: find(doc, "middle").pop("hashes"))
        self.assert_refused(tmp, stolen, "middle 1.0.0: hashes should be the one SHA-256")

    def test_a_digest_on_a_local_package_is_reported(self) -> None:
        tmp, text = fixture(member_leaf=True)
        invented = edit(text, lambda doc: find(doc, "leaf").__setitem__(
            "hashes", [{"alg": "SHA-256", "content": digest_for("leaf")}]))
        self.assert_refused(tmp, invented, "for which cargo records no digest")

    def test_a_digest_that_is_not_the_locks_is_reported(self) -> None:
        # The lock is edited, not the document: this is the case where the document was written
        # against a different build than the lock in front of the guard.
        tmp, text = fixture()
        lock = tmp / "Cargo.lock"
        lock.write_text(lock.read_text(encoding="utf-8").replace(digest_for("middle"),
                                                                 digest_for("middle", "other")),
                        encoding="utf-8")
        self.assert_refused(tmp, text, "middle 1.0.0: hash is")

    def test_a_digest_the_lock_does_not_record_is_reported(self) -> None:
        tmp, text = fixture()
        lines = (tmp / "Cargo.lock").read_text(encoding="utf-8").splitlines()
        (tmp / "Cargo.lock").write_text("\n".join(
            line for line in lines if line != f'checksum = "{digest_for("middle")}"') + "\n",
            encoding="utf-8")
        self.assert_refused(tmp, text, "Cargo.lock records no checksum for it")

    def test_a_malformed_digest_is_reported(self) -> None:
        for alg, content, expected in (
            ("SHA-1", digest_for("middle"), "hash alg is 'SHA-1'"),
            ("SHA-256", "deadbeef", "is not 64 lowercase hex digits"),
        ):
            with self.subTest(alg=alg, content=content):
                tmp, text = fixture()
                broken = edit(text, lambda doc, a=alg, c=content: find(doc, "middle").__setitem__(
                    "hashes", [{"alg": a, "content": c}]))
                self.assert_refused(tmp, broken, expected)

    def test_two_hashes_on_one_component_are_reported(self) -> None:
        tmp, text = fixture()
        doubled = edit(text, lambda doc: find(doc, "middle").__setitem__(
            "hashes", [{"alg": "SHA-256", "content": digest_for("middle")}] * 2))
        self.assert_refused(tmp, doubled, "hashes should be the one SHA-256")

    def test_the_generator_refuses_a_lock_that_digests_a_package_cargo_has_no_digest_for(self) -> None:
        # The rule "only crates.io packages get `hashes`" is only real if a lock that contradicts it
        # stops the build instead of quietly becoming a document with a hash on a path package.
        tmp = build(workspace(member_leaf=True))
        lock = tmp / "Cargo.lock"
        lock.write_text(add_lock_checksum(lock.read_text(encoding="utf-8"), "leaf"),
                        encoding="utf-8")
        proc = run_generator(tmp, "--output", str(tmp / "out.json"))
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("non-registry package(s) carry a lock", proc.stderr)
        self.assertFalse((tmp / "out.json").exists(),
                         "refusing means writing nothing, not writing and then complaining")

    def test_the_guard_refuses_a_lock_that_digests_a_package_cargo_has_no_digest_for(self) -> None:
        # The guard reads the lock as evidence, so a lock cargo could not have written is a finding
        # even when the document in front of it happens to be clean.
        tmp, text = fixture(member_leaf=True)
        lock = tmp / "Cargo.lock"
        lock.write_text(add_lock_checksum(lock.read_text(encoding="utf-8"), "leaf"),
                        encoding="utf-8")
        self.assert_refused(tmp, text, "records a checksum but does not come from a registry")

    def test_the_guard_refuses_a_lock_that_lists_one_package_with_two_digests(self) -> None:
        # Both parsers have to notice a lock that contradicts itself, or the digest the SBOM
        # publishes is "the one the parser happened to keep" rather than the one cargo recorded.
        tmp, text = fixture()
        lock = tmp / "Cargo.lock"
        original = lock.read_text(encoding="utf-8")
        block = next(b for b in original.split("\n\n")
                     if b.startswith("[[package]]") and 'name = "middle"\n' in b + "\n")
        stale = block.replace(f'checksum = "{digest_for("middle")}"',
                              f'checksum = "{digest_for("middle", "stale")}"')
        lock.write_text(original.rstrip("\n") + "\n\n" + stale + "\n", encoding="utf-8")
        self.assert_refused(tmp, text, "listed twice with")
        proc = run_generator(tmp, "--output", str(tmp / "out.json"))
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("twice with different", proc.stderr)

    def test_the_guard_refuses_a_lock_digest_that_is_not_hex(self) -> None:
        tmp, text = fixture()
        lock = tmp / "Cargo.lock"
        lock.write_text(lock.read_text(encoding="utf-8").replace(
            f'checksum = "{digest_for("middle")}"', 'checksum = "not-a-digest"'), encoding="utf-8")
        self.assert_refused(tmp, text, "not 64 lowercase hex digits")

    def test_the_generator_refuses_a_registry_package_the_lock_has_no_digest_for(self) -> None:
        tmp = build(workspace())
        lock = tmp / "Cargo.lock"
        lock.write_text(lock.read_text(encoding="utf-8").replace(
            f'checksum = "{digest_for("middle")}"\n', ""), encoding="utf-8")
        proc = run_generator(tmp, "--output", str(tmp / "out.json"))
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("have no `checksum`", proc.stderr)

    def test_the_generator_refuses_a_missing_lock(self) -> None:
        tmp = build(workspace())
        (tmp / "Cargo.lock").unlink()
        proc = run_generator(tmp, "--output", str(tmp / "out.json"))
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("is not a file", proc.stderr)

    def test_the_guard_says_so_when_the_lock_is_gone(self) -> None:
        tmp, text = fixture()
        (tmp / "Cargo.lock").unlink()
        self.assert_refused(tmp, text, "so the document's `hashes` cannot be checked")

    def test_the_two_lock_parsers_agree_on_the_same_file(self) -> None:
        # The generator and the guard parse the lock differently on purpose. If they ever disagree
        # about which digest belongs to which package, the SBOM and its check disagree too, and a
        # wrong digest would then be an agreement rather than a finding.
        tmp = build(workspace(two_versions=True, git_leaf=True, vendored_leaf=True))
        generate(tmp)
        text = (tmp / "Cargo.lock").read_text(encoding="utf-8")
        self.assertEqual(writer().lock_digests(tmp / "Cargo.lock"),
                         reader().lock_digests(tmp / "Cargo.lock")[0],
                         f"the two parsers read the lock differently:\n{text}")

    def assert_in_success(self, expected: str, proc: subprocess.CompletedProcess) -> None:
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn(expected, proc.stdout)


class Envelope(SbomCase):
    """Format claims: a document a consumer cannot open describes nothing."""

    def test_another_spec_version_is_refused(self) -> None:
        tmp, text = fixture()
        self.assert_refused(tmp, edit(text, lambda doc: doc.__setitem__("specVersion", "1.4")),
                            "specVersion is '1.4'")

    def test_the_components_and_dependencies_blocks_have_to_exist(self) -> None:
        tmp, text = fixture()
        self.assert_refused(tmp, edit(text, lambda doc: doc.pop("components")),
                            "components is missing or empty")
        self.assert_refused(tmp, edit(text, lambda doc: doc.pop("dependencies")),
                            "dependencies is missing")

    def test_the_product_has_to_be_the_application(self) -> None:
        tmp, text = fixture()
        self.assert_refused(tmp, edit(text, lambda doc: doc["metadata"]["component"].__setitem__(
            "type", "library")), "the thing being described is an application")

    def test_the_version_behind_the_product_has_to_be_the_builds(self) -> None:
        tmp, text = fixture()
        self.assert_refused(tmp, edit(text, lambda doc: doc["metadata"]["component"].__setitem__(
            "version", "9.9.9")), "is the crate that builds the binary")

    def test_the_root_crate_has_to_be_named(self) -> None:
        tmp, text = fixture()
        self.assert_refused(tmp, edit(text, lambda doc: doc["metadata"]["component"].__setitem__(
            "properties", [])), "chaos:cargo:root-crate")


class SerialNumber(SbomCase):
    """The one property that makes two builds comparable, so it is checked against the bytes."""

    def test_the_serial_re_derives_from_the_document(self) -> None:
        tmp, text = fixture()
        self.assert_accepted(tmp, text)
        tampered = edit(text, lambda doc: doc.__setitem__(
            "serialNumber", "urn:uuid:00000000-0000-0000-0000-000000000000"))
        self.assert_refused(tmp, tampered, "does not re-derive from this document's own bytes")

    def test_a_serial_that_is_not_a_uuid_is_refused(self) -> None:
        tmp, text = fixture()
        self.assertIn("is not a `urn:uuid:` UUID",
                      check(tmp, edit(text, lambda doc: doc.__setitem__(
                          "serialNumber", "1234"))).stdout)

    def test_any_content_change_moves_the_serial(self) -> None:
        # The pair that makes the serial a fingerprint: an edit that leaves the document valid in
        # every other way still fails, because the serial no longer follows from the bytes.
        tmp, text = fixture()
        edited = edit(text, lambda doc: find(doc, "middle").__setitem__("description", "edited"))
        self.assert_refused(tmp, edited, "does not re-derive")


class Determinism(SbomCase):
    """Two runs over one tree have to produce one file, or `--check` means nothing."""

    def test_two_runs_are_byte_identical(self) -> None:
        tmp = build(workspace())
        self.assertEqual(generate(tmp, "a.json"), generate(tmp, "b.json"))

    def test_the_document_is_ordered_by_origin_then_name_then_version(self) -> None:
        # Byte stability is only reviewable if the order follows from the content: a diff between two
        # SBOMs should show the package that changed, not the whole list moving.
        meta = workspace(two_versions=True)
        meta["packages"].append(package("aardvark", "0.1.0"))
        root = meta["packages"][0]["id"]
        node = next(n for n in meta["resolve"]["nodes"] if n["id"] == root)
        node["deps"].append({"pkg": meta["packages"][-1]["id"],
                             "dep_kinds": [{"kind": "normal", "target": None}]})
        meta["resolve"]["nodes"].append({"id": meta["packages"][-1]["id"], "deps": []})
        tmp, text = fixture(meta=meta)
        keys = [(flags(c)["chaos:origin"] != "dependency", flags(c)["chaos:origin"], c["name"],
                 [int(part) for part in c["version"].split(".")])
                for c in json.loads(text)["components"]]
        self.assertEqual(keys, sorted(keys), "components are not in the order the generator promises")
        self.assertEqual([key[2] for key in keys], ["aardvark", "leaf", "leaf", "middle"])
        self.assert_accepted(tmp, text)

    def test_the_document_order_does_not_depend_on_the_input_order(self) -> None:
        meta = workspace(two_versions=True)
        shuffled = dict(meta)
        shuffled["packages"] = list(reversed(meta["packages"]))
        shuffled["resolve"] = {"nodes": list(reversed(meta["resolve"]["nodes"]))}
        forward, forward_text = fixture(meta=meta)
        backward, backward_text = fixture(meta=shuffled)
        self.assertEqual(forward_text, backward_text,
                         "the document is ordered by (origin, name, version), not by cargo's order")
        self.assert_accepted(backward, backward_text)

    def test_check_reports_a_file_that_is_current_and_one_that_is_not(self) -> None:
        tmp = build(workspace())
        generate(tmp)
        current = run_generator(tmp, "--check", str(tmp / "bom.json"))
        self.assertEqual(current.returncode, 0, current.stdout + current.stderr)
        self.assertIn("is current", current.stderr)
        (tmp / "bom.json").write_text(generate(tmp, "other.json").replace(
            '"description": "the middle crate"', '"description": "edited"'), encoding="utf-8")
        stale = run_generator(tmp, "--check", str(tmp / "bom.json"))
        self.assertEqual(stale.returncode, 1, stale.stdout + stale.stderr)
        self.assertIn("is stale", stale.stderr)

    def test_a_timestamp_is_written_only_when_asked_for(self) -> None:
        tmp = build(workspace())
        self.assertNotIn("timestamp", json.loads(generate(tmp))["metadata"])
        stamped = run_generator(tmp, "--timestamp", "2026-10-05T00:00:00Z")
        self.assertEqual(stamped.returncode, 0, stamped.stderr)
        self.assertEqual(json.loads(stamped.stdout)["metadata"]["timestamp"],
                         "2026-10-05T00:00:00Z")
        # A stamp is a fact about the build, not a claim about the dependencies, so the guard is
        # indifferent to it and two stamped runs of one tree still agree.
        self.assert_accepted(tmp, stamped.stdout)
        bad = run_generator(tmp, "--timestamp", "yesterday")
        self.assertEqual(bad.returncode, 1, bad.stdout + bad.stderr)
        self.assertIn("is not YYYY-MM-DDTHH:MM:SSZ", bad.stderr)

    def test_source_date_epoch_stamps_a_reproducible_release(self) -> None:
        tmp = build(workspace())
        env = dict(os.environ, SOURCE_DATE_EPOCH="1791158400")
        proc = subprocess.run([sys.executable, str(GEN), "--repo", str(tmp),
                               "--metadata", str(tmp / "metadata.json")],
                              capture_output=True, text=True, env=env)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(json.loads(proc.stdout)["metadata"]["timestamp"], "2026-10-05T00:00:00Z")


class FailClosed(SbomCase):
    """A guard that goes quiet when it cannot see is worse than none, so each failure says why."""

    def test_a_missing_sbom_file_exits_two(self) -> None:
        tmp = build(workspace())
        proc = subprocess.run([sys.executable, str(CHECK), "--sbom", str(tmp / "nope.json"),
                               "--root", str(tmp), "--metadata", str(tmp / "metadata.json")],
                              capture_output=True, text=True)
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("is not a file", proc.stderr)

    def test_a_document_that_is_not_json_blames_the_document(self) -> None:
        tmp = build(workspace())
        proc = check(tmp, "not json at all")
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("is not JSON", proc.stdout)
        self.assertNotIn("could not be computed", proc.stdout)

    def test_a_document_that_is_a_json_array_blames_the_document(self) -> None:
        tmp = build(workspace())
        self.assertIn("expected an object", check(tmp, "[]").stdout)

    def test_metadata_that_is_not_json_blames_the_build(self) -> None:
        tmp, text = fixture()
        (tmp / "metadata.json").write_text("not json at all", encoding="utf-8")
        proc = check(tmp, text)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("could not be computed", proc.stdout)

    def test_the_generator_refuses_a_metadata_file_that_is_not_json(self) -> None:
        tmp = build("not json at all")
        proc = run_generator(tmp)
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("could not be read as `cargo metadata` JSON", proc.stderr)

    def test_metadata_without_a_shipped_root_is_a_failure(self) -> None:
        meta = workspace()
        meta["packages"] = [dict(p, name="other-" + p["name"]) for p in meta["packages"]]
        for node in meta["resolve"]["nodes"]:
            node["id"] = "other-" + node["id"]
            for dep in node["deps"]:
                dep["pkg"] = "other-" + dep["pkg"]
        tmp = build(meta)
        proc = run_generator(tmp)
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("no package named", proc.stderr)

    def test_a_generation_without_a_metadata_file_says_how_to_make_one(self) -> None:
        tmp = build(workspace())
        proc = run_generator(tmp, "--metadata", str(tmp / "absent.json"))
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("cargo metadata --frozen --format-version 1", proc.stderr)

    def test_a_document_on_stdin_is_read(self) -> None:
        tmp = build(workspace())
        proc = subprocess.run([sys.executable, str(CHECK), "--sbom", "-", "--root", str(tmp),
                               "--metadata", str(tmp / "metadata.json")],
                              input=generate(tmp), capture_output=True, text=True)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn("check-sbom: OK", proc.stdout)

    def test_the_example_list_is_capped_and_says_so(self) -> None:
        meta = workspace()
        root = meta["packages"][0]["id"]
        for index in range(4):
            extra = package(f"extra{index}", "1.0.0")
            meta["packages"].append(extra)
            node = next(n for n in meta["resolve"]["nodes"] if n["id"] == root)
            node["deps"].append({"pkg": extra["id"],
                                 "dep_kinds": [{"kind": "normal", "target": None}]})
            meta["resolve"]["nodes"].append({"id": extra["id"], "deps": []})
        tmp, text = fixture(meta=meta)
        pruned = json.loads(text)
        for index in range(4):
            drop(pruned, f"extra{index}")
        proc = check(tmp, json.dumps(pruned, indent=2, ensure_ascii=False) + "\n",
                     "--max-list", "2")
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("4 package(s) the build reaches are not in the SBOM", proc.stdout)
        self.assertIn("(and 2 more)", proc.stdout)


class RealWorkspace(SbomCase):
    """The synthetic document proves the logic; this proves it against the build that ships.

    `cargo metadata` over this workspace needs the registry, which is why this file and
    `check-sbom.py` are both recorded in `scripts/ci/docker-entry-ci-only.tsv` and both run in the
    `rust` CI job after `cargo check` warmed the cache.
    """

    def test_the_shipped_binarys_sbom_describes_this_workspace(self) -> None:
        tmp = Path(tempfile.mkdtemp())
        out = tmp / "chaos.cdx.json"
        generated = subprocess.run([sys.executable, str(GEN), "--repo", str(REPO),
                                    "--output", str(out)], capture_output=True, text=True,
                                   timeout=900)
        self.assertEqual(generated.returncode, 0, generated.stdout + generated.stderr)
        components = int(generated.stderr.split("components: ")[1].split()[0])
        # Four figures, because the binary really is this big. A walk that stopped at the direct
        # dependencies would report a few dozen and still look internally consistent.
        self.assertGreater(components, 1000, generated.stderr)
        checked = subprocess.run([sys.executable, str(CHECK), "--sbom", str(out),
                                  "--root", str(REPO)], capture_output=True, text=True,
                                 timeout=900)
        self.assertEqual(checked.returncode, 0, checked.stdout + checked.stderr)
        third_party = int(checked.stdout.split("third-party ")[1].split()[0])
        self.assertGreater(third_party, 1000, checked.stdout)
        hashed = int(checked.stdout.split("SHA-256 on ")[1].split(";")[0])
        # 1130 of the 1227 components are from crates.io today; the rest are our own crates, the
        # vendored copies and two git forks, and cargo records no digest for any of those.
        self.assertGreater(hashed, 1000, checked.stdout)

    def test_the_two_lock_parsers_agree_on_this_repository(self) -> None:
        """The generator's parse and the guard's parse, over the 1331-entry lock that ships.

        A digest is the one claim here a reader cannot re-derive from `cargo metadata`, so both
        scripts read `Cargo.lock` themselves. Two hand-written parsers of one file agree or one of
        them is wrong; this is where the difference would show up rather than in a passing CI job.
        """
        written = writer().lock_digests(REPO / "Cargo.lock")
        read, problems = reader().lock_digests(REPO / "Cargo.lock")
        self.assertEqual([], problems, "the lock this repository ships does not parse cleanly")
        self.assertEqual(written, read)
        self.assertGreater(len(written), 1000, "a lock this workspace resolves has over a thousand "
                                               "registry packages; a parser that found fewer is "
                                               "reading part of the file")

    def test_the_published_digests_match_the_downloaded_crates(self) -> None:
        """Against the bytes cargo actually fetched, not against the number next to their names.

        `Cargo.lock` is cargo's own record, so agreeing with it only proves the SBOM is as correct as
        the lock. The registry cache under `~/.cargo` holds the `.crate` files this workspace was
        built from, and a digest that does not match one of those is a digest that would certify the
        wrong bytes.
        """
        caches = sorted((Path.home() / ".cargo/registry/cache").glob("*/*.crate"))
        # A machine that has never built this workspace has no cache, and that is not a defect in the
        # SBOM; but a cache with a handful of crates in it means the read is wrong, so the floor is
        # loud rather than a silent skip.
        self.assertGreater(len(caches), 1000,
                           f"only {len(caches)} .crate files under ~/.cargo/registry/cache; this "
                           "case needs a machine that has built this workspace")
        on_disk = {path.name: path for path in caches}
        tmp = Path(tempfile.mkdtemp())
        out = tmp / "chaos.cdx.json"
        generated = subprocess.run([sys.executable, str(GEN), "--repo", str(REPO),
                                    "--output", str(out)], capture_output=True, text=True,
                                   timeout=900)
        self.assertEqual(generated.returncode, 0, generated.stdout + generated.stderr)
        document = json.loads(out.read_text(encoding="utf-8"))
        compared, wrong = 0, []
        for component in document["components"]:
            recorded = component.get("hashes") or []
            if not recorded:
                continue
            archive = on_disk.get(f"{component['name']}-{component['version']}.crate")
            if archive is None:
                continue
            compared += 1
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            if digest != recorded[0]["content"]:
                wrong.append(f"{component['name']} {component['version']}: SBOM says "
                             f"{recorded[0]['content']}, the file is {digest}")
        self.assertGreater(compared, 1000, "the SBOM's digests and the cache did not overlap")
        self.assertEqual([], wrong[:5], f"{len(wrong)} of {compared} published digests do not match "
                                        "the downloaded .crate")


if __name__ == "__main__":
    unittest.main(verbosity=2)
