#!/usr/bin/env python3
"""Reject a test that tells the OS to start a process in a directory only one host owns.

Found the expensive way. On run 37074944913 the `platform tests (windows-latest)` leg
spent 26 minutes compiling, entered `cargo test`, and lost 54 tests in
`xai_grok_tools::computer::local::terminal::tests` at once -- every test in the module,
including ones that had never been run on Windows and had nothing to assert yet. The
cause was one line, repeated 14 times:

    working_directory: PathBuf::from("/tmp"),

`TerminalRunRequest::working_directory` is handed straight to
`tokio::process::Command::current_dir` by `spawn_shell_command`, so on a Windows host
the spawn fails before the test's first real assertion. `/tmp` is not a Windows path;
the crate is compiled and tested there. Same failure shape as the two the leg had
already caught (`install.sh`'s host-decided closing line, the `SUN_LEN` socket fixtures):
a test carrying a cross-platform name whose fixture assumes one host.

That is why this check exists and why it is narrow. The rule, per crate the platform legs
actually test:

- The scanned crates are read out of `.github/workflows/ci.yml`, from the `-p` lists of
  the `cargo test (target-OS crates: ...)` steps in the `platform-tests` job -- their
  union, because the Windows leg found that one step means one timeout, and a hang in one
  crate cancels every other crate's verdict with it. The gate cannot drift from the jobs it
  protects: a workflow that renames the job, renames those steps, drops every `-p`, or adds
  a `cargo test -p` step the step-name prefix does not cover fails the check outright
  instead of silently scanning less than it used to.
- A *spawn-cwd sink* is a place whose value reaches the OS as a working directory: the
  `working_directory:` field of a run request, `.current_dir(...)` and
  `set_current_dir(...)`. Those are the sinks this repository has actually been bitten
  by; each is checked for a string literal that is POSIX-absolute (`"/tmp"`,
  `"/home/user/x"`).
- Bare `"/"` is allowed. It resolves to the current drive root on Windows, so it is not
  the host-pinned failure this rule is about, and forbidding it would only teach people
  to write `"/tmp"` harder.
- A field named `cwd` holds a string, not a syscall, so it is checked against
  HOST_DEPENDENT_ROOTS rather than outright: `"/tmp"` and `"/home/user"` are directories
  the macOS and Linux legs have and Windows does not, and `TaskTool::run` calls
  `Path::new(p).is_dir()` on the value -- one host takes "reject cwd + worktree", the other
  "clear the cwd and spawn a subagent". A made-up path (`"/old"`,
  `"/nonexistent/path/that/does/not/exist"`) is the same string everywhere and stays legal.
  The `"/tmp"` case is not theoretical: it is the second half of the incident above, worth
  three failures and a 16m43s hang on the same Windows run.

Usage: python3 scripts/ci/check-spawn-cwd-portability.py [--repo DIR] [--workflow FILE]
Exit: 0 = no spawn-cwd sink in a platform-tested crate is pinned to a POSIX path.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

PLATFORM_JOB = re.compile(r"^  platform-tests:\s*$", re.MULTILINE)
STEP_FIRST_NAME = re.compile(r"^name:\s*(.*)$", re.MULTILINE)
# A prefix, not a fixed name: the platform job splits the crate list across steps (see
# `platform_crates`), and every one of them has to be in scope.
STEP_NAME = re.compile(r"cargo test \(target-OS crates")
# Any `cargo test` that names crates by package, used to catch a platform step that the
# prefix above does not cover and would therefore leave unscanned.
CARGO_TEST_WITH_PKG = re.compile(r"\bcargo\s+test\b[^\n]*\s-p\s")
# Comments are stripped from the step body before this runs, so a `-p` token in what is
# left is a package argument. Both spellings occur: one crate per line after a `\`, and a
# single-line invocation for the crate that got its own step.
PACKAGE_ARG = re.compile(r"(?:^|[\s\\])-p\s+([A-Za-z0-9_.-]+)", re.MULTILINE)
CRATE_NAME_IN_MANIFEST = re.compile(r'^\[package\][^\[]*?^name\s*=\s*"([^"]+)"', re.MULTILINE | re.DOTALL)

# Sinks whose value becomes the OS-level working directory of a spawned process.
FIELD_SINK = re.compile(r"\bworking_directory\s*:\s*")
CALL_SINK = re.compile(r"\.(?:current_dir|set_current_dir)\s*\(\s*")
# A `cwd` *string*: not a spawn call yet, but `TaskToolInput.cwd` is fed to
# `Path::new(p).is_dir()` in `grok_build/task/mod.rs` and on that answer production picks
# between rejecting the request and spawning a subagent. Same literal, different host,
# different branch -- see HOST_DEPENDENT_ROOTS for what counts.
CWD_FIELD = re.compile(r"\bcwd\s*:\s*")

# A POSIX-absolute path literal with at least one path segment. Bare "/" is excluded on
# purpose (see the docstring).
POSIX_LITERAL = re.compile(r'"(/[A-Za-z0-9._~+-][^"\n]*)"')

# First-level directories the macOS and Linux legs have and the Windows leg does not, so a
# fixture naming one is asking the host a question. `"/old"`, `"/new/dir"` and
# `"/nonexistent/path/that/does/not/exist"` are not in this world: no host has them, so they
# test the same branch everywhere and are left alone.
HOST_DEPENDENT_ROOTS = frozenset(
    {
        "tmp", "var", "usr", "etc", "opt", "srv", "home", "root", "private", "mnt",
        "media", "run", "bin", "sbin", "lib", "dev", "proc", "sys",
        "Users", "Library", "System", "Applications", "Volumes",
    }
)


def top_level(literal: str) -> str:
    return literal.strip("/").split("/", 1)[0]


SINKS = (
    (FIELD_SINK, "working_directory field", None),
    (CALL_SINK, "current_dir call", None),
    (CWD_FIELD, "cwd field", HOST_DEPENDENT_ROOTS),
)

MANIFEST_ROOTS = ("crates", "prod", "third_party")


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8", errors="replace")


RAW_STRING = re.compile(r'r(#*)"')
CHAR_LITERAL = re.compile(r"'(?:\\.|[^\\'])'")


def strip_comments(text: str) -> tuple[str, bytearray]:
    """Blank `//` and block comments, and mark the inside of every literal.

    Returns the offset-preserving text plus a mask that is 1 wherever a character sits
    *inside* a string or char literal. Both halves are needed. A commented-out
    `working_directory: PathBuf::from("/tmp")` describes the rule instead of breaking it,
    and this file's own docstring is the proof: the check that bans the line has to quote
    it. But the literal is also the signal this check looks for, so strings cannot simply
    be blanked -- instead a sink whose name sits inside a literal is a quoted snippet, not
    a call site. Raw strings and `'"'` have to be recognised as tokens, or a `"` inside
    one desynchronises the scan and hides or invents a call site.
    """
    out = list(text)
    mask = bytearray(len(text))
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        if ch == "/" and i + 1 < n and text[i + 1] == "/":
            j = text.find("\n", i)
            j = n if j < 0 else j
            _blank(out, i, j)
            i = j
        elif ch == "/" and i + 1 < n and text[i + 1] == "*":
            depth = 1
            j = i + 2
            while j < n and depth:
                if text.startswith("*/", j):
                    depth -= 1
                    j += 2
                elif text.startswith("/*", j):
                    depth += 1
                    j += 2
                else:
                    j += 1
            _blank(out, i, j)
            i = j
        elif ch == "r" or (ch in "bc" and text.startswith("r", i + 1)) or ch in "\"'":
            j = skip_token(text, i)
            if j > i + 1:
                for k in range(i + 1, j - 1):
                    mask[k] = 1
            i = j
        else:
            i += 1
    return "".join(out), mask


def skip_token(text: str, i: int) -> int:
    """Index just past the string/char literal (or just past `text[i]` if there is none)."""
    if text[i] == '"':
        return _skip_string(text, i)
    if text[i] == "'":
        char = CHAR_LITERAL.match(text, i)
        return char.end() if char else i + 1
    raw = RAW_STRING.match(text, i) or RAW_STRING.match(text, i + 1)
    if raw and raw.group(0).startswith("r"):
        return _skip_raw_string(text, raw)
    return i + 1


def _skip_raw_string(text: str, opener: re.Match[str]) -> int:
    """Index just past the raw literal whose opener `opener` matched."""
    hashes = len(opener.group(1))
    closer = '"' + "#" * hashes
    end = text.find(closer, opener.end())
    return len(text) if end < 0 else end + len(closer)


def _blank(out: list[str], start: int, end: int) -> None:
    for k in range(start, end):
        if out[k] != "\n":
            out[k] = " "


def _skip_string(text: str, i: int) -> int:
    """Index just past the string literal starting at `text[i] == '"'`."""
    j = i + 1
    n = len(text)
    while j < n:
        if text[j] == "\\":
            j += 2
            continue
        if text[j] == '"':
            return j + 1
        if text[j] == "\n":
            return j
        j += 1
    return n


def initializer_end(text: str, start: int) -> int:
    """End of the expression starting at `start`: the first top-level `,` or `;`."""
    depth = 0
    i = start
    n = len(text)
    while i < n:
        ch = text[i]
        if ch in "\"'r" or (ch == "b" and text.startswith("r", i + 1)):
            i = skip_token(text, i)
            continue
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            if depth == 0:
                return i
            depth -= 1
        elif depth == 0 and ch in ",;":
            return i
        elif ch == "\n" and depth == 0:
            return i
        i += 1
    return n


def line_of(text: str, pos: int) -> int:
    return text.count("\n", 0, pos) + 1


def platform_crates(workflow: Path) -> list[str]:
    """The `-p` list of the platform job's `cargo test (target-OS crates: ...)` step(s).

    The Windows leg split that one step into two -- a hang in `xai-grok-tools` was
    cancelling the five other crates' verdicts along with its own -- so the scope is the
    union over every step whose name carries the prefix. The union is not open-ended: a
    `cargo test` that names `-p` crates in that job from a step the prefix does not cover
    fails the check, because the silent version of that mistake is a crate added to a leg
    and never scanned.
    """
    if not workflow.is_file():
        raise SystemExit(f"spawn-cwd portability: workflow not found: {workflow}")
    text = read(workflow)
    job_start = PLATFORM_JOB.search(text)
    if job_start is None:
        raise SystemExit(
            "spawn-cwd portability: no `platform-tests:` job in "
            f"{workflow}; the macOS and Windows legs are what this gate protects, so "
            "renaming the job means updating this check rather than passing it"
        )
    rest = text[job_start.end() :]
    # The next sibling key, not the next non-blank line: jobs are preceded by a
    # 2-space-indented comment block, and truncating there would leave no steps to read.
    next_block = re.search(r"^  [A-Za-z0-9_-]+:", rest, re.MULTILINE)
    job = rest[: next_block.start()] if next_block else rest

    # A step is a `- ` item of the job's `steps:` list. The indent is read rather than
    # assumed, so a re-indented workflow fails with "no step" instead of quietly reading
    # the wrong slice.
    steps_indent = re.search(r"^([ ]+)steps:[ ]*$", job, re.MULTILINE)
    # GitHub's own style indents the dash two further than `steps:`; both are accepted so
    # a re-format does not read as "no steps".
    item = (
        re.compile(r"^" + re.escape(steps_indent.group(1)) + r"(?:  )?- ", re.MULTILINE)
        if steps_indent
        else None
    )
    chunks: list[tuple[str, str]] = []
    for piece in (item.split(job)[1:] if item else []):
        name_match = STEP_FIRST_NAME.search(piece)
        name = name_match.group(1).strip() if name_match else ""
        body = "\n".join(
            line for line in piece.splitlines() if not line.lstrip().startswith("#")
        )
        chunks.append((name, body))

    crates: list[str] = []
    seen_step = False
    uncovered: list[str] = []
    for name, body in chunks:
        if STEP_NAME.search(name):
            seen_step = True
            for crate in PACKAGE_ARG.findall(body):
                if crate not in crates:
                    crates.append(crate)
        elif CARGO_TEST_WITH_PKG.search(body):
            uncovered.append(name or "(unnamed step)")
    if uncovered:
        raise SystemExit(
            "spawn-cwd portability: the platform-tests job tests crates by name outside a "
            "`cargo test (target-OS crates: ...)` step: "
            + "; ".join(uncovered)
            + " -- those crates get no spawn-cwd gate; rename the step or add it here"
        )
    if not seen_step:
        raise SystemExit(
            "spawn-cwd portability: no `cargo test (target-OS crates: ...)` step in the "
            f"platform-tests job of {workflow}; rename the step or update this check, do "
            "not let it scan nothing"
        )
    if not crates:
        raise SystemExit(
            "spawn-cwd portability: the platform test steps list no `-p` crates; "
            "a step that tests nothing needs no gate, so this check refuses to pass"
        )
    return crates



def crate_dirs(repo: Path, wanted: list[str]) -> dict[str, Path]:
    """Map crate name -> directory, by the `name` in Cargo.toml, not the folder name."""
    found: dict[str, Path] = {}
    for root_name in MANIFEST_ROOTS:
        root = repo / root_name
        if not root.is_dir():
            continue
        for manifest in sorted(root.rglob("Cargo.toml")):
            if "target" in manifest.parts:
                continue
            declared = CRATE_NAME_IN_MANIFEST.search(read(manifest))
            if declared and declared.group(1) in wanted:
                found.setdefault(declared.group(1), manifest.parent)
    return found


def scan_file(path: Path) -> list[tuple[int, str, str]]:
    raw = read(path)
    clean, mask = strip_comments(raw)
    hits: list[tuple[int, str, str]] = []
    for pattern, label, roots in SINKS:
        for match in pattern.finditer(clean):
            if mask[match.start()]:
                continue  # a sink name quoted inside a literal is a snippet, not code
            end = initializer_end(clean, match.end())
            start = match.start()
            literal = None
            for candidate in POSIX_LITERAL.finditer(clean, match.end(), end):
                if not mask[candidate.start()]:
                    literal = candidate.group(1)
                    break
            if not literal:
                continue
            if roots and top_level(literal) not in roots:
                continue  # a path no host has is a made-up string, not a host dependency
            hits.append((start, label, literal))
    return [(line_of(raw, pos), label, lit) for pos, label, lit in hits]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--repo", type=Path, default=Path("."), help="repository root")
    ap.add_argument(
        "--workflow",
        type=Path,
        default=None,
        help="workflow to read the platform crate list from (default: <repo>/.github/workflows/ci.yml)",
    )
    args = ap.parse_args()

    repo = args.repo
    workflow = args.workflow or repo / ".github" / "workflows" / "ci.yml"
    crates = platform_crates(workflow)
    dirs = crate_dirs(repo, crates)

    missing = [c for c in crates if c not in dirs]
    if missing:
        print(
            "spawn-cwd portability: platform crates with no Cargo.toml found: "
            + ", ".join(missing),
            file=sys.stderr,
        )
        return 1

    offenders: list[str] = []
    files = 0
    for crate in crates:
        for src in sorted(dirs[crate].rglob("*.rs")):
            if "target" in src.parts:
                continue
            files += 1
            for line, label, literal in scan_file(src):
                offenders.append(
                    f"{src.relative_to(repo)}:{line}: {label} is pinned to {literal!r}, "
                    "which does not exist on every host the platform legs run"
                )

    for line in sorted(offenders):
        print(line)
    if offenders:
        print(
            f"\nspawn-cwd portability: {len(offenders)} spawn-cwd sink(s) name a POSIX-only "
            "path; pass std::env::temp_dir() (or a TempDir) instead so the same test works "
            "on the macos and windows legs",
            file=sys.stderr,
        )
        return 1
    print(
        f"spawn-cwd portability: {len(crates)} platform-tested crate(s), {files} .rs file(s), "
        "no working directory pinned to a path one host does not have"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
