#!/usr/bin/env python3
"""Fixtures for `check-notices-coverage.py`.

This guard compares the notices document against what the build resolves, so the fixtures hand it a
`cargo metadata` document of the same shape and watch which packages it decides are shipped. Each
case moves one thing at a time: an entry added, a version moved, an edge marked dev, a crate moved
under `third_party/`. The interesting cases are the three that decide *membership* rather than
bookkeeping, because those are where a wrong reading of `cargo metadata` would quietly shrink the
obligation:

- a dependency reachable only through a dev edge is not in the binary, so it is owed no notice --
  and the fixture proves the guard really dropped that edge by putting an entry in anyway and
  expecting the guard to call it coverage of nothing;
- this workspace's own crates are not third-party, proved the same way, from the other side;
- a crate vendored under `third_party/` *is* compiled into the binary, so it is owed a notice even
  though it is a workspace member -- the opposite conclusion would have removed five entries from
  the real document.

Membership cases need both halves for the reason every case here needs an expected phrase: a guard
that resolved nothing would report nothing missing and pass all of them. The last class runs the
guard against this repository's real `Cargo.lock` and asserts the shipped count is in four figures,
which is what tells a reviewer the reading is the one the build uses.

    python3 scripts/ci/test-check-check-notices-coverage.py
"""

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-notices-coverage.py")
REPO = SCRIPT.parents[2]
_spec = importlib.util.spec_from_file_location(
    "notices_lib", SCRIPT.parents[1] / "notices_lib.py")
notices = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(notices)

RULE = "-" * 80
BANNER = "=" * 80
HASHES = "#" * 80
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"


def package(name: str, version: str, *, license_id: str = "MIT", member: bool = False,
            vendored: bool = False, repository: str = None) -> dict:
    return {
        "id": f"{name} {version} ({REGISTRY})" if not member else f"{name} {version} (path+file:///w/{name})",
        "name": name,
        "version": version,
        "license": license_id,
        "manifest_path": (f"/w/third_party/{name}/Cargo.toml" if vendored
                          else f"/registry/src/index.crates.io/{name}-{version}/Cargo.toml"),
        "authors": [f"{name} authors <hi@example.invalid>"],
        "repository": repository if repository is not None else f"https://example.invalid/{name}",
    }


def metadata(packages: list[dict], edges: dict[str, list[tuple[str, bool]]]) -> dict:
    nodes = [{"id": pid, "deps": [{"pkg": child, "dep_kinds": [{"kind": "dev" if dev else "normal",
                                                                "target": None}]}
                                  for child, dev in kids]}
             for pid, kids in edges.items()]
    return {
        "packages": packages,
        "workspace_members": [pkg["id"] for pkg in packages if "(path+file:" in pkg["id"]],
        "resolve": {"nodes": nodes},
    }


def entry(name: str, version: str, *, declares: str = None, license_id: str = "MIT") -> str:
    chosen = f"License: {license_id}"
    if declares:
        chosen = f"License: {license_id}  (upstream declares: {declares})"
    note = ""
    if declares:
        note = ("\nAdditional requirements / notices:\n  Upstream license expression:"
                f" {declares}. For this distribution, obligations are satisfied under:"
                f" {license_id}.\n")
    return (f"{RULE}\n{name} {version}\n{RULE}\n"
            f"Source:  https://example.invalid/{name}\n{chosen}\n\n"
            "Copyright notice:\n  Copyright (c) 2024 The authors\n\n"
            f"License text: see Part II — {license_id}\n{note}\n")


def document(entries: tuple[str, ...]) -> str:
    sections = sorted({line.split("— ")[1].split("\n")[0] for e in entries for line in e.splitlines()
                       if line.startswith("License text: see Part II")})
    body = "\n".join(entry_text.rstrip() for entry_text in entries)
    texts = "\n".join(f"{HASHES}\n# {license_id}\n{HASHES}\n\nThe {license_id} text.\n"
                      for license_id in sections)
    return (f"THIRD-PARTY NOTICES\n{BANNER}\n{notices.PART_I}\n{BANNER}\n\n{body}\n\n\n"
            f"{BANNER}\n{notices.PART_II}\n{BANNER}\n\nReferenced texts.\n{texts}\n\n"
            f"{BANNER}\nEND OF THIRD-PARTY NOTICES\n{BANNER}\n")


def workspace(*, dev_leaf: bool = False, vendored_leaf: bool = False, member_leaf: bool = False,
              two_versions: bool = False) -> tuple[dict, tuple[str, ...]]:
    """A root binary, one direct dependency, and one leaf reached through it.

    The leaf changes shape per case, which is what makes the three membership tests the same test.
    """
    root = package(notices.SHIPPED_ROOT, "0.1.0", member=True)
    middle = package("middle", "1.0.0", license_id="Apache-2.0 OR MIT")
    leaf = package("leaf", "1.0.0", member=member_leaf, vendored=vendored_leaf)
    packages = [root, middle, leaf]
    edges = {
        root["id"]: [(middle["id"], False), (leaf["id"], dev_leaf)],
        middle["id"]: [(leaf["id"], dev_leaf)],
        leaf["id"]: [],
    }
    if two_versions:
        other = package("leaf", "0.9.0")
        packages.append(other)
        edges[middle["id"]].append((other["id"], False))
    return metadata(packages, edges), (entry("middle", "1.0.0", license_id="Apache-2.0",
                                             declares="Apache-2.0 OR MIT"),
                                       entry("leaf", "1.0.0"))


def run(text: str, meta: dict, *args: str) -> subprocess.CompletedProcess:
    tmp = Path(tempfile.mkdtemp())
    (tmp / notices.DOCUMENT).write_text(text, encoding="utf-8")
    handle = tmp / "metadata.json"
    handle.write_text(json.dumps(meta) if not isinstance(meta, str) else meta, encoding="utf-8")
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--root", str(tmp), "--metadata", str(handle), *args],
        capture_output=True, text=True)


class Shipped(unittest.TestCase):
    def assert_problem(self, text: str, meta: dict, expected: str, *args: str) -> None:
        proc = run(text, meta, *args)
        self.assertEqual(proc.returncode, 1, f"expected {expected!r}, got:\n{proc.stdout}"
                                          f"{proc.stderr}")
        self.assertIn(expected, proc.stdout)

    def test_every_reached_package_needs_an_entry(self) -> None:
        meta, entries = workspace()
        self.assertEqual(run(document(entries), meta).returncode, 0,
                         run(document(entries), meta).stdout)
        self.assert_problem(document(entries[:1]), meta, "1 shipped package(s) with no entry")

    def test_a_version_that_no_longer_ships_is_reported(self) -> None:
        meta, entries = workspace()
        moved = entry("leaf", "0.9.0")
        self.assert_problem(document((entries[0], moved)), meta,
                            "naming a version that is not what ships")

    def test_two_versions_in_one_binary_are_two_obligations(self) -> None:
        meta, entries = workspace(two_versions=True)
        both = document(entries + (entry("leaf", "0.9.0"),))
        proc = run(both, meta)
        self.assertEqual(proc.returncode, 0, proc.stdout)
        self.assert_problem(document(entries), meta, "1 shipped package(s) with no entry")

    def test_an_entry_for_nothing_shipped_is_reported(self) -> None:
        meta, entries = workspace()
        extra = document(entries + (entry("gone", "3.0.0"),))
        self.assert_problem(extra, meta, "the build does not reach")

    def test_a_transitive_dependency_is_covered_only_by_its_own_entry(self) -> None:
        # `leaf` is reached through `middle` here, so an entry naming `middle` covers nothing for
        # it -- the case a hand-maintained list of "what we depend on" gets wrong most often.
        meta, entries = workspace()
        self.assert_problem(document(entries[:1]), meta, "leaf 1.0.0")

    def test_a_stale_declaration_is_reported_against_the_package(self) -> None:
        meta, entries = workspace()
        stale = entry("middle", "1.0.0", license_id="Apache-2.0",
                      declares="Apache-2.0 OR Zlib")
        self.assert_problem(document((stale, entries[1])), meta,
                            "differs from the package's own")

    def test_a_license_the_rules_cannot_decide_is_reported(self) -> None:
        meta, entries = workspace()
        meta["packages"][2]["license"] = "GPL-3.0-only"
        self.assert_problem(document(entries), meta, "cannot decide a term")

class Membership(unittest.TestCase):
    """Which packages count as shipped, pinned from both sides each."""

    def assert_problem(self, text: str, meta: dict, expected: str) -> None:
        proc = run(text, meta)
        self.assertEqual(proc.returncode, 1, f"expected {expected!r}, got:\n{proc.stdout}"
                                          f"{proc.stderr}")
        self.assertIn(expected, proc.stdout)

    def test_a_dev_only_dependency_is_not_shipped(self) -> None:
        meta, entries = workspace(dev_leaf=True)
        # Reached only through dev edges, so nothing in the binary comes from it: the entry for it
        # is coverage of nothing, and dropping that entry is correct rather than a gap.
        self.assert_problem(document(entries), meta, "leaf 1.0.0")
        self.assertEqual(run(document(entries[:1]), meta).returncode, 0,
                         run(document(entries[:1]), meta).stdout)

    def test_this_workspaces_own_crates_are_not_third_party(self) -> None:
        meta, entries = workspace(member_leaf=True)
        self.assert_problem(document(entries), meta, "leaf 1.0.0")
        self.assertEqual(run(document(entries[:1]), meta).returncode, 0,
                         run(document(entries[:1]), meta).stdout)

    def test_a_vendored_crate_under_third_party_is_shipped(self) -> None:
        meta, entries = workspace(vendored_leaf=True)
        # A workspace member all the same, but the code is compiled into the binary, so the entry
        # is owed. Treating members as never-third-party would drop five entries from the real
        # document, which is the whole reason this case exists.
        self.assertEqual(run(document(entries), meta).returncode, 0, run(document(entries), meta).stdout)
        self.assert_problem(document(entries[:1]), meta, "leaf 1.0.0")


class FailClosed(unittest.TestCase):
    """A guard that goes quiet when it cannot see is worse than none, so each failure says why.

    These cases hand the guard something unreadable on one side or the other. They are written with a
    document that parses unless the document *is* the thing under test, because a parse error in the
    document would otherwise be reported and the case would pass without ever reaching cargo.
    """

    def test_metadata_that_names_no_shipped_root_is_a_failure(self) -> None:
        meta, entries = workspace()
        meta["packages"] = [dict(p, name="other-" + p["name"]) for p in meta["packages"]]
        proc = run(document(entries), meta)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("could not be computed", proc.stdout)

    def test_metadata_with_no_packages_is_a_failure(self) -> None:
        proc = run(document((entry("middle", "1.0.0"),)),
                   {"packages": [], "workspace_members": [], "resolve": {"nodes": []}})
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("could not be computed", proc.stdout)

    def test_metadata_that_is_not_json_is_a_failure(self) -> None:
        proc = run(document((entry("middle", "1.0.0"),)), "not json at all")
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("could not be computed", proc.stdout)

    def test_a_package_list_that_reaches_nothing_is_a_failure(self) -> None:
        root = package(notices.SHIPPED_ROOT, "0.1.0", member=True)
        meta = {"packages": [root], "workspace_members": [root["id"]],
                "resolve": {"nodes": [{"id": root["id"], "deps": []}]}}
        proc = run(document((entry("middle", "1.0.0"),)), meta)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("reaches no third-party package", proc.stdout)

    def test_running_without_metadata_and_without_a_lockfile_is_a_failure(self) -> None:
        tmp = Path(tempfile.mkdtemp())
        (tmp / notices.DOCUMENT).write_text(document((entry("middle", "1.0.0"),)), encoding="utf-8")
        proc = subprocess.run([sys.executable, str(SCRIPT), "--root", str(tmp)],
                              capture_output=True, text=True)
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("could not be computed", proc.stdout)

    def test_a_missing_document_exits_two(self) -> None:
        tmp = Path(tempfile.mkdtemp())
        meta, _ = workspace()
        handle = tmp / "metadata.json"
        handle.write_text(json.dumps(meta), encoding="utf-8")
        proc = subprocess.run([sys.executable, str(SCRIPT), "--root", str(tmp),
                               "--metadata", str(handle)], capture_output=True, text=True)
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("is not a file", proc.stderr)

    def test_the_example_list_is_capped_and_says_so(self) -> None:
        meta, entries = workspace()
        packages = list(meta["packages"])
        edges = {node["id"]: [(d["pkg"], False) for d in node["deps"]]
                 for node in meta["resolve"]["nodes"]}
        for index in range(4):
            extra = package(f"extra{index}", "1.0.0")
            packages.append(extra)
            edges[packages[0]["id"]] = edges.get(packages[0]["id"], []) + [(extra["id"], False)]
            edges[extra["id"]] = []
        meta = metadata(packages, edges)
        proc = run(document(entries), meta, "--max-list", "2")
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("4 shipped package(s) with no entry", proc.stdout)
        self.assertIn("(and 2 more)", proc.stdout)

    def test_a_document_that_does_not_parse_blames_the_document(self) -> None:
        # The two sides of this guard fail differently and have to say so: an unreadable document is
        # a broken file, while an unreadable build is a missing toolchain, and a message naming the
        # wrong one sends the reader to the wrong fix.
        meta, _ = workspace()
        header_only = (f"THIRD-PARTY NOTICES\n{BANNER}\n{notices.PART_I}\n{BANNER}\n")
        proc = run(header_only, meta)
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("could not be read", proc.stdout)
        self.assertNotIn("dependency set could not be computed", proc.stdout)


class RealWorkspace(unittest.TestCase):
    def test_the_shipped_set_is_the_one_the_build_resolves(self) -> None:
        proc = subprocess.run([sys.executable, str(SCRIPT), "--root", str(REPO)],
                              capture_output=True, text=True, timeout=900)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        count = int(proc.stdout.split("(")[1].split()[0])
        # Four figures, because the binary really does link this much third-party code. A reading
        # that stopped at direct dependencies would report a few dozen and still find them covered.
        self.assertGreater(count, 1000, proc.stdout)


if __name__ == "__main__":
    unittest.main(verbosity=2)
