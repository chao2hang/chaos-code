#!/usr/bin/env python3
"""Fixtures for `scripts/ci/check-container-hygiene.py`.

The gate keeps a root-owned file out of a developer's checkout. `scripts/verify-in-docker.sh`
bind-mounts the working tree at `/src` and runs as root, and three things were measured coming back
through that mount on 2026-10-04:

- one gate run left `scripts/ci/__pycache__/check-doc-path-refs.cpython-311.pyc`, and the
  `--full` run that morning left two more (`check-timeout-child`, `test-installer-asset-names`);
- where no `__pycache__` directory exists yet, the container creates one, and then the owner of the
  tree cannot delete it: `rm -rf` over such a directory prints `Permission denied` and exits 1. That
  is how the frozen clone used by a `--full` run had to be removed with a privileged shell;
- even the named volume over `/src/target` leaves a root-owned `target/` behind, because the runtime
  creates a mount point the image does not have inside the parent mount. Over a clean clone, one
  gate run produced exactly that, and creating the directory host-side first left nothing.

So the fixtures pin four things: the runner has to set `PYTHONDONTWRITEBYTECODE=1`, cargo's output
has to stay out of the mount (named volume over `<mount>/target`, or a `CARGO_TARGET_DIR` elsewhere),
a volume mounted inside the checkout needs its directory created on the host, and the image itself
has to set the variable so a bare `docker run --shell` behaves the same.

`RUNNER_BEFORE_FIX` and `DOCKERFILE_BEFORE_FIX` are the shipped text before the fix, character for
character, so the shape that was actually running stays a test case after the scripts were fixed.

    python3 scripts/ci/test-check-container-hygiene.py
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
GATE = HERE / "check-container-hygiene.py"

HEADER = "#!/usr/bin/env bash\nset -euo pipefail\nrepo_root=\"$(cd \"$(dirname \"$0\")/..\" && pwd)\"\n"

#: The `run_args` array exactly as shipped before the fix: the checkout mounted, the build output
#: covered by a named volume, and no `PYTHONDONTWRITEBYTECODE` anywhere.
RUNNER_BEFORE_FIX = """
run_args=(
  --rm
  --init
  --workdir /src
  --volume "${repo_root}:/src"
  --volume chaos-verify-cargo-registry:/usr/local/cargo/registry
  --volume chaos-verify-cargo-git:/usr/local/cargo/git
  --volume chaos-verify-target:/src/target
  --env RUST_MIN_STACK=16777216
  --env GIT_CONFIG_COUNT=1
  --env GIT_CONFIG_KEY_0=safe.directory
  --env GIT_CONFIG_VALUE_0=/src
)
docker run "${run_args[@]}" "${IMAGE_TAG}" bash -c "${1:-true}"
"""

#: Same array with the entry the fix added.
ENV_LINE = '  --env PYTHONDONTWRITEBYTECODE=1\n'

#: The host-side creation of the volume's mount point, as shipped after the fix.
MKDIR_LINE = 'mkdir -p "${repo_root}/target"\n'

#: `RUNNER_BEFORE_FIX` plus both halves of the fix, the control every clean assertion uses.
CLEAN_RUNNER = RUNNER_BEFORE_FIX.replace(")\n", ENV_LINE + ")\n") + MKDIR_LINE

#: The two lines around the image's own `ENV RUST_MIN_STACK`, as shipped before the fix.
DOCKERFILE_BEFORE_FIX = """FROM debian:bookworm-slim
RUN apt-get update
# Test-thread stack for the large xai-grok-shell actor tests, same as CI.
ENV RUST_MIN_STACK=16777216

WORKDIR /src
CMD ["bash"]
"""

DOCKERFILE_ENV_LINE = "# caches created here belong to root\nENV PYTHONDONTWRITEBYTECODE=1\n"

#: `DOCKERFILE_BEFORE_FIX` with the image's own `ENV` line added, the other half of the control.
CLEAN_DOCKERFILE = DOCKERFILE_BEFORE_FIX.replace("\nWORKDIR", "\n" + DOCKERFILE_ENV_LINE + "WORKDIR")

#: `scripts/install-sh-in-docker.sh` shape: a lab directory mounted, nothing from the checkout.
LAB_RUNNER = """
docker run -d --name "$container_name" --volume "${work_dir}/payload:/lab" "$image" sleep infinity
"""


def run_gate(root: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(GATE), "--root", str(root)],
        cwd=REPO,
        capture_output=True,
        text=True,
    )


class Base(unittest.TestCase):
    """The gate runs as a subprocess over a generated tree; the repository is only read."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tree = Path(self._tmp.name) / "repo"
        (self.tree / "scripts").mkdir(parents=True)
        (self.tree / "docker").mkdir(parents=True)

    def write_runner(self, body: str, rel: str = "scripts/verify-in-docker.sh") -> Path:
        path = self.tree / rel
        path.write_text(HEADER + body, encoding="utf-8")
        return path

    def write_dockerfile(self, body: str) -> Path:
        path = self.tree / "docker" / "verify.Dockerfile"
        path.write_text(body, encoding="utf-8")
        return path

    def clean_pair(self) -> None:
        """A runner and an image that both satisfy the rules, used as the control."""
        self.write_runner(CLEAN_RUNNER)
        self.write_dockerfile(CLEAN_DOCKERFILE)

    def runner_with(self, body: str) -> None:
        """A runner variant against an image that is not part of the assertion."""
        self.write_runner(body)
        self.write_dockerfile(CLEAN_DOCKERFILE)

    def assert_flags(self, *fragments: str) -> subprocess.CompletedProcess:
        proc = run_gate(self.tree)
        self.assertNotEqual(proc.returncode, 0, f"expected a finding\nstdout: {proc.stdout}")
        self.assertTrue(
            proc.stderr.strip(),
            f"gate failed without naming the site\nstdout: {proc.stdout}\nstderr: {proc.stderr}",
        )
        for fragment in fragments:
            self.assertIn(fragment, proc.stderr)
        return proc

    def assert_clean(self, verdict: str = "0 problem(s)") -> subprocess.CompletedProcess:
        proc = run_gate(self.tree)
        self.assertEqual(proc.returncode, 0, f"stderr: {proc.stderr}\nstdout: {proc.stdout}")
        self.assertIn(verdict, proc.stdout)
        return proc


@unittest.skipIf(os.name == "nt", "the scripts under test are bash scripts")
class ContainerHygieneTests(Base):
    # -- what has to be caught ---------------------------------------------------------------

    def test_measured_runner_without_the_env_is_flagged(self) -> None:
        """The pre-fix `run_args` array: mount of the checkout, no bytecode guard anywhere."""
        self.write_runner(RUNNER_BEFORE_FIX)
        self.write_dockerfile(CLEAN_DOCKERFILE)
        self.assert_flags("verify-in-docker.sh", "PYTHONDONTWRITEBYTECODE", "/src")

    def test_measured_dockerfile_without_the_env_is_flagged(self) -> None:
        """The pre-fix image: the runner alone would still leave `--shell` unguarded."""
        self.write_runner(CLEAN_RUNNER)
        self.write_dockerfile(DOCKERFILE_BEFORE_FIX)
        self.assert_flags("verify.Dockerfile", "PYTHONDONTWRITEBYTECODE")

    def test_target_directory_needs_a_volume_or_a_moved_cargo_dir(self) -> None:
        """Drop the named volume over `/src/target` and cargo output lands in the checkout."""
        self.runner_with(RUNNER_BEFORE_FIX.replace(")\n", ENV_LINE + ")\n").replace(
            "  --volume chaos-verify-target:/src/target\n", ""))
        self.assert_flags("/src/target", "CARGO_TARGET_DIR")

    def test_cargo_dir_inside_the_mount_is_still_flagged(self) -> None:
        """Relocating cargo to another path under the same mount changes nothing."""
        self.runner_with(
            RUNNER_BEFORE_FIX.replace("  --volume chaos-verify-target:/src/target\n", "")
            .replace(")\n", ENV_LINE + "  --env CARGO_TARGET_DIR=/src/build\n)\n")
        )
        self.assert_flags("/src/target")

    def test_env_set_to_zero_is_flagged(self) -> None:
        """`PYTHONDONTWRITEBYTECODE=0` is an opt-out, not a guard."""
        self.runner_with(RUNNER_BEFORE_FIX.replace(")\n", "  --env PYTHONDONTWRITEBYTECODE=0\n)\n"))
        self.assert_flags("PYTHONDONTWRITEBYTECODE", "0")

    def test_commented_out_env_is_not_an_env_line(self) -> None:
        """A commented entry documents an intention instead of setting one."""
        self.runner_with(RUNNER_BEFORE_FIX.replace(")\n", "  # --env PYTHONDONTWRITEBYTECODE=1\n)\n"))
        self.assert_flags("verify-in-docker.sh", "PYTHONDONTWRITEBYTECODE")

    def test_dockerfile_commented_env_is_flagged(self) -> None:
        self.write_runner(CLEAN_RUNNER)
        self.write_dockerfile(
            DOCKERFILE_BEFORE_FIX.replace("\nWORKDIR", "\n# ENV PYTHONDONTWRITEBYTECODE=1\nWORKDIR")
        )
        self.assert_flags("verify.Dockerfile", "PYTHONDONTWRITEBYTECODE")

    def test_volume_mount_point_needs_a_host_directory(self) -> None:
        """The measured third leak: the runtime makes `/src/target` as root inside the bind mount."""
        self.runner_with(CLEAN_RUNNER.replace(MKDIR_LINE, ""))
        self.assert_flags("verify-in-docker.sh", "/src/target", "mount point")

    def test_host_mkdir_of_another_directory_is_not_the_mount_point(self) -> None:
        """Creating some other host path says nothing about the one a volume is mounted on."""
        self.runner_with(CLEAN_RUNNER.replace(MKDIR_LINE, 'mkdir -p "${repo_root}/build"\n'))
        self.assert_flags("/src/target", "mount point")

    def test_mkdir_inside_a_gate_command_is_not_a_host_mkdir(self) -> None:
        """A `mkdir` in the container creates the path on the side that is not being trusted."""
        self.runner_with(
            CLEAN_RUNNER.replace(MKDIR_LINE, "")
            + 'docker run "${run_args[@]}" "${IMAGE_TAG}" bash -c "mkdir -p /src/target"\n'
        )
        self.assert_flags("/src/target", "mount point")

    def test_finding_says_who_would_own_the_file(self) -> None:
        """The point of the rule is ownership inside the checkout, so the finding has to say it."""
        self.write_runner(RUNNER_BEFORE_FIX)
        self.write_dockerfile(DOCKERFILE_BEFORE_FIX)
        proc = self.assert_flags("root", "checkout")
        self.assertIn("3 problem(s)", proc.stderr)

    # -- what must stay quiet ----------------------------------------------------------------

    def test_env_in_the_runner_is_enough_for_the_runner_rule(self) -> None:
        self.runner_with(CLEAN_RUNNER)
        self.assert_clean()

    def test_env_spellings_the_shell_accepts(self) -> None:
        """`--env NAME=V`, `--env=NAME=V` and `-e NAME=V` all reach the container."""
        for spelling in (
            "  --env PYTHONDONTWRITEBYTECODE=1\n",
            "  --env=PYTHONDONTWRITEBYTECODE=1\n",
            "  -e PYTHONDONTWRITEBYTECODE=1\n",
        ):
            with self.subTest(spelling=spelling.strip()):
                self.runner_with(RUNNER_BEFORE_FIX.replace(")\n", spelling + ")\n") + MKDIR_LINE)
                self.assert_clean()

    def test_named_volume_over_target_is_enough(self) -> None:
        """The control for the cargo rule: this is what the shipped entry point does."""
        self.clean_pair()
        text = (self.tree / "scripts/verify-in-docker.sh").read_text()
        self.assertIn("chaos-verify-target:/src/target", text)
        self.assertIn('mkdir -p "${repo_root}/target"', text)
        self.assert_clean()

    def test_cargo_dir_outside_the_mount_is_enough(self) -> None:
        """Nothing is mounted inside the checkout then, so no host directory is wanted either."""
        self.runner_with(
            RUNNER_BEFORE_FIX.replace("  --volume chaos-verify-target:/src/target\n", "")
            .replace(")\n", ENV_LINE + "  --env CARGO_TARGET_DIR=/cargo-target\n)\n")
        )
        self.assert_clean()

    def test_a_runner_that_mounts_nothing_of_the_checkout_is_not_judged(self) -> None:
        """The lab entry points mount a throwaway directory and need no bytecode guard."""
        self.write_runner(LAB_RUNNER, rel="scripts/install-sh-in-docker.sh")
        self.assert_clean("0 problem(s)")
        self.assertIn("0 mounts the checkout", run_gate(self.tree).stdout)

    def test_dockerfile_env_without_equals_is_accepted(self) -> None:
        """`ENV NAME VALUE` is the other Dockerfile spelling."""
        self.write_runner(CLEAN_RUNNER)
        self.write_dockerfile(
            DOCKERFILE_BEFORE_FIX.replace("\nWORKDIR", "\nENV PYTHONDONTWRITEBYTECODE 1\nWORKDIR")
        )
        self.assert_clean()

    # -- the scan itself ---------------------------------------------------------------------

    def test_repository_is_clean_and_actually_scanned(self) -> None:
        proc = run_gate(REPO)
        self.assertEqual(proc.returncode, 0, f"stderr: {proc.stderr}")
        self.assertIn("1 mounts the checkout", proc.stdout)
        scanned = int(proc.stdout.split("(")[1].split(" shell script")[0])
        self.assertGreaterEqual(scanned, 12, f"only {scanned} scripts scanned: {proc.stdout}")

    def test_tree_without_anything_to_scan_is_an_error(self) -> None:
        """An empty root must not read as a pass; nothing was compared."""
        empty = Path(self._tmp.name) / "empty"
        empty.mkdir()
        proc = run_gate(empty)
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("no shell script", proc.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
