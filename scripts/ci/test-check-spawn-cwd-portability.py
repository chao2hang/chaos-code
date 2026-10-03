#!/usr/bin/env python3
"""Fixtures for `check-spawn-cwd-portability.py`.

The check exists because one fixture line cost 54 tests on the Windows leg, after a
26-minute build, and the run's report said only `cancelled`. These cases are the ways a
host-pinned working directory hides, plus the two ways the check itself could go quietly
useless:

  * the clean shape (`std::env::temp_dir()`) passes, and says which crates it read;
  * `working_directory: PathBuf::from("/tmp")` and `.current_dir("/var/tmp")` fail, with
    the literal and the line they are on;
  * a working directory pinned across a line break still fails at the field's line;
  * a literal buried inside `Some(PathBuf::from("/tmp/x").into())` still fails;
  * bare `"/"` passes on purpose -- it resolves on Windows too, so banning it would buy
    nothing and cost credibility;
  * a `cwd:` string naming a directory the other legs really have (`/tmp`, `/home/user`)
    fails, because production code stats it and the answer decides which branch runs,
    while a made-up path (`/old`, `/nonexistent/...`) passes -- it is the same string
    everywhere, and a rule that demanded a baseline for those would be a rule nobody keeps;
  * a crate the workflow does not list is not scanned, which is what makes the workflow
    the single source of truth for the scope;
  * `"/tmp"` in a line comment, a block comment, a doc comment and a log message does
    not count -- and the same file is then made to count by turning one mention into the
    real field, so a green run proves the blanking works rather than that nothing was read;
  * a Windows-style literal passes;
  * a workflow whose platform step was renamed fails closed with a message that says to
    fix the check, because a scanner that finds no step and reports success is the exact
    way this repository has been burned before;
  * a platform step that lists no `-p` crate fails for the same reason;
  * a listed crate with no manifest fails, so a moved crate cannot become unscanned.

The last case runs the check over this repository, so a source change that reintroduces
a pinned path fails here as well as in CI.

    python3 scripts/ci/test-check-spawn-cwd-portability.py
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-spawn-cwd-portability.py")
REPO = SCRIPT.parents[2]

WORKFLOW = """\
name: CI
on: push
jobs:
  rust:
    runs-on: ubuntu-latest
    steps:
      - run: cargo test --workspace
  platform-tests:
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - name: cargo test (target-OS crates)
        shell: bash
        env:
          RUST_MIN_STACK: "16777216"
        run: |
          set -euo pipefail
          cargo test --locked --no-fail-fast \\
{pkgs}
"""


def make_repo(tmp: Path, crates: list[str], files: dict[str, str]) -> Path:
    """A repository holding `crates`, with `files` written under the first one."""
    for crate in crates:
        manifest = tmp / "crates" / "codegen" / crate / "Cargo.toml"
        manifest.parent.mkdir(parents=True, exist_ok=True)
        manifest.write_text(f'[package]\nname = "{crate}"\nversion = "0.1.0"\n')
    for rel, body in files.items():
        path = tmp / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body, encoding="utf-8")
    workflow = tmp / ".github" / "workflows" / "ci.yml"
    workflow.parent.mkdir(parents=True, exist_ok=True)
    pkgs = ", \\\n".join(f"            -p {c}" for c in crates)
    workflow.write_text(WORKFLOW.format(pkgs=pkgs), encoding="utf-8")
    return tmp


def write_repo_file(repo: Path, rel: str, body: str) -> Path:
    path = repo / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(body, encoding="utf-8")
    return path


def run(repo: Path, workflow: Path | None = None) -> tuple[int, str]:
    argv = [sys.executable, str(SCRIPT), "--repo", str(repo)]
    if workflow is not None:
        argv += ["--workflow", str(workflow)]
    proc = subprocess.run(argv, capture_output=True, text=True)
    return proc.returncode, proc.stdout + proc.stderr


SRC = "crates/codegen/widget/src/lib.rs"


def request_with(line: str) -> str:
    return (
        "use std::path::PathBuf;\n"
        "pub struct TerminalRunRequest {\n"
        "    pub working_directory: PathBuf,\n"
        "}\n"
        "\n"
        "pub fn make_request() -> TerminalRunRequest {\n"
        "    TerminalRunRequest {\n"
        f"{line}\n"
        "    }\n"
        "}\n"
    )


class SpawnCwdPortability(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="spawn-cwd-")
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def test_clean_tree_passes_and_names_what_it_read(self) -> None:
        repo = make_repo(
            self.tmp, ["widget"], {SRC: request_with("        working_directory: std::env::temp_dir(),")}
        )
        code, out = run(repo)
        self.assertEqual(code, 0, out)
        self.assertIn("1 platform-tested crate(s)", out)
        self.assertIn("no working directory pinned", out)

    def test_pinned_working_directory_fails_with_the_literal_and_line(self) -> None:
        repo = make_repo(
            self.tmp, ["widget"], {SRC: request_with("        working_directory: PathBuf::from(\"/tmp\"),")}
        )
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertIn(f"{SRC}:8:", out)
        self.assertIn("'/tmp'", out)
        self.assertIn("std::env::temp_dir()", out)

    def test_multiline_initializer_fails_at_the_field_line(self) -> None:
        body = (
            "use std::path::PathBuf;\n"
            "\n"
            "pub struct TerminalRunRequest {\n"
            "    pub working_directory: PathBuf,\n"
            "}\n"
            "\n"
            "pub fn make_request() -> TerminalRunRequest {\n"
            "    TerminalRunRequest {\n"
            "        working_directory: PathBuf::from(\n"
            '            "/var/lib/chaos/work",\n'
            "        ),\n"
            "    }\n"
            "}\n"
        )
        repo = make_repo(self.tmp, ["widget"], {SRC: body})
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        # The field opens on line 9; the literal sits on the line after it.
        self.assertIn(f"{SRC}:9:", out)
        self.assertIn("'/var/lib/chaos/work'", out)

    def test_literal_nested_in_a_conversion_still_fails(self) -> None:
        line = '        working_directory: Some(PathBuf::from("/tmp/x").into()).unwrap(),'
        repo = make_repo(self.tmp, ["widget"], {SRC: request_with(line)})
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertIn("'/tmp/x'", out)

    def test_current_dir_call_sink_fails(self) -> None:
        body = (
            "use std::process::Command;\n"
            "\n"
            "pub fn spawn_it() {\n"
            '    Command::new("sh").current_dir("/var/tmp").spawn().unwrap();\n'
            "}\n"
        )
        repo = make_repo(self.tmp, ["widget"], {SRC: body})
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertIn(f"{SRC}:4:", out)
        self.assertIn("current_dir call", out)

    def test_bare_root_is_allowed_on_purpose(self) -> None:
        body = (
            "use std::env;\n"
            "\n"
            "pub fn to_root() {\n"
            '    env::set_current_dir("/").unwrap();\n'
            "}\n"
        )
        repo = make_repo(self.tmp, ["widget"], {SRC: body})
        code, out = run(repo)
        self.assertEqual(code, 0, out)

    def test_a_cwd_string_naming_a_real_directory_fails(self) -> None:
        # `TaskTool::run` asks `Path::new(cwd).is_dir()`, so a fixture pointing at /tmp is
        # a question about the host: on the macos and Linux legs the answer is yes, on the
        # Windows leg no, and the two answers take different branches of production code.
        body = (
            "pub struct TaskToolInput {\n    pub cwd: Option<String>,\n}\n\n"
            "pub fn input() -> TaskToolInput {\n"
            '    TaskToolInput { cwd: Some("/tmp".into()) }\n}\n'
        )
        repo = make_repo(self.tmp, ["widget"], {SRC: body})
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertIn(f"{SRC}:6:", out)
        self.assertIn("cwd field is pinned to '/tmp'", out)

    def test_a_cwd_string_no_host_has_is_data(self) -> None:
        # `/old` exists nowhere, so every leg takes the same branch: flagging this would
        # make the check a tax on ordinary fixtures, which is how checks get switched off.
        for path in ("/old", "/nonexistent/path/that/does/not/exist", "/new/dir"):
            with self.subTest(path):
                body = (
                    "use std::path::PathBuf;\n\n"
                    "pub struct ShellState {\n    pub cwd: PathBuf,\n}\n"
                )
                repo = make_repo(
                    self.tmp / path.strip("/").replace("/", "_"),
                    ["widget"],
                    {SRC: body + f'\npub fn state() -> ShellState {{ ShellState {{ cwd: PathBuf::from("{path}") }} }}\n'},
                )
                code, out = run(repo)
                self.assertEqual(code, 0, out)

    def test_crates_outside_the_platform_list_are_not_scanned(self) -> None:
        repo = make_repo(
            self.tmp,
            ["widget", "bystander"],
            {
                "crates/codegen/bystander/src/lib.rs": request_with(
                    '        working_directory: PathBuf::from("/tmp"),'
                )
            },
        )
        workflow = repo / ".github" / "workflows" / "ci.yml"
        text = workflow.read_text()
        workflow.write_text(text.replace("            -p bystander\n", ""))
        code, out = run(repo)
        self.assertEqual(code, 0, out)
        self.assertIn("1 platform-tested crate(s)", out)

    def test_comments_and_log_strings_are_not_call_sites(self) -> None:
        body = "\n".join(
            [
                "//! `working_directory: PathBuf::from(\"/tmp\")` is the bug this guard bans.",
                "pub fn note() {",
                '    // working_directory: PathBuf::from("/tmp"),',
                "    /* working_directory: PathBuf::from(\"/tmp\"), */",
                '    tracing::info!("wrote /tmp somewhere");',
                "}",
                "",
            ]
        )
        repo = make_repo(self.tmp, ["widget"], {SRC: body})
        code, out = run(repo)
        self.assertEqual(code, 0, out)

        # Same file, one mention turned into live code: proves the blanking above worked
        # rather than the scanner having read nothing.
        live = body.replace(
            '    tracing::info!("wrote /tmp somewhere");',
            '    let _ = tracing::info!("wrote /tmp somewhere");\n'
            '    let request = TerminalRunRequest { working_directory: PathBuf::from("/tmp") };',
        )
        repo2 = make_repo(self.tmp / "live", ["widget"], {SRC: live})
        code, out = run(repo2)
        self.assertEqual(code, 1, out)
        self.assertIn(f"{SRC}:6:", out)
        self.assertEqual(out.count("/tmp'"), 1, out)

    def test_windows_style_literal_is_not_posix_pinned(self) -> None:
        repo = make_repo(
            self.tmp,
            ["widget"],
            {SRC: request_with('        working_directory: PathBuf::from("C:\\\\Users\\\\me"),')},
        )
        code, out = run(repo)
        self.assertEqual(code, 0, out)

    def test_renamed_platform_step_fails_closed(self) -> None:
        repo = make_repo(
            self.tmp, ["widget"], {SRC: request_with("        working_directory: std::env::temp_dir(),")}
        )
        workflow = repo / ".github" / "workflows" / "ci.yml"
        workflow.write_text(workflow.read_text().replace("cargo test (target-OS crates)", "cargo test everywhere"))
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertIn("no `cargo test (target-OS crates: ...)` step", out)

    def test_platform_step_without_crate_list_fails_closed(self) -> None:
        repo = make_repo(
            self.tmp, ["widget"], {SRC: request_with("        working_directory: std::env::temp_dir(),")}
        )
        workflow = repo / ".github" / "workflows" / "ci.yml"
        workflow.write_text(workflow.read_text().replace("-p widget", "--workspace"))
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertIn("list no `-p` crates", out)

    def test_split_platform_steps_are_scanned_together(self) -> None:
        # The real workflow runs the crate list across two steps, because one step means
        # one timeout and one hang erases every crate's verdict with it. The gate has to
        # follow the union, or the crate that moved to its own step quietly stops being
        # checked.
        repo = make_repo(self.tmp, ["widget", "gadget"], {})
        workflow = repo / ".github" / "workflows" / "ci.yml"
        text = workflow.read_text()
        head = text[: text.index("      - name: cargo test (target-OS crates)")]
        workflow.write_text(
            head
            + "      - name: cargo test (target-OS crates: except gadget)\n"
            "        timeout-minutes: 35\n"
            "        shell: bash\n"
            "        run: |\n"
            "          set -euo pipefail\n"
            "          cargo test --locked --no-fail-fast \\\n"
            "            -p widget\n"
            "      - name: cargo test (target-OS crates: gadget)\n"
            "        timeout-minutes: 35\n"
            "        if: always()\n"
            "        shell: bash\n"
            "        run: |\n"
            "          set -euo pipefail\n"
            "          cargo test --locked --no-fail-fast -p gadget\n"
        )
        code, out = run(repo)
        self.assertEqual(code, 0, out)
        self.assertIn("2 platform-tested crate(s)", out)

        # And the crate that sits alone on its own step is really scanned: only a hit in
        # it can make this fail.
        write_repo_file(
            repo, "crates/codegen/gadget/src/lib.rs", request_with('        working_directory: PathBuf::from("/tmp"),')
        )
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertIn("crates/codegen/gadget/src/lib.rs:8:", out)

    def test_platform_step_outside_the_prefix_fails_closed(self) -> None:
        # A third step naming crates by package, one the gate's step-name prefix does not
        # cover, would test crates nothing scans. That is the silent shape of this bug, so
        # it is an error rather than a smaller scan.
        repo = make_repo(self.tmp, ["widget"], {})
        workflow = repo / ".github" / "workflows" / "ci.yml"
        text = workflow.read_text()
        workflow.write_text(
            text.rstrip("\n")
            + "\n      - name: cargo test (the slow ones)\n"
            "        shell: bash\n"
            "        run: |\n"
            "          cargo test --locked -p laggard\n"
        )
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertIn("cargo test (the slow ones)", out)
        self.assertIn("no spawn-cwd gate", out)

    def test_renamed_platform_job_fails_closed(self) -> None:
        repo = make_repo(
            self.tmp, ["widget"], {SRC: request_with("        working_directory: std::env::temp_dir(),")}
        )
        workflow = repo / ".github" / "workflows" / "ci.yml"
        workflow.write_text(workflow.read_text().replace("  platform-tests:", "  other-platform-tests:"))
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertIn("no `platform-tests:` job", out)

    def test_listed_crate_without_a_manifest_fails(self) -> None:
        repo = make_repo(
            self.tmp, ["widget"], {SRC: request_with("        working_directory: std::env::temp_dir(),")}
        )
        (repo / "crates" / "codegen" / "widget" / "Cargo.toml").unlink()
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertIn("no Cargo.toml found: widget", out)

    def test_raw_string_and_char_literal_do_not_desync_the_scan(self) -> None:
        # The raw literal holds a banned call site inside generated-code text, and the
        # char literal holds a bare double quote. Mis-parsing either one would shift every
        # offset after it: the raw mention must stay invisible, and the real call site on
        # the last line must still be found at its own line.
        body = "\n".join(
            [
                "pub fn generated() -> &'static str {",
                '    r#"working_directory: PathBuf::from("/tmp")"#',
                "}",
                "",
                "pub fn quote() -> char {",
                "    '\"'",
                "}",
                "",
                "pub fn real() {",
                "    let cwd = PathBuf::from(",
                '        "/srv/chaos",',
                "    );",
                '    open_with(cwd.current_dir("/srv/chaos"));',
                "}",
                "",
            ]
        )
        repo = make_repo(self.tmp, ["widget"], {SRC: body})
        code, out = run(repo)
        self.assertEqual(code, 1, out)
        self.assertIn(f"{SRC}:13:", out)
        self.assertEqual(out.count("/srv/chaos'"), 1, out)
        self.assertNotIn("/tmp'", out)

    def test_shipped_tree_has_no_pinned_spawn_cwd(self) -> None:
        code, out = run(REPO)
        self.assertEqual(code, 0, out)


if __name__ == "__main__":
    unittest.main(verbosity=2)
