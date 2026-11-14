#!/usr/bin/env python3
"""Fixtures for `check-doc-path-refs.py`.

Every case pins one decision the gate makes, and the ones that could be satisfied by a
checker that finds nothing carry a second half that has to fire. A test asserting "a link
inside backticks is not a link" passes just as happily when the scanner is broken and
sees no links at all, so each such case asserts both: quoted, it is silence; the same
target unquoted, one problem naming the right line.

The four exclusions are the whole design, so they are each pinned from both sides:

- prose is not a reference, but a backticked path in the same sentence is;
- a fenced block is not scanned, but the identical text outside one is;
- a pattern (`docs/*.md`, `docs/tutorial/01…09-*.md`, a trailing `scripts/ci/x-`) is not
  a path, but `docs/gone.md` in the same file is;
- a name that does not start at a tracked top-level entry is not a claim about this
  repository, while the same shape starting at one is.

Three cases exist because of bugs found while writing the gate. One pins line numbers
against fenced blocks above them: blanking a fenced line used to blank its newline too,
which folded lines together and reported `docs/release-process.md` 33 lines away from
where it is. One pins the same numbers against `\x0c` and U+2028, which `str.splitlines`
counts as line breaks and `grep -n` does not. One pins that a broken *link* cannot be
recorded away, which is what keeps the allowlist from becoming the escape hatch it was
written to discipline.

    python3 scripts/ci/test-check-doc-path-refs.py
"""

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-doc-path-refs.py")
REPO = SCRIPT.parents[2]
_spec = importlib.util.spec_from_file_location("check_doc_path_refs", SCRIPT)
guard = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(guard)


def git(root: Path, *args: str) -> None:
    env = {**os.environ, "GIT_AUTHOR_NAME": "f", "GIT_AUTHOR_EMAIL": "f@e.invalid",
           "GIT_COMMITTER_NAME": "f", "GIT_COMMITTER_EMAIL": "f@e.invalid"}
    proc = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True, env=env)
    assert proc.returncode == 0, proc.stderr


def write(root: Path, rel: str, text: str) -> Path:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


def build(root: Path) -> Path:
    """A repository-shaped fixture: real git index, docs, a nested crate README."""
    git(root, "init", "-q")
    write(root, "README.md", "root readme\n")
    write(root, "TODO.md", "- [x] 见 `README.md`。\n")
    write(root, "docs/guide.md", "a guide\n")
    write(root, "crates/codegen/demo/src/lib.rs", "fn main() {}\n")
    write(root, "crates/codegen/demo/README.md", "see [the guide](docs/guide.md)\n")
    write(root, "scripts/ci/thing.py", "print(1)\n")
    write(root, ".gitignore", "ignored/\n")
    git(root, "add", "-A")
    return root


def run(root: Path, *extra: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--root", str(root), *extra],
        capture_output=True, text=True,
    )


def scan_text(doc: str, text: str) -> tuple[list[tuple[str, str]], list[str]]:
    """Write one document into a fresh fixture and scan the whole tree."""
    with tempfile.TemporaryDirectory() as tmp:
        root = build(Path(tmp))
        write(root, doc, text)
        git(root, "add", "-A")
        return guard.scan(guard.Repo(root))


def mentions_of(text: str, doc: str = "docs/guide.md") -> list[str]:
    return [tok for tok, _ in scan_text(doc, text)[0]]


def problems_of(text: str, doc: str = "docs/guide.md") -> list[str]:
    return scan_text(doc, text)[1]


class Links(unittest.TestCase):
    def test_working_link_is_silent(self) -> None:
        self.assertEqual(problems_of("[guide](guide.md)\n"), [])

    def test_broken_link_reports_with_line_number(self) -> None:
        problems = problems_of("intro\n\nsee [gone](gone.md)\n")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("docs/guide.md:3", problems[0])
        self.assertIn("gone.md", problems[0])

    def test_link_resolves_relative_to_the_citing_file(self) -> None:
        # crates/codegen/demo/README.md links `docs/guide.md`, which exists next to that
        # README in the fixture. A root-anchored checker calls this broken.
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, "crates/codegen/demo/docs/guide.md", "crate-local guide\n")
            git(root, "add", "-A")
            self.assertEqual(guard.scan(guard.Repo(root))[1], [])

    def test_line_and_anchor_suffixes_are_stripped(self) -> None:
        self.assertEqual(problems_of("[a](guide.md#section) [b](../README.md:12)\n"), [])

    def test_url_anchor_and_absolute_are_not_repo_links(self) -> None:
        text = ("[a](https://example.com/x.md) [b](#heading) [c](/etc/passwd) "
                "[d](mailto:x@y.z) [e](~/x.md)\n")
        self.assertEqual(problems_of(text), [])

    def test_anchor_link_to_missing_file_is_not_reported(self) -> None:
        # `#heading` carries no file at all; reporting it would mean guessing.
        self.assertEqual(problems_of("see [x](#missing-anchor)\n"), [])

    def test_non_ascii_anchor_is_stripped(self) -> None:
        # `README.md#安装` names README.md with a Chinese heading. Requiring the fragment
        # to be ASCII made two real links in this repo look broken.
        self.assertEqual(problems_of("[安装](../README.md#安装)\n"), [])
        problems = problems_of("[安装](../README-missing.md#安装)\n")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("README-missing.md", problems[0])

    def test_line_range_suffix_is_stripped(self) -> None:
        self.assertEqual(problems_of("[a](../README.md:12)\n[b](../README.md:12-19)\n"), [])


class Mentions(unittest.TestCase):
    def test_backticked_path_is_checked(self) -> None:
        self.assertEqual(mentions_of("详见 `docs/missing.md`。\n"), ["docs/missing.md"])

    def test_prose_slash_is_not_a_reference(self) -> None:
        text = ("The I/O path on macOS/Windows covers go/no-go decisions and tok/s, "
                "see desktop/mobile layouts.\n")
        self.assertEqual(mentions_of(text), [])
        # ... and the scanner is not simply blind: the same file with a real reference.
        self.assertEqual(mentions_of(text + "See `docs/missing.md`.\n"), ["docs/missing.md"])

    def test_fenced_block_is_not_scanned(self) -> None:
        fenced = "```text\nbuild it: target/release/chaos, then `docs/missing.md`\n```\n"
        self.assertEqual(mentions_of(fenced), [])
        # Load-bearing half: identical text outside the fence does fire.
        self.assertEqual(mentions_of("`docs/missing.md`\n"), ["docs/missing.md"])

    def test_pattern_is_not_a_path(self) -> None:
        for tok in ("docs/*.md", "docs/tutorial/01…09-*.md", "docs/{name}.md",
                    "scripts/ci/release-integrity-", "changelogs/X.Y.Z.md",
                    "docs/missing?.md", "docs/[abc].md"):
            with self.subTest(tok=tok):
                self.assertEqual(mentions_of(f"see `{tok}`\n"), [])
        self.assertEqual(mentions_of("see `docs/missing.md`\n"), ["docs/missing.md"])

    def test_brace_set_of_files_expands(self) -> None:
        toks = mentions_of("see `crates/codegen/demo/src/{lib.rs,gone.rs}`\n")
        self.assertEqual(toks, ["crates/codegen/demo/src/gone.rs"])

    def test_brace_set_of_things_that_are_not_files_is_a_pattern(self) -> None:
        self.assertEqual(mentions_of("see `src/{lib.rs,TODO}`\n"), [])

    def test_non_path_span_is_not_a_reference(self) -> None:
        for tok in ("cargo test -p demo --test flow", "0.4.0", "CHAOS_WEB_TOKEN",
                    "session/new", "src/", "just-a-name"):
            with self.subTest(tok=tok):
                self.assertEqual(mentions_of(f"run `{tok}`\n"), [])

    def test_bare_directory_without_slash_is_not_claimed(self) -> None:
        # a branch name and a directory are indistinguishable without another source
        self.assertEqual(mentions_of("on branch `sync/curated-port-20260918`\n"), [])


class RepositoryAnchoring(unittest.TestCase):
    def test_runtime_tree_is_not_a_claim_about_the_repo(self) -> None:
        # `.chaos` exists on disk in a developer checkout and owns no tracked file.
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, ".gitignore", "ignored/\n.chaos/\n")
            write(root, ".chaos/skills/x/SKILL.md", "local only\n")
            write(root, "docs/guide.md", "reads `./.chaos/skills/x/SKILL.md`\n")
            git(root, "add", "-A")
            mentions, problems = guard.scan(guard.Repo(root))
            self.assertEqual((mentions, problems), ([], []))
            # Load-bearing half: once the same top-level entry owns tracked content, the
            # identical shape is a claim about the repository and a bad one is caught.
            write(root, ".agents/notes.md", "tracked\n")
            write(root, "docs/guide.md", "reads `./.agents/gone.md`\n")
            git(root, "add", "-A")
            mentions, _ = guard.scan(guard.Repo(root))
        self.assertEqual([t for t, _ in mentions], ["./.agents/gone.md"])

    def test_untracked_top_level_name_is_not_a_claim(self) -> None:
        self.assertEqual(mentions_of("see `xai-grok-workspace/src/bin/gone.rs`\n"), [])
        # crate-relative shorthand starting at a real top level IS claimed and is caught
        self.assertEqual(mentions_of("see `scripts/gone.rs`\n"), ["scripts/gone.rs"])

    def test_ignored_file_on_disk_is_not_a_valid_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, "docs/ghost.md", "ignored, but present on disk\n")
            write(root, ".gitignore", "docs/ghost.md\n")
            write(root, "TODO.md", "see `docs/ghost.md`\n")
            git(root, "add", "-A")
            mentions, _ = guard.scan(guard.Repo(root))
        self.assertEqual([t for t, _ in mentions], ["docs/ghost.md"])

    def test_staged_but_uncommitted_file_is_a_valid_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, "docs/new.md", "not committed yet\n")
            write(root, "docs/guide.md", "see [new](new.md)\n")
            git(root, "add", "docs/new.md", "docs/guide.md")
            self.assertEqual(guard.scan(guard.Repo(root))[1], [])

    def test_line_numbers_survive_fenced_blocks(self) -> None:
        text = (
            "para one\n\n```bash\nmake\nmake install\n```\n\n"
            "para two\n\nsee [gone](gone.md)\n"
        )
        problems = problems_of(text)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("docs/guide.md:10", problems[0], "the link is on line 10")

    def test_masking_cannot_shift_line_numbers(self) -> None:
        text = "a\n```x\nb\nc\n```\nd\n"
        self.assertEqual(len(guard.lines_of(guard.strip_fences(text))),
                         len(guard.lines_of(text)))

    def test_line_numbers_match_grep_on_text_with_odd_breaks(self) -> None:
        """`\\x0c`, U+2028 and friends are not line breaks to `grep -n`.

        `str.splitlines()` does treat them as breaks, so a document carrying one would
        move every line number the guard reports after that point -- while the reader
        following up with grep sees a different number. The fixture puts two of them
        above the cited line so the two notions of "line number" genuinely disagree, then
        pins the reproducible one, for the mention path and the link path separately.
        """
        text = (
            "para one\u2028still para one\n"
            "note\x0csecond half\n"
            "\n"
            "see [gone](docs/missing.md) and `docs/gone2.md`\n"
        )
        self.assertGreater(len(text.splitlines()), len(guard.lines_of(text)),
                           "the fixture has to be text the two splitters disagree about")
        mentions, problems = scan_text("docs/guide.md", text)
        self.assertEqual([where for _, where in mentions], ["docs/guide.md:4"], mentions)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("docs/guide.md:4", problems[0], problems[0])


class Allowlist(unittest.TestCase):
    def recorded(self, root: Path, tok: str = "docs/missing.md",
                 category: str = "recorded-absent", reason: str = "x" * 30) -> Path:
        path = root / "allow.tsv"
        path.write_text(f"{tok}\t{category}\t{reason}\n", encoding="utf-8")
        return path

    def broken_doc(self, tmp: str) -> Path:
        root = Path(tmp)
        build(root)
        write(root, "docs/guide.md", "see `docs/missing.md`\n")
        git(root, "add", "-A")
        return root

    def test_unrecorded_dangling_mention_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self.broken_doc(tmp)
            proc = run(root)
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("docs/missing.md", proc.stderr)
        self.assertIn("(unrecorded)", proc.stderr)

    def test_recorded_mention_passes_and_stays_live(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self.broken_doc(tmp)
            allow = self.recorded(root)
            ok = run(root, "--allowlist", str(allow))
            self.assertEqual(ok.returncode, 0, ok.stdout + ok.stderr)
            # ... and `--all` still prints it, so recording never hides evidence
            shown = run(root, "--allowlist", str(allow), "--all")
            self.assertEqual(shown.returncode, 1)
            self.assertIn("recorded-absent", shown.stderr)

    def test_stale_entry_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            allow = self.recorded(root, tok="docs/fixed-long-ago.md")
            proc = run(root, "--allowlist", str(allow))
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("stale", proc.stderr)

    def test_broken_link_cannot_be_recorded_away(self) -> None:
        # The load-bearing case: the recorded target is a *link*, not a mention.
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, "docs/guide.md", "see [gone](gone.md) and `gone.md`\n")
            git(root, "add", "-A")
            allow = self.recorded(root, tok="gone.md")
            proc = run(root, "--allowlist", str(allow))
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("broken link target", proc.stderr)

    def test_quoted_link_is_not_a_link(self) -> None:
        # CHANGELOG-style prose about a dead link is not itself a dead link. The second
        # half, the same target unquoted, is what makes the first half mean anything.
        with tempfile.TemporaryDirectory() as tmp:
            root = build(Path(tmp))
            write(root, "CHANGELOG.md", "当年带着 `[CHAOS.md](gone/gone.md)`，已修\n")
            git(root, "add", "-A")
            quiet = run(root)
            self.assertEqual(quiet.returncode, 0, quiet.stdout + quiet.stderr)
            write(root, "CHANGELOG.md", "当年带着 [CHAOS.md](gone/gone.md)，已修\n")
            git(root, "add", "-A")
            loud = run(root)
        self.assertEqual(loud.returncode, 1, loud.stdout + loud.stderr)
        self.assertIn("broken link target", loud.stderr)

    def test_unknown_category_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self.broken_doc(tmp)
            allow = self.recorded(root, category="trust-me")
            proc = run(root, "--allowlist", str(allow))
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("is not one of", proc.stderr)

    def test_short_reason_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self.broken_doc(tmp)
            allow = self.recorded(root, reason="because")
            proc = run(root, "--allowlist", str(allow))
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("characters", proc.stderr)

    def test_missing_column_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self.broken_doc(tmp)
            path = root / "allow.tsv"
            path.write_text("docs/missing.md\trecorded-absent\n", encoding="utf-8")
            proc = run(root, "--allowlist", str(path))
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("3 tab-separated columns", proc.stderr)

    def test_duplicate_entry_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self.broken_doc(tmp)
            path = self.recorded(root)
            path.write_text(path.read_text(encoding="utf-8") * 2, encoding="utf-8")
            proc = run(root, "--allowlist", str(path))
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("duplicate", proc.stderr)


class RealRepository(unittest.TestCase):
    def test_committed_docs_pass(self) -> None:
        proc = run(REPO)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)

    def test_scan_set_covers_the_citing_documents(self) -> None:
        listed = run(REPO, "--list")
        names = set(listed.stdout.split())
        for expected in ("TODO.md", "CHANGELOG.md", "CONTRIBUTING.md",
                         "third_party/README.md", "docs/telemetry-policy.md",
                         ".agents/skills/chaos-upstream-sync/SKILL.md"):
            self.assertIn(expected, names)

    def test_scan_set_is_not_trivially_small(self) -> None:
        # A guard that scans four documents would also be green. The repo cites paths in
        # far more files than that, so the scanned set has to stay wide.
        names = run(REPO, "--list").stdout.split()
        self.assertGreater(len(names), 150, names[:5])

    def test_the_shipped_allowlist_is_accepted(self) -> None:
        allow = REPO / "scripts/ci/doc-path-refs-allowlist.tsv"
        self.assertTrue(allow.exists())
        entries = guard.load_allowlist(allow)
        self.assertGreaterEqual(len(entries), 20)
        self.assertTrue(all(cat in guard.CATEGORIES for cat, _ in entries.values()))


if __name__ == "__main__":
    unittest.main(verbosity=2)
