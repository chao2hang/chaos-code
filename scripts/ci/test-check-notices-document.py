#!/usr/bin/env python3
"""Fixtures for `check-notices-document.py`.

The guard reads a document nobody reads, so these tests have to pin the reading itself: a checker
that found nothing at all would be green on every case here. Two things stop that. Every negative
case names the problem it expects in the guard's own words, so a guard that stopped parsing reports
zero problems and fails the case. And the last class runs the guard on this repository's real
document, asserts it is clean, and asserts the counts it reports are the counts the file really
holds -- a reader that had silently lost Part I would report far fewer entries there before it
reported anything here.

The shapes under test are the ones the shipped document actually uses, taken from it: a single
license; a chosen license with the declaration in parentheses and the note that explains the
choice; a stacking expression with its applicable terms spelled out; a vendored component whose
license text is pointed at in prose rather than by an SPDX id; and an entry sent upstream for a
text this document does not reproduce. The rules that let those pass are as deliberate as the ones
that reject. An entry may record more of a declaration than this project needed to take, which is
what `encoding_rs` does today; recording less than the license rules would take is the one
direction that quietly removes a permission from a reader.

    python3 scripts/ci/test-check-notices-document.py
"""

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-notices-document.py")
REPO = SCRIPT.parents[2]
_spec = importlib.util.spec_from_file_location(
    "notices_lib", SCRIPT.parents[1] / "notices_lib.py")
notices = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(notices)

RULE = "-" * 80
BANNER = "=" * 80
HASHES = "#" * 80
MIT_TEXT = "Permission is hereby granted, free of charge, to any person obtaining a copy."
APACHE_TEXT = "Licensed under the Apache License, Version 2.0 (the License)."
ISC_TEXT = "Permission to use, copy, modify, and/or distribute this software is granted."


def entry(name: str, version: str, *, license_lines: tuple[str, ...], source: str | None = None,
          copyright_lines: tuple[str, ...] = ("  Copyright (c) 2024 The authors",),
          text_lines: tuple[str, ...] = ("License text: see Part II — MIT",),
          note: tuple[str, ...] = ()) -> str:
    """One Part I entry, assembled from the same pieces the real document assembles them from."""
    lines = [RULE, f"{name} {version}", RULE]
    if source is not None:
        lines.append(f"Source:  {source}")
    lines.extend(license_lines)
    lines.append("")
    if copyright_lines:
        lines.append("Copyright notice:")
        lines.extend(copyright_lines)
        lines.append("")
    lines.extend(text_lines)
    if note:
        lines.append("")
        lines.append("Additional requirements / notices:")
        lines.extend(note)
    return "\n".join(lines) + "\n\n"


def section(license_id: str, text: str = MIT_TEXT) -> str:
    return f"{HASHES}\n# {license_id}\n{HASHES}\n\n{text}\n"


def document(entries: tuple[str, ...], sections: tuple[str, ...], intro: str = "",
             head_extra: str = "", end_banner: bool = True) -> str:
    body = "\n".join(entries).rstrip() if entries else "nothing here that reads as an entry"
    part1 = f"THIRD-PARTY NOTICES\n{BANNER}\n{head_extra}{notices.PART_I}\n{BANNER}\n\n{body}\n\n\n"
    texts = "\n".join(sections).rstrip()
    part2 = f"{BANNER}\n{notices.PART_II}\n{BANNER}\n\n{intro}\n{texts}\n\n"
    tail = f"{BANNER}\nEND OF THIRD-PARTY NOTICES\n{BANNER}\n" if end_banner else ""
    return part1 + part2 + tail


def single() -> str:
    return entry("alpha", "1.0.0", license_lines=("License: MIT",),
                 source="https://example.invalid/alpha")


def chosen() -> str:
    """The shape the shipped document uses for a package that offered a choice."""
    return entry(
        "beta", "2.0.0",
        license_lines=("License: MIT  (upstream declares: Apache-2.0 OR MIT)",),
        source="https://example.invalid/beta",
        copyright_lines=("  Copyright holders / authors (from package metadata):",
                         "    - Beta Author <beta@example.invalid>"),
        note=("  Upstream license expression: Apache-2.0 OR MIT. For this distribution,"
              " obligations are satisfied under: MIT.",),
    )


def stacking() -> str:
    """Terms that stack: `License:` alone would not say which of the two inside apply."""
    return entry(
        "gamma", "0.1.0",
        license_lines=("License: (Apache-2.0 OR MIT) AND ISC",
                       "  (applicable terms: Apache-2.0, ISC)"),
        source="https://example.invalid/gamma",
        text_lines=("License text: see Part II — Apache-2.0; ISC",),
    )


def baseline_sections() -> tuple[str, ...]:
    return (section("MIT", MIT_TEXT), section("Apache-2.0", APACHE_TEXT), section("ISC", ISC_TEXT))


def baseline(**kwargs) -> str:
    kwargs.setdefault("entries", (single(), chosen(), stacking()))
    kwargs.setdefault("sections", baseline_sections())
    kwargs.setdefault("intro", "Original license texts referenced by Part I.")
    return document(**kwargs)


def run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, str(SCRIPT), *args], capture_output=True, text=True)


def check(text: str, *args: str) -> subprocess.CompletedProcess:
    """Run the guard over `text`, written out as a document on disk."""
    path = Path(tempfile.mkdtemp()) / notices.DOCUMENT
    path.write_text(text, encoding="utf-8")
    return run("--document", str(path), *args)


class Accepted(unittest.TestCase):
    """The shapes this project ships under, which the guard must not object to."""

    def assert_clean(self, text: str) -> None:
        proc = check(text)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)

    def test_the_three_shipped_shapes_are_accepted(self) -> None:
        proc = check(baseline())
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn("3 entries, 3 license texts", proc.stdout)

    def test_a_note_may_be_wrapped_across_lines(self) -> None:
        # The real document keeps the note on one long line. Wrapping it changes nothing about what
        # it claims, and a guard that cried "no note" here would teach people to distrust the note
        # check, which is the one that catches an unexplained choice.
        wrapped = chosen().replace(
            "  Upstream license expression: Apache-2.0 OR MIT. For this distribution, obligations"
            " are satisfied under: MIT.",
            "  Upstream license expression: Apache-2.0 OR MIT. For this distribution,\n"
            "  obligations are satisfied under: MIT.")
        self.assertIn("\n  obligations are satisfied under", wrapped)
        self.assert_clean(baseline(entries=(single(), wrapped, stacking())))

    def test_recording_the_whole_declaration_is_accepted(self) -> None:
        # What `encoding_rs` does: it names all three terms its declaration offers rather than the
        # one this project needed to take. A gate that pushed that down would be narrowing what a
        # reader may do with the package.
        wide = entry(
            "beta", "2.0.0",
            license_lines=("License: Apache-2.0 OR MIT", "  (applicable terms: Apache-2.0, MIT)"),
            source="https://example.invalid/beta",
            text_lines=("License text: see Part II — Apache-2.0; MIT",),
            note=("  Upstream license expression: Apache-2.0 OR MIT. For this distribution,"
                  " obligations are satisfied under: Apache-2.0, MIT.",),
        )
        self.assert_clean(baseline(entries=(single(), wide, stacking())))

    def test_a_pointer_upstream_is_accepted_with_a_url_and_a_known_id(self) -> None:
        # `borrow-or-share` is MIT-0 and `notify` is CC0-1.0: texts this document does not
        # reproduce, so the entry says where to read them instead.
        sent = entry("beta", "2.0.0", license_lines=("License: MIT-0",),
                     source="https://example.invalid/beta",
                     text_lines=("License text: see upstream https://example.invalid/beta"
                                 " (license: MIT-0)",))
        self.assert_clean(baseline(entries=(single(), sent, stacking())))

    def test_a_section_named_in_prose_counts_as_reachable(self) -> None:
        # The vendored-C-library case: the libgit2 entry says the text is in Part II under
        # GPL-2.0-only rather than carrying a `License text:` line that names it.
        text = baseline(intro="Original license texts referenced by Part I. The ISC text is also\n"
                              "reproduced for a vendored component, see Part II under ISC.")
        self.assert_clean(text)

    def test_two_versions_of_one_name_are_two_entries(self) -> None:
        later = entry("alpha", "0.10.0", license_lines=("License: MIT",),
                      source="https://example.invalid/alpha")
        earlier = entry("alpha", "0.9.0", license_lines=("License: MIT",),
                        source="https://example.invalid/alpha")
        self.assert_clean(baseline(entries=(earlier, later, single(), chosen(), stacking())))


class Refused(unittest.TestCase):
    """Every way the document can fail a reader, one case each."""

    def assert_problem(self, text: str, expected: str) -> None:
        proc = check(text)
        self.assertEqual(proc.returncode, 1, f"expected {expected!r}, got:\n{proc.stdout}")
        self.assertIn(expected, proc.stdout)

    def test_an_unreadable_document_is_a_failure_not_an_empty_success(self) -> None:
        self.assert_problem("just prose about licenses\n", "FAIL (1 problem(s))")

    def test_no_entries_is_a_failure(self) -> None:
        self.assert_problem(document((), baseline_sections()), "no readable")

    def test_no_sections_is_a_failure(self) -> None:
        self.assert_problem(document((single(),), ()), "no readable license sections")

    def test_a_part_ii_heading_earlier_than_part_i_is_a_failure(self) -> None:
        # Not a hypothetical: the reader splits the file at the first occurrence of each heading,
        # so one stray `PART II — LICENSE TEXTS` line in the preamble moves every license text
        # into Part I and every entry into Part II.
        self.assert_problem(baseline(head_extra=f"{notices.PART_II}\n"), "must come before")

    def test_a_missing_closing_banner_is_a_failure(self) -> None:
        self.assert_problem(baseline(end_banner=False), "END OF THIRD-PARTY NOTICES")

    def test_two_entries_for_one_package_version_are_a_failure(self) -> None:
        self.assert_problem(baseline(entries=(single(), single(), chosen(), stacking())),
                            "2 entries for one package version")

    def test_entries_must_be_in_name_and_version_order(self) -> None:
        self.assert_problem(baseline(entries=(single(), stacking(), chosen())),
                            "not in (name, version) order")

    def test_versions_sort_numerically_not_lexically(self) -> None:
        # `0.9.0` before `0.10.0` is correct. A string sort would call it backwards and then flag
        # every real document, because 103 names in this one carry two versions each.
        later = entry("alpha", "0.10.0", license_lines=("License: MIT",),
                      source="https://example.invalid/alpha")
        earlier = entry("alpha", "0.9.0", license_lines=("License: MIT",),
                        source="https://example.invalid/alpha")
        self.assert_problem(baseline(entries=(later, earlier, single(), chosen(), stacking())),
                            "not in (name, version) order")

    def test_a_source_line_is_required(self) -> None:
        no_source = entry("alpha", "1.0.0", license_lines=("License: MIT",))
        self.assert_problem(baseline(entries=(no_source, chosen(), stacking())), "no `Source:` line")

    def test_a_source_that_is_not_a_url_is_refused(self) -> None:
        odd = entry("alpha", "1.0.0", license_lines=("License: MIT",), source="example.invalid/a")
        self.assert_problem(baseline(entries=(odd, chosen(), stacking())),
                            "does not give a http(s) URL")

    def test_a_copyright_notice_is_required(self) -> None:
        silent = entry("alpha", "1.0.0", license_lines=("License: MIT",),
                       source="https://example.invalid/alpha", copyright_lines=())
        self.assert_problem(baseline(entries=(silent, chosen(), stacking())),
                            "no `Copyright notice:` block")

    def test_an_empty_copyright_block_is_refused(self) -> None:
        blank = entry("alpha", "1.0.0", license_lines=("License: MIT",),
                      source="https://example.invalid/alpha", copyright_lines=("   ",))
        self.assert_problem(baseline(entries=(blank, chosen(), stacking())), "records no notice")

    def test_a_bare_alternation_with_no_terms_is_unreadable(self) -> None:
        guess = entry("alpha", "1.0.0", license_lines=("License: MIT OR Apache-2.0",),
                      source="https://example.invalid/alpha")
        self.assert_problem(baseline(entries=(guess, chosen(), stacking())),
                            "names neither one license nor an expression")

    def test_a_term_outside_the_shipped_license_set_is_refused(self) -> None:
        copyleft = entry("alpha", "1.0.0", license_lines=("License: GPL-3.0-only",),
                         source="https://example.invalid/alpha",
                         text_lines=("License text: see Part II — MIT",))
        self.assert_problem(baseline(entries=(copyleft, chosen(), stacking())),
                            "is outside the license set")

    def test_a_term_the_declaration_does_not_offer_is_refused(self) -> None:
        wrong = entry("beta", "2.0.0",
                      license_lines=("License: Zlib  (upstream declares: Apache-2.0 OR MIT)",),
                      source="https://example.invalid/beta",
                      text_lines=("License text: see Part II — MIT",),
                      note=("  Upstream license expression: Apache-2.0 OR MIT. For this"
                            " distribution, obligations are satisfied under: Zlib.",))
        self.assert_problem(baseline(entries=(single(), wrong, stacking())), "does not offer")

    def test_recording_less_than_the_rules_would_take_is_refused(self) -> None:
        # This project takes MIT from `Apache-2.0 OR MIT`. Recording only Apache-2.0 is the
        # document taking a permission away from the reader.
        narrow = entry("beta", "2.0.0",
                       license_lines=("License: Apache-2.0"
                                      "  (upstream declares: Apache-2.0 OR MIT)",),
                       source="https://example.invalid/beta",
                       text_lines=("License text: see Part II — Apache-2.0",),
                       note=("  Upstream license expression: Apache-2.0 OR MIT. For this"
                             " distribution, obligations are satisfied under: Apache-2.0.",))
        self.assert_problem(baseline(entries=(single(), narrow, stacking())),
                            "to be among the recorded terms")

    def test_a_declaration_without_the_note_explaining_it_is_refused(self) -> None:
        unexplained = entry("beta", "2.0.0",
                            license_lines=("License: MIT"
                                           "  (upstream declares: Apache-2.0 OR MIT)",),
                            source="https://example.invalid/beta",
                            text_lines=("License text: see Part II — MIT",))
        self.assert_problem(baseline(entries=(single(), unexplained, stacking())),
                            "not what was chosen from it")

    def test_a_note_naming_a_different_expression_is_refused(self) -> None:
        lying = chosen().replace("Upstream license expression: Apache-2.0 OR MIT.",
                                 "Upstream license expression: Apache-2.0 OR Zlib.")
        self.assert_problem(baseline(entries=(single(), lying, stacking())),
                            "the note names expression")

    def test_a_note_naming_different_terms_is_refused(self) -> None:
        lying = chosen().replace("satisfied under: MIT.", "satisfied under: MIT, Zlib.")
        self.assert_problem(baseline(entries=(single(), lying, stacking())),
                            "the note says obligations are satisfied under")

    def test_an_unparsable_declaration_is_refused(self) -> None:
        broken = chosen().replace("Apache-2.0 OR MIT", "Apache-2.0 AND")
        self.assert_problem(baseline(entries=(single(), broken, stacking())), "cannot be read")

    def test_exactly_one_pointer_per_entry_is_required(self) -> None:
        twice = chosen().replace("License text: see Part II — MIT",
                                 "License text: see Part II — MIT\nLicense text: see Part II — ISC")
        self.assert_problem(baseline(entries=(single(), twice, stacking())),
                            "2 `License text:` lines")
        none = single().replace("License text: see Part II — MIT\n", "")
        self.assert_problem(baseline(entries=(none, chosen(), stacking())),
                            "0 `License text:` lines")

    def test_a_pointer_in_an_unknown_shape_is_refused(self) -> None:
        vague = single().replace("License text: see Part II — MIT", "License text: ask upstream")
        self.assert_problem(baseline(entries=(vague, chosen(), stacking())),
                            "is neither `see Part II")

    def test_a_pointer_at_an_absent_section_is_refused(self) -> None:
        # The defect with no symptom at all: the entry looks complete, and the text it promises is
        # simply not in the file the reader is holding.
        missing = stacking().replace("see Part II — Apache-2.0; ISC", "see Part II — Apache-2.0; Zlib")
        self.assert_problem(baseline(entries=(single(), chosen(), missing)),
                            "which the document does not contain")

    def test_an_empty_section_is_refused(self) -> None:
        # Not last: the reader takes everything after a section heading up to the next one, so the
        # final section's body always includes the closing banner.
        empty = (section("ISC", "   "), section("MIT", MIT_TEXT), section("Apache-2.0", APACHE_TEXT))
        self.assert_problem(baseline(sections=empty), "which is empty")

    def test_a_license_text_nobody_points_at_is_refused(self) -> None:
        extra = baseline_sections() + (section("BSD-2-Clause", "Redistribution is permitted."),)
        self.assert_problem(baseline(sections=extra), "is not pointed at by any entry")

    def test_an_unreachable_section_heading_is_reported_once(self) -> None:
        # A section whose banner no longer reads as a banner is not a section any more: the reader
        # that cannot find it must not also be told it is unreferenced.
        text = baseline().replace(section("ISC", ISC_TEXT), "ISC\n\n" + ISC_TEXT + "\n")
        proc = check(text)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertEqual(proc.stdout.count("ISC"), 1, proc.stdout)


class Invocation(unittest.TestCase):
    def test_a_missing_document_exits_two(self) -> None:
        tmp = Path(tempfile.mkdtemp())
        proc = run("--root", str(tmp))
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("is not a file", proc.stderr)

    def test_list_reports_counts_without_judging(self) -> None:
        proc = check(baseline(entries=(single(),)), "--list")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn("1 entries, 3 license sections", proc.stdout)


class RealDocument(unittest.TestCase):
    """The guard against the file it exists for, which is what keeps the fixtures honest."""

    def counts(self) -> list[int]:
        proc = run("--root", str(REPO), "--list")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        import re
        return [int(number) for number in re.findall(r"\d+", proc.stdout)]

    def test_the_shipped_document_is_clean(self) -> None:
        proc = run("--root", str(REPO))
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)

    def test_the_shipped_document_is_read_at_full_size(self) -> None:
        entries, sections, declared, pointers = self.counts()[:4]
        # Four figures: a reader that lost Part I, or that stopped at the first section banner,
        # would report far fewer and still call everything clean.
        self.assertGreater(entries, 1000, f"{entries} entries")
        self.assertGreater(sections, 10, f"{sections} sections")
        # Most crates declare a choice. Zero here would mean the note rule had stopped matching the
        # document's wording, which would also silence the check that a choice was explained.
        self.assertGreater(declared, 500, f"{declared} declared")
        self.assertGreater(pointers, 1000, f"{pointers} pointers")


if __name__ == "__main__":
    unittest.main(verbosity=2)
