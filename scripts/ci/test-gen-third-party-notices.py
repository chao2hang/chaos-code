#!/usr/bin/env python3
"""Fixtures for `scripts/gen-third-party-notices.py`, the tool that rewrites a legal artifact.

The two `check-notices-*.py` guards say when `THIRD-PARTY-NOTICES` is wrong; this script is what makes
it right again, on a 19 000-line file nobody is going to repair by hand. That makes it the most
dangerous script in this directory: it holds the only write handle on a document whose hand written
parts -- verbatim upstream `NOTICE` files, the `VENDORED WITH LOCAL MODIFICATIONS:` blocks that explain
what this repository changed inside `third_party/` -- must survive it. So the cases here are not about
whether an entry appears, which the coverage guard already proves, but about what happens to the bytes
around it:

- a surviving entry comes back byte for byte, prose and all, and the only thing removed is the entry
  for a package the build no longer reaches -- together with a Part II text no other entry asks for,
  which is why dropping one entry can legitimately shrink the file's second half;
- a text kept alive by a prose mention rather than a pointer is not collateral damage of that rule;
- a package's own license file is what lands in Part II and the copyright line inside it is what lands
  in the entry, because that is the attribution the author wrote;
- a run writes nothing when there is nothing to change, and refuses -- leaving the file untouched --
  when it meets a license this project's rules cannot judge.

Every case drives the real command line against a scratch repository shape and the saved-JSON seam
`--metadata` provides, the same one `check-notices-coverage.py` uses, so no case needs a toolchain, a
registry, or the network. Each document carries one entry that is already correct, both because the
reader refuses a Part I it cannot parse and because that entry is what "nothing else moved" is measured
against.

    python3 scripts/ci/test-gen-third-party-notices.py
"""

import importlib.util
import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "gen-third-party-notices.py"
_spec = importlib.util.spec_from_file_location(
    "notices_lib", SCRIPT.parent / "notices_lib.py")
notices = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(notices)

RULE = "-" * 80
BANNER = "=" * 80
HASHES = "#" * 80
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
MIT_TEXT = "The MIT text."
ZLIB_TEXT = ("Permission to use, copy, modify, and/or distribute this software for any purpose\n"
             "with or without fee is hereby granted.")
GPL_TEXT = ("                    GNU GENERAL PUBLIC LICENSE\n"
            "                       Version 2, June 1991")


def repository() -> Path:
    """A directory that looks like the repository root to the generator."""
    tmp = Path(tempfile.mkdtemp())
    (tmp / "Cargo.lock").write_text("# fake lockfile\n", encoding="utf-8")
    return tmp


def package(root: Path, name: str, version: str, *, license_id: str = "MIT",
            member: bool = False, vendored: bool = False,
            files: dict[str, str] | None = None) -> dict:
    """One `cargo metadata` package, with its real manifest (and any license files) on disk.

    The manifest path is what decides membership and vendoring, so it is written rather than invented:
    a vendored crate has to sit under `/third_party/` in a directory that exists, because that is the
    only way the generator can read the license text the package itself ships.
    """
    if vendored:
        manifest = root / "third_party" / name / "Cargo.toml"
    elif member:
        manifest = root / "crates" / name / "Cargo.toml"
    else:
        # A crates.io path that is deliberately not on disk: the registry is not fetched here, and
        # "the source is not available" is one of the cases the generator has to handle.
        manifest = Path(f"{root}/registry/{name}-{version}/Cargo.toml")
    if member or vendored:
        manifest.parent.mkdir(parents=True, exist_ok=True)
        manifest.write_text(f'[package]\nname = "{name}"\nversion = "{version}"\n',
                            encoding="utf-8")
        for filename, text in (files or {}).items():
            (manifest.parent / filename).write_text(text, encoding="utf-8")
    source = f"path+file://{manifest}" if member or vendored else REGISTRY
    return {
        "id": f"{name} {version} ({source})",
        "name": name,
        "version": version,
        "license": license_id,
        "manifest_path": str(manifest),
        "authors": [f"{name} authors <hi@example.invalid>"],
        "repository": f"https://example.invalid/{name}",
    }


def metadata(packages: list[dict], edges: dict[str, list[tuple[str, bool]]]) -> dict:
    nodes = [{"id": pid,
              "deps": [{"pkg": child,
                        "dep_kinds": [{"kind": "dev" if dev else "normal", "target": None}]}
                       for child, dev in kids]}
             for pid, kids in edges.items()]
    members = [pkg["id"] for pkg in packages if "path+file:" in pkg["id"]]
    return {"packages": packages, "workspace_members": members, "resolve": {"nodes": nodes}}


def entry(name: str, version: str, *, points: str = "MIT", declares: str = None,
          extra: str = "") -> str:
    """A Part I entry in the shape the committed document uses."""
    license_line = f"License: {points}"
    note = ""
    if declares:
        license_line = f"License: {points}  (upstream declares: {declares})"
        note = ("\nAdditional requirements / notices:\n  Upstream license expression:"
                f" {declares}. For this distribution, obligations are satisfied under: {points}.\n")
    tail = f"\n{extra.strip()}\n" if extra else ""
    return (f"{RULE}\n{name} {version}\n{RULE}\n"
            f"Source:  https://example.invalid/{name}\n{license_line}\n\n"
            "Copyright notice:\n  Copyright (c) 2020 The authors\n\n"
            f"License text: see Part II — {points}\n{note}{tail}")


def section(license_id: str, text: str) -> str:
    return f"{HASHES}\n# {license_id}\n{HASHES}\n\n{text.strip()}\n"


def document(entries: tuple[str, ...], sections: tuple[str, ...]) -> str:
    body = "\n".join(text.rstrip() for text in entries)
    texts = "\n\n".join(sections)
    return (f"THIRD-PARTY NOTICES\n{BANNER}\n{notices.PART_I}\n{BANNER}\n\n"
            f"{body}\n\n\n"
            f"{BANNER}\n{notices.PART_II}\n{BANNER}\n\nReferenced texts follow.\n"
            f"{texts}\n\n"
            f"{BANNER}\nEND OF THIRD-PARTY NOTICES\n{BANNER}\n")


def build(root: Path, *, shipped: list[dict], dev: tuple[str, ...] = (),
          entries: tuple[str, ...] = (), sections: tuple[str, ...] = ()) -> None:
    """Write the document and the `cargo metadata` the generator will be pointed at.

    `shipped` hangs off the binary the release builds; anything named in `dev` is reached only through
    a test dependency, which is the difference between owing a notice and not.
    """
    root_pkg = package(root, notices.SHIPPED_ROOT, "0.1.0", member=True)
    edges = {root_pkg["id"]: [(dep["id"], dep["name"] in dev) for dep in shipped]}
    for dep in shipped:
        edges.setdefault(dep["id"], [])
    meta = metadata([root_pkg, *shipped], edges)
    (root / "metadata.json").write_text(json.dumps(meta), encoding="utf-8")
    write_document(root, entries, sections)


def write_document(root: Path, entries: tuple[str, ...], sections: tuple[str, ...]) -> None:
    (root / notices.DOCUMENT).write_text(document(entries, sections), encoding="utf-8")


def generate(root: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--repo", str(root),
         "--metadata", str(root / "metadata.json"), *args],
        capture_output=True, text=True)


def read(root: Path) -> object:
    return notices.Document((root / notices.DOCUMENT).read_text(encoding="utf-8"))


def text_of(root: Path) -> str:
    return (root / notices.DOCUMENT).read_text(encoding="utf-8")


def keys(root: Path) -> list[tuple[str, str]]:
    return sorted(entry.key for entry in read(root).entries)


class Rewrites(unittest.TestCase):
    def test_a_missing_entry_is_written_and_a_stale_one_dropped(self) -> None:
        root = repository()
        anchor = package(root, "anchor", "9.9.9")
        middle = package(root, "middle", "1.0.0", license_id="Apache-2.0 OR MIT")
        build(root, shipped=[anchor, middle],
              entries=(entry("anchor", "9.9.9"), entry("gone", "3.0.0")),
              sections=(section("MIT", MIT_TEXT),))
        proc = generate(root, "--write")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertEqual(keys(root), [("anchor", "9.9.9"), ("middle", "1.0.0")],
                         "the entry for a package nothing ships goes and the missing one is written")
        written = read(root).by_key()[("middle", "1.0.0")]
        self.assertIn("Source:  https://example.invalid/middle", written.body)
        self.assertIn("License: MIT  (upstream declares: Apache-2.0 OR MIT)", written.body)
        self.assertIn("obligations are satisfied under: MIT", written.body)
        self.assertTrue(written.has_copyright())
        self.assertEqual(written.references(), ["MIT"])

    def test_a_surviving_entry_keeps_its_bytes(self) -> None:
        root = repository()
        helper = package(root, "helper", "1.0.0", vendored=True)
        hand_written = entry(
            "helper", "1.0.0",
            extra="VENDORED WITH LOCAL MODIFICATIONS:\n"
                  "  We removed the telemetry call from `src/lib.rs` and kept the rest of the\n"
                  "  upstream file tree exactly as published.\n\n"
                  "Upstream NOTICE file, reproduced verbatim:\n"
                  "  This product includes software developed by Nobody.")
        build(root, shipped=[helper], entries=(hand_written,),
              sections=(section("MIT", MIT_TEXT),))
        before = read(root).by_key()[("helper", "1.0.0")].body
        proc = generate(root, "--write")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertEqual(read(root).by_key()[("helper", "1.0.0")].body, before,
                         "a current entry has to be spliced through untouched, prose included")
        self.assertIn("VENDORED WITH LOCAL MODIFICATIONS:", text_of(root))
        self.assertIn("This product includes software developed by Nobody.", text_of(root))

    def test_report_mode_describes_the_change_and_writes_nothing(self) -> None:
        root = repository()
        anchor = package(root, "anchor", "9.9.9")
        plain = package(root, "plain", "2.1.0")
        build(root, shipped=[anchor, plain], entries=(entry("anchor", "9.9.9"),),
              sections=(section("MIT", MIT_TEXT),))
        untouched = text_of(root)
        proc = generate(root)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn("entries to write: 1", proc.stdout)
        self.assertIn("no file written", proc.stdout)
        self.assertEqual(text_of(root), untouched)

    def test_running_the_writer_twice_changes_nothing(self) -> None:
        root = repository()
        anchor = package(root, "anchor", "9.9.9")
        middle = package(root, "middle", "1.0.0", license_id="Apache-2.0 OR MIT")
        build(root, shipped=[anchor, middle], entries=(entry("anchor", "9.9.9"),),
              sections=(section("MIT", MIT_TEXT),))
        first = generate(root, "--write")
        self.assertEqual(first.returncode, 0, first.stdout + first.stderr)
        self.assertEqual(keys(root), [("anchor", "9.9.9"), ("middle", "1.0.0")])
        written = (root / notices.DOCUMENT).read_bytes()
        second = generate(root, "--write")
        self.assertEqual(second.returncode, 0, second.stdout + second.stderr)
        self.assertIn("already matches what is shipped", second.stdout)
        self.assertEqual((root / notices.DOCUMENT).read_bytes(), written,
                         "a second run must be byte-identical, or a dependency bump would produce "
                         "churn on top of the notices diff")

    def test_a_handed_in_license_text_is_used_verbatim(self) -> None:
        root = repository()
        anchor = package(root, "anchor", "9.9.9")
        plain = package(root, "plain", "1.0.0", license_id="ISC")
        build(root, shipped=[anchor, plain], entries=(entry("anchor", "9.9.9"),),
              sections=(section("MIT", MIT_TEXT),))
        handed = root / "isc.txt"
        handed.write_text("Permission to use, copy, modify and distribute this software is hereby\n"
                          "granted, provided this notice is preserved.\n", encoding="utf-8")
        without = generate(root, "--write")
        self.assertEqual(without.returncode, 0, without.stdout + without.stderr)
        self.assertIn("! Part II ISC: no package ships an unambiguous copy of the text",
                      without.stdout)
        doc = read(root)
        self.assertNotIn("ISC", doc.license_sections,
                         "with no text it can trust, the generator must not invent one")
        self.assertIn("see upstream https://example.invalid/plain (license: ISC)",
                      doc.by_key()[("plain", "1.0.0")].body)
        write_document(root, (entry("anchor", "9.9.9"),), (section("MIT", MIT_TEXT),))
        with_flag = generate(root, "--write", "--license-text", f"ISC={handed}")
        self.assertEqual(with_flag.returncode, 0, with_flag.stdout + with_flag.stderr)
        doc = read(root)
        self.assertEqual(doc.license_sections["ISC"], handed.read_text(encoding="utf-8").strip())
        self.assertEqual(doc.by_key()[("plain", "1.0.0")].references(), ["ISC"])

    def test_the_line_count_it_prints_is_the_one_wc_l_reports(self) -> None:
        root = repository()
        anchor = package(root, "anchor", "9.9.9")
        plain = package(root, "plain", "1.0.0", license_id="ISC")
        build(root, shipped=[anchor, plain], entries=(entry("anchor", "9.9.9"),),
              sections=(section("MIT", MIT_TEXT),))
        # The GNU GPL texts are laid out for a printer and carry a form feed per page; a count
        # taken with str.splitlines() treats those as line breaks and reports more lines than
        # the file has, which is exactly the kind of number this tool prints and a reviewer
        # repeats.
        handed = root / "isc.txt"
        handed.write_text("first page line\n\x0csecond page line\n", encoding="utf-8")
        proc = generate(root, "--write", "--license-text", f"ISC={handed}")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        written = text_of(root)
        reported = re.search(r"wrote \S+: (\d+) lines", proc.stdout)
        self.assertIsNotNone(reported, proc.stdout)
        self.assertEqual(int(reported.group(1)), written.count("\n"),
                         "the printed line count has to be what wc -l reports for the same file")
        self.assertIn("\x0c", written, "the form feed has to still be in the file for this to mean anything")

    def test_a_copyright_line_split_by_a_form_feed_stays_one_line(self) -> None:
        root = repository()
        anchor = package(root, "anchor", "9.9.9")
        # License texts laid out for a printer carry a form feed where a page ended, sometimes in
        # the middle of an attribution line. Cutting the file at anything other than a newline
        # would then record half of the copyright notice and drop the rest.
        license_file = "Copyright 2021 The\x0cWidget Authors\n\n" + ZLIB_TEXT + "\n"
        helper = package(root, "helper", "1.0.0", license_id="Zlib", vendored=True,
                         files={"LICENSE": license_file})
        build(root, shipped=[anchor, helper], entries=(entry("anchor", "9.9.9"),),
              sections=(section("MIT", MIT_TEXT),))
        proc = generate(root, "--write")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        body = read(root).by_key()[("helper", "1.0.0")].body
        self.assertIn("  Copyright 2021 The\x0cWidget Authors\n", body,
                      "the attribution has to survive whole, not as the half before the page break")

    def test_a_vendored_package_supplies_its_own_text_and_copyright(self) -> None:
        root = repository()
        anchor = package(root, "anchor", "9.9.9")
        license_file = f"Copyright 2021 The Helper Authors\n\n{ZLIB_TEXT}\n"
        helper = package(root, "helper", "1.0.0", license_id="Zlib", vendored=True,
                         files={"LICENSE": license_file})
        build(root, shipped=[anchor, helper], entries=(entry("anchor", "9.9.9"),),
              sections=(section("MIT", MIT_TEXT),))
        proc = generate(root, "--write")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        doc = read(root)
        self.assertEqual(doc.license_sections["Zlib"], license_file.strip(),
                         "the text in Part II has to be the one the package ships, not a paraphrase")
        body = doc.by_key()[("helper", "1.0.0")].body
        self.assertIn("Copyright 2021 The Helper Authors", body,
                      "the attribution the author wrote outranks the metadata's author list")
        self.assertEqual(doc.by_key()[("helper", "1.0.0")].references(), ["Zlib"])


class PartIIReachability(unittest.TestCase):
    """Dropping an entry may drop a license text with it; it may never drop one still named."""

    def test_a_text_only_the_dropped_entry_asked_for_goes_with_it(self) -> None:
        root = repository()
        middle = package(root, "middle", "1.0.0")
        mocky = package(root, "mocky", "1.0.0", license_id="WTFPL")
        build(root, shipped=[middle, mocky], dev=("mocky",),
              entries=(entry("middle", "1.0.0"), entry("mocky", "1.0.0", points="WTFPL")),
              sections=(section("MIT", MIT_TEXT), section("WTFPL", "Do what thou wilt.")))
        proc = generate(root, "--write")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn("- Part II WTFPL", proc.stdout)
        doc = read(root)
        self.assertEqual(sorted(entry.key for entry in doc.entries), [("middle", "1.0.0")])
        self.assertEqual(list(doc.license_sections), ["MIT"],
                         "a text no entry asks for describes nothing this binary contains")
        self.assertNotIn("Do what thou wilt.", text_of(root))
        self.assertIn(MIT_TEXT, text_of(root))

    def test_a_text_named_in_prose_survives_without_a_pointer(self) -> None:
        root = repository()
        git = package(root, "git", "1.0.0")
        notes = entry("git", "1.0.0", extra="Notes:\n"
                                            "  Full upstream COPYING is in Part II under GPL-2.0-only.")
        build(root, shipped=[git], entries=(notes,),
              sections=(section("MIT", MIT_TEXT), section("GPL-2.0-only", GPL_TEXT)))
        before = read(root)
        pointed = {section for entry in before.entries for section in entry.references()}
        self.assertNotIn("GPL-2.0-only", pointed,
                         "this case is about a text nothing points at; the other case covers pointers")
        self.assertEqual(notices.unreachable_sections(before), [],
                         "the mention in the entry's own notes is what keeps it reachable")
        proc = generate(root, "--write")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        doc = read(root)
        self.assertIn("GPL-2.0-only", doc.license_sections,
                      "an entry describes this text in words, which is a reason to keep it")
        self.assertEqual(doc.license_sections["GPL-2.0-only"], GPL_TEXT.strip())
        self.assertEqual(notices.unreachable_sections(doc), [])


class Refusals(unittest.TestCase):
    def assert_refused_unchanged(self, root: Path, expected: str, *args: str) -> None:
        before = (root / notices.DOCUMENT).read_bytes()
        proc = generate(root, *args)
        self.assertEqual(proc.returncode, 2, f"expected a refusal, got:\n{proc.stdout}{proc.stderr}")
        self.assertIn(expected, proc.stdout + proc.stderr)
        self.assertEqual((root / notices.DOCUMENT).read_bytes(), before,
                         "a refusal has to leave the document exactly as it found it")

    def test_a_new_license_outside_the_allowed_set_stops_the_write(self) -> None:
        root = repository()
        anchor = package(root, "anchor", "9.9.9")
        odd = package(root, "odd", "1.0.0", license_id="GPL-3.0-only")
        build(root, shipped=[anchor, odd], entries=(entry("anchor", "9.9.9"),),
              sections=(section("MIT", MIT_TEXT),))
        self.assert_refused_unchanged(root, "is not in ALLOWED_LICENSES", "--write")

    def test_a_shipped_license_the_rules_cannot_judge_explains_the_fix(self) -> None:
        # Same license, but this one already has an entry, so nothing has to be rendered and the run
        # gets as far as the triage. The answer is not "pick something": it is "record the license".
        root = repository()
        odd = package(root, "odd", "1.0.0", license_id="GPL-3.0-only")
        build(root, shipped=[odd], entries=(entry("odd", "1.0.0"),),
              sections=(section("MIT", MIT_TEXT),))
        self.assert_refused_unchanged(root, "record the license in scripts/notices_lib.py",
                                      "--write")

    def test_metadata_that_is_not_json_is_refused(self) -> None:
        root = repository()
        build(root, shipped=[package(root, "anchor", "9.9.9")],
              entries=(entry("anchor", "9.9.9"),), sections=(section("MIT", MIT_TEXT),))
        (root / "metadata.json").write_text("error: no such command", encoding="utf-8")
        self.assert_refused_unchanged(root, "is not JSON", "--write")

    def test_a_document_in_an_unknown_shape_is_refused(self) -> None:
        root = repository()
        build(root, shipped=[package(root, "anchor", "9.9.9")])
        (root / notices.DOCUMENT).write_text("Third party notices.\n\nsee the website\n",
                                             encoding="utf-8")
        self.assert_refused_unchanged(root, "refuses to guess", "--write")

    def test_a_missing_document_exits_two(self) -> None:
        root = repository()
        build(root, shipped=[package(root, "anchor", "9.9.9")])
        (root / notices.DOCUMENT).unlink()
        proc = generate(root, "--write")
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("is not a file", proc.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
