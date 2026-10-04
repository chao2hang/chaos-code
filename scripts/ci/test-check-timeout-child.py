#!/usr/bin/env python3
"""Fixtures for `check-timeout-child.py`.

Every case drives the shipped script over a synthetic tree and pins one decision it makes, from
both sides wherever the absence of a match would otherwise look like a clean tree. The pair for
"a helper's body is what answers the question" is therefore not only the helper that sets the
flag but the identically-named helper that does not, so a scanner that merely found the string
`kill_on_drop` somewhere in the file cannot pass.

Decisions pinned here, in the order the cases appear:

- the dependency really does say so: the tokio version the gate cites is the one the lockfile
  pins, and against the vendored `tokio` source in `~/.cargo`, when it is readable, the six line
  numbers the gate quotes are the module documentation about a dropped handle, the
  `kill_on_drop` default, the two `output()` / `status()` sentences about the destructor and the
  two `self.spawn()` calls behind them. The premise is checked rather than quoted, and so is the
  claim that `ProcessScope::enroll` needs a `&Child` that `.output()` never hands out.
- the rule is timeout-shaped: a `.spawn()` whose `Child` is enrolled and never abandoned by a
  timeout is left alone, and the same command behind a timeout without the flag is not.
  `timeout_at` counts like `timeout` -- the future it abandons is the same object -- so both
  spellings are pinned, which is what keeps the `_at` branch of the matcher load-bearing.
- the flag is read, not the token: `kill_on_drop(true)` on the chain answers it, and
  `kill_on_drop(false)` is a finding of its own, because that is the default.
- a mutator or a builder is read from its body, which is how most correct sites in this
  repository are correct; a call with no readable body is reported instead of assumed fine.
- helper names resolve inside the calling crate first, so two crates may define `git_command`
  with different answers; two such definitions inside one crate are reported rather than
  resolved by whichever one the scanner reached last, and a method of the same name is excluded
  rather than counted as a second definition.
- `std::process::Command` is counted as a std command instead of reported, and a file that binds
  the bare name `Command` to both modules is reported rather than guessed.
- a future wrapped before the timeout is `unreadable-future` when the wrapped receiver is a
  `let mut` command -- the only receiver `Command::output(&mut self)` can have -- and is not a
  site at all when the same-shaped call belongs to some other type.
- the ledger: a row excuses exactly the finding it names, a row whose finding is gone fails as
  stale, and a malformed row fails before any finding is reported.
- the pairing with `clippy.toml`: the gate refuses a tree whose clippy config no longer bans the
  two `Command::spawn` paths its docstring argues about.
- one case runs the gate on this repository, so it cannot be green in fixtures and red on the
  tree it was written for.

    python3 scripts/ci/test-check-timeout-child.py
"""

import glob
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-timeout-child.py")
REPO = SCRIPT.parents[2]
ALLOWLIST = Path(__file__).with_name("timeout-child-allowlist.tsv")
_spec = importlib.util.spec_from_file_location("check_timeout_child", SCRIPT)
guard = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(guard)

HEAD = "use tokio::process::Command;\n"

# One correct site, reused by the cases that only need the gate to be quiet about a file.
KILLED_INLINE = HEAD + """
async fn snapshot(cwd: &std::path::Path) -> std::io::Result<String> {
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        Command::new("git").arg("rev-parse").current_dir(cwd).kill_on_drop(true).output(),
    )
    .await?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
"""

KILLS_BUILDER = HEAD + """
pub fn git_command(cwd: &std::path::Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd).kill_on_drop(true);
    cmd
}

pub async fn run_it(cwd: &std::path::Path) -> std::io::Result<()> {
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1),
                                   git_command(cwd).output()).await;
    Ok(())
}
"""

CLIPPY = """\
disallowed-methods = [
    { path = "std::process::Command::spawn", reason = "an unenrolled child outlives its session" },
    { path = "tokio::process::Command::spawn", reason = "an unenrolled child outlives its session" },
]
"""


def run(root: Path, allowlist: Path | None = None, *flags: str) -> tuple[int, str]:
    cmd = [sys.executable, str(SCRIPT), "--root", str(root)]
    if allowlist is not None:
        cmd += ["--allowlist", str(allowlist)]
    cmd += list(flags)
    proc = subprocess.run(cmd, capture_output=True, text=True)
    return proc.returncode, proc.stdout + proc.stderr


class CrateDir:
    """`crates/a/src/lib.rs` -> manifest in `crates/a`, package name `a`.

    The package name is what the gate resolves helper names per crate, so a fixture that named
    both crates after their `src` directory would test nothing about crate scoping.
    """

    def __init__(self, rel: Path) -> None:
        parts = list(rel.parts[:-1])
        while parts and parts[-1] in ("src", "tests", "benches", "examples"):
            parts.pop()
        self.dir = Path(*parts) if parts else Path(".")
        self.name = parts[-1] if parts else "root"


class Tree(unittest.TestCase):
    """Writes a one- or two-crate tree, then runs the shipped script over it."""

    def tree(self, sources: dict[str, str], rows: str = "",
             clippy: str | None = CLIPPY) -> Path:
        tmp = Path(tempfile.mkdtemp(prefix="timeout-child-fixture-"))
        self.addCleanup(shutil.rmtree, tmp, ignore_errors=True)
        for rel in sources:
            crate = CrateDir(Path(rel))
            manifest = tmp / crate.dir / "Cargo.toml"
            manifest.parent.mkdir(parents=True, exist_ok=True)
            if not manifest.exists():
                manifest.write_text(f'[package]\nname = "{crate.name}"\nversion = "0.1.0"\n')
        for rel, text in sources.items():
            path = tmp / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        if clippy is not None:
            (tmp / "clippy.toml").write_text(clippy)
        self.ledger = tmp / "ledger.tsv"
        self.ledger.write_text(rows)
        return tmp

    def red(self, sources: dict[str, str], *needles: str) -> str:
        root = self.tree(sources)
        rc, out = run(root, self.ledger)
        self.assertEqual(rc, 1, out)
        for needle in needles:
            self.assertIn(needle, out)
        return out

    def green(self, sources: dict[str, str]) -> str:
        root = self.tree(sources)
        rc, out = run(root, self.ledger)
        self.assertEqual(rc, 0, out)
        return out


class Premise(Tree):
    """The gate's factual claims about tokio, checked against the vendored source."""

    QUOTED = (
        (202, "paradigm of dropping-implies-cancellation"),
        (641, "this value is assumed to be `false`"),
        (974, "The destructor of the future returned by this function will kill"),
        (1037, "The destructor of the future returned by this function will kill"),
        (1003, "let child = self.spawn();"),
        (1069, "let child = self.spawn();"),
    )

    def tokio_source(self) -> Path | None:
        cited = re.search(r"tokio-(\d+\.\d+\.\d+)/src/process/mod\.rs",
                          SCRIPT.read_text(encoding="utf-8"))
        self.assertIsNotNone(cited, "the gate has to cite the tokio file it quotes")
        pinned = re.search(r'name = "tokio"\nversion = "([^"]+)"',
                           (REPO / "Cargo.lock").read_text(encoding="utf-8"))
        self.assertIsNotNone(pinned, "Cargo.lock does not pin a tokio")
        self.assertEqual(cited.group(1), pinned.group(1),
                         f"the gate cites tokio-{cited.group(1)} but Cargo.lock pins "
                         f"{pinned.group(1)}")
        found = sorted(glob.glob(os.path.expanduser(
            f"~/.cargo/registry/src/*/tokio-{cited.group(1)}/src/process/mod.rs")))
        return Path(found[0]) if found else None

    def test_cited_lines_are_what_the_gate_says_they_are(self) -> None:
        source = self.tokio_source()
        if source is None:
            self.skipTest("the pinned tokio source is not vendored on this machine")
        lines = source.read_text(encoding="utf-8").splitlines()
        for lineno, fragment in self.QUOTED:
            self.assertIn(fragment, lines[lineno - 1],
                          f"{source}:{lineno} no longer reads the way the gate quotes it")

    def test_output_and_status_spawn_before_anything_is_awaited(self) -> None:
        """This is why there is no handle to enroll: the calls do the spawning themselves."""
        source = self.tokio_source()
        if source is None:
            self.skipTest("the pinned tokio source is not vendored on this machine")
        text = source.read_text(encoding="utf-8")
        for name in ("output", "status"):
            head = text[text.index(f"pub fn {name}(&mut self)"):]
            self.assertIn("self.spawn()", head[:600],
                          f"{name}() no longer spawns internally, so the enrollability argument "
                          "in the gate's docstring has to be rewritten together with tokio")

    def test_enroll_needs_a_child_handle(self) -> None:
        text = (REPO / "crates/codegen/xai-tty-utils/src/process_scope.rs").read_text(
            encoding="utf-8")
        self.assertIn("pub fn enroll(&self, child: &tokio::process::Child)", text)
        self.assertIn("pub fn enroll_std(&self, child: &std::process::Child)", text)


class TheRule(Tree):
    def test_a_spawned_and_enrolled_child_is_not_a_site(self) -> None:
        """Nothing schedules a drop here, so this gate has no business with the call."""
        source = HEAD + """
async fn serve(scope: &xai_tty_utils::ProcessScope) -> std::io::Result<()> {
    let mut child = Command::new("git").arg("serve").spawn()?;
    scope.enroll(&child);
    child.wait().await?;
    Ok(())
}
"""
        out = self.green({"app/src/lib.rs": source})
        self.assertIn("0 timeout-abandoned output/status site(s)", out)

    def test_a_timeout_without_the_flag_is_the_defect(self) -> None:
        source = HEAD + """
async fn snapshot(cwd: &std::path::Path) -> std::io::Result<String> {
    let out = tokio::time::timeout(std::time::Duration::from_secs(1),
                                     Command::new("git").current_dir(cwd).output()).await?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
"""
        self.red({"app/src/lib.rs": source}, "src/lib.rs:4", "no-kill-on-drop",
                 "app/src/lib.rs::snapshot", "kill_on_drop(true)")

    def test_the_flag_on_the_chain_answers_it(self) -> None:
        out = self.green({"app/src/lib.rs": KILLED_INLINE})
        self.assertIn("1 timeout-abandoned output/status site(s), 1 kill their child on drop",
                      out)

    def test_the_flag_set_to_false_is_its_own_finding(self) -> None:
        """The argument is read, not the token: `false` is the default the docs warn about."""
        source = HEAD + """
async fn snapshot() -> std::io::Result<()> {
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1),
                                   Command::new("git").kill_on_drop(false).output()).await;
    Ok(())
}
"""
        self.red({"app/src/lib.rs": source}, "kill-on-drop-false")

    def test_status_counts_like_output(self) -> None:
        source = HEAD + """
async fn check() -> std::io::Result<()> {
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1),
                                   Command::new("git").status()).await;
    Ok(())
}
"""
        self.red({"app/src/lib.rs": source}, "no-kill-on-drop", "Command::new(\"git\").status()")

    def test_timeout_at_abandons_the_same_future(self) -> None:
        """`timeout_at(deadline, cmd.output())` is the same defect with the clock moved out."""
        source = HEAD + """
async fn check(deadline: tokio::time::Instant) -> std::io::Result<()> {
    let _ = tokio::time::timeout_at(deadline, Command::new("git").status()).await;
    Ok(())
}
"""
        self.red({"app/src/lib.rs": source}, "src/lib.rs:4", "no-kill-on-drop",
                 "app/src/lib.rs::check", "kill_on_drop(true)")

    def test_timeout_at_answers_to_the_flag_too(self) -> None:
        source = HEAD + """
async fn check(deadline: tokio::time::Instant) -> std::io::Result<()> {
    let _ = tokio::time::timeout_at(
        deadline,
        Command::new("git").arg("rev-parse").kill_on_drop(true).output(),
    )
    .await;
    Ok(())
}
"""
        out = self.green({"app/src/lib.rs": source})
        self.assertIn("1 timeout-abandoned output/status site(s), 1 kill their child on drop",
                      out)


class Helpers(Tree):
    def test_a_mutator_that_never_sets_the_flag_leaves_it_broken(self) -> None:
        source = HEAD + """
fn quiet(cmd: &mut Command) {
    cmd.stdin(std::process::Stdio::null());
}

async fn run_it() -> std::io::Result<()> {
    let mut cmd = Command::new("git");
    quiet(&mut cmd);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), cmd.output()).await;
    Ok(())
}
"""
        self.red({"app/src/lib.rs": source}, "no-kill-on-drop", "app/src/lib.rs::run_it")

    def test_a_mutator_that_sets_the_flag_answers_it(self) -> None:
        """This is how the repository's own `detach_search_command` sites are correct."""
        source = HEAD + """
fn quiet(cmd: &mut Command) {
    cmd.stdin(std::process::Stdio::null());
    cmd.kill_on_drop(true);
}

async fn run_it() -> std::io::Result<()> {
    let mut cmd = Command::new("git");
    quiet(&mut cmd);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), cmd.output()).await;
    Ok(())
}
"""
        self.green({"app/src/lib.rs": source})

    def test_a_builder_function_answers_it(self) -> None:
        self.green({"app/src/lib.rs": KILLS_BUILDER})

    def test_a_mutator_with_no_readable_body_is_reported(self) -> None:
        source = HEAD + """
async fn run_it() -> std::io::Result<()> {
    let mut cmd = Command::new("git");
    xai_tty_utils::mystery(&mut cmd);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), cmd.output()).await;
    Ok(())
}
"""
        self.red({"app/src/lib.rs": source}, "unknown-helper", "mystery")

    def test_a_builder_with_no_readable_body_is_reported(self) -> None:
        source = HEAD + """
async fn run_it() -> std::io::Result<()> {
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1),
                                   external::build_command().output()).await;
    Ok(())
}
"""
        self.red({"app/src/lib.rs": source}, "unknown-helper", "build_command")

    def test_a_commented_out_site_is_not_a_site(self) -> None:
        source = HEAD + """
async fn run_it() -> std::io::Result<()> {
    // let _ = timeout(Duration::from_secs(1), Command::new("git").output()).await;
    Ok(())
}
"""
        out = self.green({"app/src/lib.rs": source})
        self.assertIn("0 timeout-abandoned output/status site(s)", out)


class Resolution(Tree):
    PLAIN_BUILDER = KILLS_BUILDER.replace(
        "    cmd.current_dir(cwd).kill_on_drop(true);\n", "")

    def test_the_same_helper_name_may_differ_between_crates(self) -> None:
        """`git_command` is defined in three crates here, two of them building a std command."""
        root = self.tree({"crates/a/src/lib.rs": KILLS_BUILDER,
                          "crates/b/src/lib.rs": self.PLAIN_BUILDER})
        rc, out = run(root, self.ledger)
        self.assertEqual(rc, 1, out)
        self.assertEqual(out.count("no-kill-on-drop"), 1, out)
        self.assertIn("crates/b/src/lib.rs", out)
        self.assertNotIn("crates/a/src/lib.rs:", out)
        self.assertNotIn("defined more than once", out)

    def test_conflicting_definitions_in_one_crate_are_reported(self) -> None:
        """Crate scoping must not quietly become whichever definition the scan reached last."""
        source = HEAD + """
pub mod a {
    use super::*;
    pub fn git_command(cwd: &std::path::Path) -> Command {
        let mut cmd = Command::new("git");
        cmd.current_dir(cwd).kill_on_drop(true);
        cmd
    }
}
pub mod b {
    use super::*;
    pub fn git_command(cwd: &std::path::Path) -> Command {
        Command::new("git").current_dir(cwd)
    }
}
pub async fn run_it(cwd: &std::path::Path) -> std::io::Result<()> {
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1),
                                   a::git_command(cwd).output()).await;
    Ok(())
}
"""
        self.red({"app/src/lib.rs": source}, "unknown-helper", "defined more than once")

    def test_a_method_of_the_same_name_is_not_a_second_definition(self) -> None:
        source = HEAD + """
pub struct Sandbox { cwd: std::path::PathBuf }

impl Sandbox {
    pub fn git_command(&self) -> Command {
        Command::new("git").current_dir(&self.cwd)
    }
}

pub fn git_command(cwd: &std::path::Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd).kill_on_drop(true);
    cmd
}

pub async fn run_it(cwd: &std::path::Path) -> std::io::Result<()> {
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1),
                                   git_command(cwd).output()).await;
    Ok(())
}
"""
        self.green({"app/src/lib.rs": source})


class CommandKind(Tree):
    def test_a_std_command_is_counted_not_reported(self) -> None:
        """`std::process::Command::output` blocks until the child exits; nothing is abandoned."""
        source = """
async fn run_it() -> std::io::Result<()> {
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1),
                                   std::process::Command::new("git").output()).await;
    Ok(())
}
"""
        out = self.green({"app/src/lib.rs": source})
        self.assertIn("1 timeout-abandoned output/status site(s)", out)
        self.assertIn("1 are std commands", out)

    def test_a_bare_name_bound_to_both_modules_is_reported(self) -> None:
        source = """
use std::process::{self, Command};

mod inner {
    use std::path::Path;
    use tokio::process::Command;

    pub async fn run_it(cwd: &Path) -> std::io::Result<()> {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1),
                                      Command::new("git").current_dir(cwd).output()).await;
        Ok(())
    }
}

pub use inner::run_it;
"""
        self.red({"app/src/lib.rs": source}, "ambiguous-command")

    def test_an_aliased_import_elsewhere_does_not_make_it_ambiguous(self) -> None:
        """A test module's `use std::process::Command as StdCommand` says nothing about `Command`."""
        source = HEAD + """
async fn run_it() -> std::io::Result<()> {
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1),
                                  Command::new("git").kill_on_drop(true).output()).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::Command as StdCommand;

    #[test]
    fn helper() {
        let _ = StdCommand::new("git").output();
    }
}
"""
        self.green({"app/src/lib.rs": source})


class WrappedFutures(Tree):
    def test_a_wrapped_command_future_is_reported_as_unreadable(self) -> None:
        source = HEAD + """
async fn plain() -> std::io::Result<()> {
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1),
                                   Command::new("git").output()).await;
    Ok(())
}

async fn wrapped() -> std::io::Result<()> {
    let mut cmd = Command::new("git");
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1),
                                   cmd.output().map(|out| out.status.success())).await;
    Ok(())
}
"""
        out = self.red({"app/src/lib.rs": source}, "unreadable-future", "wrapped")
        # The plain site keeps a second finding in the same file, so a scanner that matched
        # nothing at all cannot satisfy this case by accident.
        self.assertIn("no-kill-on-drop", out)

    def test_the_same_shape_on_another_type_is_not_a_site(self) -> None:
        """`drained.output()` in a test is a tool output; `output(&mut self)` rules it out."""
        source = """
struct Holder { code: u8 }

impl Holder {
    fn new(code: u8) -> Self { Self { code } }
    fn output(&self) -> u8 { self.code }
}

async fn dispatch(_: u8) {}

async fn run_it() {
    let drained = Holder::new(0);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5),
                                   dispatch(drained.output())).await;
}
"""
        out = self.green({"app/src/lib.rs": source})
        self.assertIn("0 timeout-abandoned output/status site(s)", out)

    def test_a_receiver_that_is_not_a_local_is_reported(self) -> None:
        source = HEAD + """
pub struct Runner { cmd: Command }

impl Runner {
    async fn go(&mut self) -> std::io::Result<()> {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1),
                                      self.cmd.output()).await;
        Ok(())
    }
}
"""
        self.red({"app/src/lib.rs": source}, "unreadable-receiver")


class Ledger(Tree):
    BROKEN = HEAD + """
async fn snapshot() -> std::io::Result<()> {
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1),
                                   Command::new("git").output()).await;
    Ok(())
}
"""
    ROW = ("app/src/lib.rs::snapshot\tno-kill-on-drop\tthe child is a read-only `git log`, and "
           "the session that spawns it owns its teardown\n")

    def test_a_recorded_row_excuses_the_finding_it_names(self) -> None:
        root = self.tree({"app/src/lib.rs": self.BROKEN}, self.ROW)
        rc, out = run(root, self.ledger)
        self.assertEqual(rc, 0, out)
        self.assertIn("1 recorded", out)

    def test_a_row_for_the_wrong_category_does_not_excuse_it(self) -> None:
        root = self.tree({"app/src/lib.rs": self.BROKEN},
                         self.ROW.replace("no-kill-on-drop", "unreadable-future"))
        rc, out = run(root, self.ledger)
        self.assertEqual(rc, 1, out)
        self.assertIn("no-kill-on-drop", out)

    def test_a_stale_row_fails(self) -> None:
        root = self.tree({"app/src/lib.rs": KILLED_INLINE}, self.ROW)
        rc, out = run(root, self.ledger)
        self.assertEqual(rc, 1, out)
        self.assertIn("nothing here matches that row", out)

    def bad_row(self, row: str, needle: str) -> None:
        root = self.tree({"app/src/lib.rs": KILLED_INLINE}, row)
        rc, out = run(root, self.ledger)
        self.assertEqual(rc, 1, out)
        self.assertIn(needle, out)

    def test_unknown_category(self) -> None:
        self.bad_row("X\twhatever\treason that is long enough to pass the length check\n",
                     "unknown category")

    def test_short_reason(self) -> None:
        self.bad_row("X\tno-kill-on-drop\ttoo short\n", "write at least 20")

    def test_wrong_column_count(self) -> None:
        self.bad_row("X\tno-kill-on-drop\n", "expected 3 tab-separated columns")

    def test_comments_and_blank_lines_are_not_rows(self) -> None:
        root = self.tree({"app/src/lib.rs": KILLED_INLINE}, "# review note\n\n")
        rc, out = run(root, self.ledger)
        self.assertEqual(rc, 0, out)


class ClippyPairing(Tree):
    def test_a_config_that_dropped_the_spawn_ban_is_refused(self) -> None:
        """The docstring argues with those two entries, so the argument moves with the config."""
        root = self.tree({"app/src/lib.rs": KILLED_INLINE},
                         clippy='disallowed-methods = [\n'
                                '    { path = "reqwest::Client::new", reason = "share a client" },\n'
                                ']\n')
        rc, out = run(root, self.ledger)
        self.assertEqual(rc, 1, out)
        self.assertIn("no longer bans", out)
        self.assertIn("tokio::process::Command::spawn", out)

    def test_a_tree_without_a_clippy_config_is_scanned_anyway(self) -> None:
        root = self.tree({"app/src/lib.rs": KILLED_INLINE}, clippy=None)
        rc, out = run(root, self.ledger)
        self.assertEqual(rc, 0, out)


class ThisRepository(Tree):
    def summary(self) -> tuple[int, str]:
        return run(REPO, ALLOWLIST)

    def test_the_shipped_tree_is_clean(self) -> None:
        rc, out = self.summary()
        self.assertEqual(rc, 0, out)
        self.assertIn("timeout children hold", out)
        sites = int(re.search(r"hold: (\d+) timeout-abandoned", out).group(1))
        killed = int(re.search(r", (\d+) kill their child on drop", out).group(1))
        self.assertGreaterEqual(sites, 12, out)
        self.assertEqual(sites, killed, "every timeout site on the tree must be accounted for")

    def test_the_two_sites_this_gate_found_are_marked(self) -> None:
        rc, out = run(REPO, ALLOWLIST, "--print")
        self.assertEqual(rc, 0, out)
        for path in ("session/goal_classifier.rs:", "session/workflow/host_service.rs:"):
            rows = [line for line in out.splitlines() if path in line]
            self.assertTrue(rows, f"{path} is no longer scanned as a timeout site")
            for line in rows:
                self.assertIn("killed", line, line)

    def test_json_and_the_summary_line_agree(self) -> None:
        rc, text = self.summary()
        self.assertEqual(rc, 0, text)
        rc, out = run(REPO, ALLOWLIST, "--json")
        self.assertEqual(rc, 0, out)
        payload = json.loads(out)
        self.assertEqual(payload["findings"], [])
        self.assertEqual(payload["sites"],
                         int(re.search(r"hold: (\d+) timeout-abandoned", text).group(1)))
        self.assertEqual(payload["killed_on_drop"],
                         int(re.search(r", (\d+) kill their child on drop", text).group(1)))

    def test_the_shipped_allowlist_parses(self) -> None:
        rows, errors = guard.read_allowlist(ALLOWLIST)
        self.assertEqual(errors, [], "the shipped ledger is malformed")
        self.assertIsInstance(rows, dict)


if __name__ == "__main__":
    unittest.main(verbosity=2)
