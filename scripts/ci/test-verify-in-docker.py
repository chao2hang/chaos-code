#!/usr/bin/env python3
"""Fixtures for `scripts/verify-in-docker.sh`, driven with a stubbed `docker`.

The host runner `scripts/verify-gates.sh` carries 50 self-test cases and reads its gate list out
of this very file, so the list is checked from both ends. The entry point that owns that list had
none: its flag parsing, its `--only` filter, its before/after tree checksums, its preflight, its
verdict wording and its exit codes had only ever been exercised by hand, against a real image, one
gate at a time. That is how one bug in its own reporting got found on 2026-10-04: a `diff` under
`set -euo pipefail` ended a 164-second `--only` run before any verdict was printed (fixed in
021b5453, with no fixture that could have caught it earlier). Writing these found a second one the
same day -- `--only ""` ran the whole quick list and printed a verdict claiming a filter had been in
effect, because the guard meant to refuse it tested `$1`, the literal `--only`, instead of `$2`.

A stub `docker` on `PATH` makes the interesting parts cheap. It records every invocation, so the
assertions are about what the script actually asked for -- that `--only` reached exactly one gate,
that a failed preflight ran no gate at all, that every gate run carries the env entries the leak
fixes depend on -- and it can fail on request or rewrite the tree on request, which is how the
verdict and the UNATTRIBUTABLE path get driven for real rather than argued about.

The script under test is copied byte-identically into a throwaway repository (it derives its own
`repo_root` from `$0`, so the copy is what makes the fixture hermetic), and `PATH` is prefixed with
the stub directory, so the real daemon is never addressed. Nothing here builds an image or touches
the cargo volumes; `docker build` and `docker volume create` are answered by the stub.

    python3 scripts/ci/test-verify-in-docker.py
"""

import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SCRIPT = REPO / "scripts" / "verify-in-docker.sh"
if not SCRIPT.is_file():  # pragma: no cover - the shipped layout always has it
    raise SystemExit(f"script under test not found: {SCRIPT}")

STUB = r"""#!/usr/bin/env bash
# Answers the four docker subcommands the entry point uses and records every call.
set -uo pipefail
log="${DOCKER_STUB_LOG}"
# One invocation per line. Gate command lines carry embedded newlines (the `bootstrap` prefix is
# three git config lines), and a wrapped record would split a run into several lines.
line=''
for a in "$@"; do
  a="${a//$'\n'/\\n}"
  line+=" [$a]"
done
printf 'docker%s\n' "$line" >>"$log"

sub="${1:-}"; shift || true
case "${sub}" in
  build)  echo 'sha256:stubbed'; exit 0 ;;
  volume) exit 0 ;;
  run)    ;;
  *) echo "stub: unsupported subcommand '${sub}'" >&2; exit 9 ;;
esac

# Every gate run is `docker run <flags> IMAGE bash -c "<command line>"`; the last word is the
# command line, which is what has to be inspected to tell a preflight from a gate from a shell.
script="${*: -1}"
if [ -n "${DOCKER_STUB_TARGET:-}" ]; then
  printf 'target-probe %s\n' "$([ -d "${DOCKER_STUB_TARGET}" ] && echo yes || echo no)" >>"$log"
fi
case "${script}" in
  *'rev-parse --short HEAD'*) exit "${DOCKER_STUB_PREFLIGHT_RC:-0}" ;;
esac
rc=0
if [ -n "${DOCKER_STUB_RULES:-}" ]; then
  while IFS=$'\t' read -r needle want; do
    [ -n "${needle:-}" ] || continue
    case "${script}" in *"${needle}"*) rc="${want}"; break ;; esac
  done <"${DOCKER_STUB_RULES}"
fi
# A gate that writes into the bind-mounted tree, which is how the tree actually moves mid-run.
if [ -n "${DOCKER_STUB_TOUCH:-}" ] && [ ! -e "${log}.touched" ]; then
  : >"${log}.touched"; echo written-by-a-gate >"${DOCKER_STUB_TOUCH}"
fi
if [ "${rc}" -ne 0 ]; then
  echo "stub gate failing as asked: ${script}" >&2
  exit "${rc}"
fi
# The silent shape is the one that once produced a reported pass with no words after it.
[ -n "${DOCKER_STUB_QUIET:-}" ] || echo "stub gate ran: ${script//$'\n'/ }"
"""

# Each gate run has to carry these, checked against the recorded argv as adjacent pairs so a flag
# that lost its value would not pass.
REQUIRED_RUN_FLAGS = (
    "[--rm]",
    "[--init]",
    "[--workdir] [/src]",
    "[--env] [RUST_MIN_STACK=16777216]",
    "[--env] [PYTHONDONTWRITEBYTECODE=1]",
    "[--env] [GIT_CONFIG_COUNT=1]",
    "[--env] [GIT_CONFIG_KEY_0=safe.directory]",
    "[--env] [GIT_CONFIG_VALUE_0=/src]",
)


def run_script(repo: Path, *args: str, env: dict | None = None) -> subprocess.CompletedProcess:
    full = dict(os.environ)
    full["PATH"] = f"{repo / 'stub-bin'}{os.pathsep}{full['PATH']}"
    full["IMAGE_TAG"] = "stub-image:tag"
    full.update(env or {})
    return subprocess.run(["bash", str(repo / "scripts" / "verify-in-docker.sh"), *args],
                          cwd=repo, capture_output=True, text=True, env=full, timeout=300)


class EntryFixture:
    """A repository shaped like the one the entry point expects, plus a stub docker."""

    def __init__(self, tmp: Path) -> None:
        self.root = tmp / "repo"
        self.log = tmp / "docker.log"
        for rel, body in (
            ("README.md", "gate fixture repo\n"),
            ("docker/verify.Dockerfile", "FROM x\n"),
            ("docs/gate-notes.md", "a tracked doc in the fixture tree\n"),
            # stub-bin/ holds the fake docker and target/ is created by the script itself; neither
            # is content of the repository, and keeping them out of `git status --porcelain` is
            # what lets a fixture assert "the tree was at rest" as an absence of caveat lines.
            (".gitignore", "stub-bin/\ntarget/\n"),
        ):
            path = self.root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(body, encoding="utf-8")
        copied = self.root / "scripts" / "verify-in-docker.sh"
        copied.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(SCRIPT, copied)
        stub_dir = self.root / "stub-bin"
        stub_dir.mkdir()
        stub = stub_dir / "docker"
        stub.write_text(STUB, encoding="utf-8")
        stub.chmod(0o755)
        ident = ["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid"]
        for args in (["init", "-q"], ["add", "-A"], ident + ["commit", "-q", "-m", "fixture repo"]):
            proc = subprocess.run(["git", *args], cwd=self.root, capture_output=True, text=True)
            assert proc.returncode == 0, proc.stderr

    def env(self, **extra: str) -> dict:
        base = {
            "DOCKER_STUB_LOG": str(self.log),
            "DOCKER_STUB_TARGET": str(self.root / "target"),
        }
        base.update(extra)
        return base

    def calls(self) -> list[str]:
        if not self.log.exists():
            return []
        return self.log.read_text(encoding="utf-8").splitlines()

    def gate_runs(self) -> list[str]:
        """Every `docker run` invocation, preflight and shell included."""
        return [ln for ln in self.calls() if ln.startswith("docker [run]")]

    def probes(self) -> list[str]:
        return [ln for ln in self.calls() if ln.startswith("target-probe")]

    def tree_files(self) -> int:
        """How many files the entry point's own fingerprint should see right now."""
        proc = subprocess.run(["git", "ls-files", "-co", "--exclude-standard"],
                              cwd=self.root, capture_output=True, text=True)
        assert proc.returncode == 0, proc.stderr
        return len([ln for ln in proc.stdout.splitlines() if ln])

    def run(self, *args: str, **env: str) -> subprocess.CompletedProcess:
        return run_script(self.root, *args, env=self.env(**env))


class FlagTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.fx = EntryFixture(Path(self._tmp.name))

    def test_help_prints_the_usage_header_and_runs_nothing(self) -> None:
        proc = self.fx.run("--help")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("Usage:", proc.stdout)
        self.assertIn("--only <label>", proc.stdout)
        self.assertEqual(self.fx.calls(), [], "--help must not reach docker")

    def test_an_unknown_argument_is_refused_with_exit_2(self) -> None:
        proc = self.fx.run("--alL")
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("unknown argument: --alL", proc.stderr)
        self.assertEqual(self.fx.calls(), [], "a bad flag must not reach docker")

    def test_only_without_a_label_is_refused(self) -> None:
        proc = self.fx.run("--only")
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("--only needs a gate label", proc.stderr)
        self.assertEqual(self.fx.calls(), [], "an empty pattern would select everything")

    def test_an_empty_pattern_is_refused_rather_than_selecting_everything(self) -> None:
        # The guard read `$1`, which is the literal `--only`, so `--only ""` reached the filter
        # where an empty pattern matches every label: the whole list ran and the verdict claimed
        # a filter had been in effect.
        proc = self.fx.run("--only", "")
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("--only needs a gate label", proc.stderr)
        self.assertEqual(self.fx.calls(), [], "an empty pattern is not a filter")

    def test_a_pattern_matching_no_label_is_an_error_not_a_green_run(self) -> None:
        proc = self.fx.run("--only", "no such gate at all")
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("no such gate at all: no gate label matches it", proc.stderr)
        self.assertIn("verify-gates.sh --list", proc.stderr)
        self.assertEqual(self.fx.gate_runs(), [],
                         "an unmatched pattern must not run the whole list")

    def test_a_label_that_only_full_mode_appends_says_so(self) -> None:
        proc = self.fx.run("--only", "cargo test")
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("appended by --full", proc.stderr)
        self.assertEqual(self.fx.gate_runs(), [])

    def test_the_same_label_is_found_once_full_mode_appends_it(self) -> None:
        proc = self.fx.run("--full", "--only", "cargo test")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("== --only is in effect: 1 of ", proc.stdout)
        self.assertIn("cargo test --workspace", " ".join(self.fx.gate_runs()))

    def test_shell_mode_hands_the_terminal_over_and_runs_no_gate(self) -> None:
        proc = self.fx.run("--shell")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        runs = self.fx.gate_runs()
        self.assertEqual(len(runs), 1, self.fx.calls())
        self.assertIn("[bash]", runs[0])
        self.assertNotIn("[-c]", runs[0], "a shell is not a command line")
        self.assertNotIn("source tree:", proc.stdout, "a shell has no run to fingerprint")


class SelectionTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.fx = EntryFixture(Path(self._tmp.name))

    def test_only_reaches_exactly_the_matching_gate(self) -> None:
        proc = self.fx.run("--only", "tree ownership")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        total = re.search(r"== --only is in effect: 1 of (\d+) gates selected", proc.stdout)
        self.assertIsNotNone(total, proc.stdout)
        self.assertEqual(len(self.fx.gate_runs()), 2, self.fx.calls())  # preflight + one gate
        asked = " ".join(self.fx.gate_runs())
        self.assertIn("check-tree-ownership.py", asked)
        self.assertNotIn("secret-scan", asked)

    def test_the_command_line_reaches_the_container_without_its_label(self) -> None:
        # `label: command` is one array element split in two places; a slip in either would run a
        # truncated command while the verdict still names the gate.
        proc = self.fx.run("--only", "toolchain matches the pin")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        runs = self.fx.gate_runs()
        self.assertEqual(len(runs), 2, runs)
        gate = runs[-1]
        self.assertIn("[bash] [-c] [rustc -V && cargo -V]", gate)
        self.assertNotIn("[toolchain matches the pin]", gate, "the label is not a command")

    def test_a_fragment_selects_every_label_it_hits(self) -> None:
        proc = self.fx.run("--only", "workflow")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        line = re.search(r"== --only is in effect: (\d+) of (\d+) gates selected", proc.stdout)
        self.assertIsNotNone(line, proc.stdout)
        self.assertEqual(line.group(1), str(len(self.fx.gate_runs()) - 1),
                         "the count has to be the gates that actually ran")
        self.assertEqual(line.group(1), "3", proc.stdout)

    def test_two_patterns_select_the_union_not_the_intersection(self) -> None:
        proc = self.fx.run("--only", "tree ownership", "--only", "secret scan")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        line = re.search(r"== --only is in effect: (\d+) of \d+ gates selected", proc.stdout)
        self.assertEqual(line.group(1), "2", proc.stdout)
        asked = " ".join(self.fx.gate_runs())
        for needle in ("check-tree-ownership.py", "secret-scan.sh"):
            self.assertIn(needle, asked)

    def test_a_filtered_run_cannot_be_quoted_as_a_sweep(self) -> None:
        proc = self.fx.run("--only", "tree ownership")
        self.assertIn("selected gates passed in stub-image:tag", proc.stdout)
        self.assertNotIn("all gates passed", proc.stdout)
        self.assertIn("--only was in effect: 1 of ", proc.stdout)
        self.assertIn("this is not a full sweep", proc.stdout)

    def test_an_unfiltered_quick_run_uses_the_sweep_wording(self) -> None:
        proc = self.fx.run()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("all gates passed in stub-image:tag", proc.stdout)
        self.assertNotIn("--only was in effect", proc.stdout)
        total = len(self.fx.gate_runs()) - 1  # minus the preflight
        self.assertEqual(total, 36, f"the quick list moved: {total} gates")


class VerdictTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.fx = EntryFixture(Path(self._tmp.name))

    def test_a_failing_gate_is_named_and_the_run_exits_1(self) -> None:
        rules = self.fx.root / "rules.tsv"
        rules.write_text("secret-scan.sh\t7\n", encoding="utf-8")
        proc = self.fx.run("--only", "secret scan", DOCKER_STUB_RULES=str(rules))
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("FAILED gates: secret scan", proc.stdout)
        self.assertIn("stub gate failing as asked", proc.stderr)
        self.assertNotIn("passed in stub-image:tag", proc.stdout)

    def test_the_first_gate_to_fail_does_not_stop_the_others(self) -> None:
        rules = self.fx.root / "rules.tsv"
        rules.write_text("check-versions.sh\t3\n", encoding="utf-8")
        proc = self.fx.run("--only", "version", "--only", "secret scan",
                           DOCKER_STUB_RULES=str(rules))
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("FAILED gates: version lockstep", proc.stdout)
        self.assertEqual(len(self.fx.gate_runs()), 3, self.fx.calls())

    def test_a_failing_gate_that_prints_nothing_on_stdout_is_still_named(self) -> None:
        rules = self.fx.root / "rules.tsv"
        rules.write_text("check-versions.sh\t1\n", encoding="utf-8")
        proc = self.fx.run("--only", "version lockstep", DOCKER_STUB_RULES=str(rules),
                           DOCKER_STUB_QUIET="1")
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("FAILED gates: version lockstep", proc.stdout)

    def test_a_silent_passing_gate_still_ends_with_a_verdict(self) -> None:
        # The shape that once read as a green sweep with no words: the gate prints nothing, and
        # the run has to say for itself that it passed.
        proc = self.fx.run("--only", "tree ownership", DOCKER_STUB_QUIET="1")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("== tree ownership", proc.stdout)
        self.assertIn("selected gates passed in stub-image:tag", proc.stdout)

    def test_a_failing_preflight_runs_no_gate_and_says_what_to_fix(self) -> None:
        proc = self.fx.run("--only", "tree ownership", DOCKER_STUB_PREFLIGHT_RC="128")
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("preflight: FAIL", proc.stderr)
        self.assertIn("safe.directory", proc.stderr)
        self.assertEqual(len(self.fx.gate_runs()), 1,
                         f"only the preflight may run: {self.fx.calls()}")
        self.assertNotIn("FAILED gates", proc.stdout)

    def test_a_tree_that_moves_mid_run_is_unattributable_and_still_gives_a_verdict(self) -> None:
        # The regression this pins: the report was built with a `diff` under `set -euo pipefail`,
        # so the movement killed the script before the verdict, exit 1, and no words. Movement is
        # produced the way it happens in practice -- a gate writing into the bind-mounted tree.
        touched = self.fx.root / "docs" / "written-by-a-gate.md"
        proc = self.fx.run("--only", "tree ownership", DOCKER_STUB_TOUCH=str(touched))
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("UNATTRIBUTABLE", proc.stdout)
        self.assertIn("written-by-a-gate.md", proc.stdout)
        self.assertIn("selected gates passed in stub-image:tag", proc.stdout)
        self.assertIn("nothing in this run can be attributed to a commit", proc.stdout)

    def test_a_tree_at_rest_prints_one_checksum_and_no_caveat(self) -> None:
        proc = self.fx.run("--only", "tree ownership")
        heads = re.findall(r"== source tree: (\d+) files, checksum (\d+)", proc.stdout)
        self.assertEqual(len(heads), 1, proc.stdout)
        self.assertEqual(heads[0][0], str(self.fx.tree_files()))
        self.assertNotIn("differ from HEAD", proc.stdout)
        self.assertNotIn("UNATTRIBUTABLE", proc.stdout)

    def test_a_file_nobody_committed_is_counted_and_declared(self) -> None:
        before = self.fx.run("--only", "tree ownership")
        count_before = re.search(r"== source tree: (\d+) files", before.stdout).group(1)
        # An editor's new file, which is the kind of path a run races with.
        (self.fx.root / "new-module-note.md").write_text("in the tree, not in a commit\n",
                                                         encoding="utf-8")
        proc = self.fx.run("--only", "tree ownership")
        count_after = re.search(r"== source tree: (\d+) files", proc.stdout).group(1)
        self.assertEqual(int(count_after), int(count_before) + 1,
                         "untracked content has to be inside the fingerprint")
        self.assertIn("1 path(s) differ from HEAD, so this run describes the working tree, "
                      "not a commit", proc.stdout)
        self.assertLess(proc.stdout.index("path(s) differ from HEAD"),
                        proc.stdout.index("== tree ownership"),
                        "the caveat has to precede the gate output")


class MountContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.fx = EntryFixture(Path(self._tmp.name))

    def test_every_gate_run_carries_the_env_the_leak_fixes_depend_on(self) -> None:
        self.fx.run("--only", "workflow yaml")
        runs = self.fx.gate_runs()
        self.assertEqual(len(runs), 2, runs)
        for line in runs:
            for entry in REQUIRED_RUN_FLAGS:
                self.assertIn(entry, line, f"{entry} missing from: {line}")
            self.assertIn("[bash] [-c]", line, f"not a command line: {line}")

    def test_the_build_output_directory_precedes_the_first_run(self) -> None:
        # The mount point of a named volume is created by the runtime as root inside its parent,
        # so the host has to own it first. Probed from inside the stub, at run time.
        proc = self.fx.run("--only", "tree ownership")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertTrue(self.fx.probes(), "no run reached the container")
        self.assertEqual(set(self.fx.probes()), {"target-probe yes"}, self.fx.calls())
        self.assertTrue((self.fx.root / "target").is_dir())

    def test_the_volumes_are_named_not_bind_mounted(self) -> None:
        self.fx.run("--only", "tree ownership")
        calls = "\n".join(self.fx.calls())
        for volume in ("chaos-verify-cargo-registry", "chaos-verify-cargo-git",
                       "chaos-verify-target"):
            self.assertIn(f"[volume] [create] [{volume}]", calls)
        self.assertIn("[--volume] [chaos-verify-target:/src/target]", calls)
        self.assertIn(f"[--volume] [{self.fx.root}:/src]", calls)

    def test_the_image_is_built_once_before_any_run(self) -> None:
        self.fx.run("--only", "tree ownership", "--only", "secret scan")
        calls = self.fx.calls()
        builds = [n for n, ln in enumerate(calls) if ln.startswith("docker [build]")]
        first_run = next(n for n, ln in enumerate(calls) if ln.startswith("docker [run]"))
        self.assertEqual(len(builds), 1, calls)
        self.assertLess(builds[0], first_run)


if __name__ == "__main__":
    unittest.main(verbosity=2)
