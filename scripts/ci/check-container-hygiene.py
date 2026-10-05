#!/usr/bin/env python3
"""Container entry points must not leave files in the checkout that it cannot delete.

Every shell script under `scripts/` is read, and a mount of the checkout is what decides
who gets judged. Only one of them has such a mount today: `scripts/verify-in-docker.sh`
bind-mounts the repository at `/src`, while `scripts/install-sh-in-docker.sh`,
`scripts/install-integrity-in-docker.sh`, `scripts/npm-install-in-docker.sh`,
`scripts/remote-acceptance-in-docker.sh` and `scripts/web-deployment-in-docker.sh` mount
a throwaway lab directory instead. The distinction matters because the container runs as
root, so anything it creates through the mount belongs to root inside the developer's
checkout.

Three ways this went wrong have been measured on this machine:

* Sixteen of the 29 test modules under `scripts/ci` load their gate module with
  `importlib.util.spec_from_file_location` rather than running it as a subprocess, and a load
  through the mount writes a bytecode cache. Running one gate left
  `check-doc-path-refs.cpython-311.pyc` behind, and the `--full` run on 2026-10-04 left two
  such files. The developer's own Python is 3.10, so those 3.11 files are never reused and
  never rewritten by the host either.
* Where no `__pycache__` directory exists yet, the container *creates* one, and a
  directory owned by root cannot be emptied by its owner's successor: `rm -rf` on a tree
  containing one exits 1 with `Permission denied`. That is how the frozen clone used for
  the `--full` run had to be deleted. The same holds for the build output: with the
  `chaos-verify-target` volume removed, `mkdir -p /src/target` in the container created a
  `/src/target` owned by root.
* The volume alone is not enough either, and this one turned up after the fix above had
  landed: a mount point that the image does not already have is created by the runtime as
  root *inside the parent mount*. So `--volume chaos-verify-target:/src/target` puts a
  root-owned `target/` in the checkout even though the volume means nothing is written
  through it. Measured over a clean clone on 2026-10-04: one gate run, one root-owned
  directory. Creating it on the host beforehand leaves the run with nothing root-owned
  behind it at all.

So the rules are:

R1. A script that mounts the checkout has to pass `PYTHONDONTWRITEBYTECODE=1` to the
    container, so no cache is written through the mount whatever the image says.
R2. It also has to keep cargo's output out of the mount, either by covering
    `<mount>/target` with a named volume or by pointing `CARGO_TARGET_DIR` somewhere
    outside the mount.
R3. A named volume is mounted on a directory that has to exist first, and the runtime makes
    it as root inside the bind mount when the script does not. So where R2 is satisfied by a
    volume, the script has to create that path on the host itself -- recognised as a `mkdir`
    naming the host side of the mount. A `mkdir` inside a gate command does not count: that
    one runs on the container side, which is the side being judged. Where R2 is satisfied by
    moving `CARGO_TARGET_DIR` out instead, nothing is mounted inside the mount and no `mkdir`
    is wanted.
R4. `docker/verify.Dockerfile`, the image that mount is used with, sets the same
    `PYTHONDONTWRITEBYTECODE` in an `ENV` line, so a bare `docker run` -- including the
    `--shell` this entry point offers -- gets the same behaviour.

Comments do not count, in shell or in the Dockerfile: `# ENV PYTHONDONTWRITEBYTECODE=1`
documents an intention rather than setting one. `ENV NAME VALUE` without an equals sign
is the other valid Dockerfile spelling and is accepted.

What this cannot see: a mount path assembled at runtime, a `docker run` issued by a
command line stored in a variable, or a guard that writes somewhere else on purpose.
Those would have to be caught by looking at the checkout after a run, which needs Docker,
which the gates here do not have. There is no allow list (see
`scripts/ci/check-script-portability.py` for the house policy): a rule that fires is
fixed in the entry point, not in this file.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

BYTECODE_VAR = "PYTHONDONTWRITEBYTECODE"

#: Host side of a mount that names the checkout rather than a lab directory.
CHECKOUT_HOST_RE = re.compile(r"(?:repo_root|repo_dir|REPO_ROOT|checkout|source_root)", re.IGNORECASE)

#: `--volume HOST:CONTAINER[:flags]`, `-v` short form, and `--mount type=bind,...`.
MOUNT_RE = re.compile(
    r"""(?:--volume|-v)[ \t]+["']?([^"'\s]+?):(["'/][^"'\s,\]]+)""",
)

#: `--env NAME=VALUE`, `--env=NAME=VALUE`, `-e NAME=VALUE`, `-e NAME`.
ENV_RE = re.compile(r"""(?:--env[ \t=]+|-e[ \t]+)["']?([A-Za-z_][A-Za-z0-9_]*)=?(["']?)([^"'\s]*)""")

#: `ENV NAME=VALUE` and `ENV NAME VALUE` in a Dockerfile.
DOCKERFILE_ENV_RE = re.compile(r"^ENV[ \t]+([A-Za-z_][A-Za-z0-9_]*)(?:[ \t=]+)(\S+)")


def strip_comment(line: str) -> str:
    """Drop a whole-line shell or Dockerfile comment, keeping indented code."""
    if line.lstrip().startswith("#"):
        return ""
    return line


def shell_lines(text: str) -> list[tuple[int, str]]:
    """Return (line number, text) for non-comment lines of a shell script."""
    return [(n, strip_comment(line)) for n, line in enumerate(text.splitlines(), 1) if strip_comment(line).strip()]


def mounts_checkout(lines) -> list[tuple[int, str, str]]:
    """Return (line, host side, container path) for every mount of the checkout."""
    found = []
    for number, text in lines:
        for match in MOUNT_RE.finditer(text):
            host, container = match.group(1), match.group(2)
            if CHECKOUT_HOST_RE.search(host):
                found.append((number, host, container.rstrip("/")))
    return found


def env_values(lines) -> dict[str, str]:
    """Every environment variable the script passes to the container."""
    values: dict[str, str] = {}
    for _, text in lines:
        for match in ENV_RE.finditer(text):
            name, _, value = match.groups()
            values.setdefault(name, value)
    return values


def precreates_host_dir(lines, host: str) -> bool:
    """True if the script creates the host directory that a volume is mounted on.

    The host side is spelled through a shell variable, so the test is that some `mkdir` line
    names both `target` and the same host token the mount uses. A `mkdir` inside a gate command
    runs on the container side and does not count, which is why the host token is required
    rather than any path ending in `target`.
    """
    token = CHECKOUT_HOST_RE.search(host)
    if token is None:
        return True
    lowered = token.group(0).lower()
    return any("mkdir" in text and "target" in text and lowered in text.lower()
               for _, text in lines)


def volume_targets(lines) -> set[str]:
    """Container paths covered by a named volume (a mount that is not the checkout)."""
    targets = set()
    for _, text in lines:
        for match in MOUNT_RE.finditer(text):
            host, container = match.group(1), match.group(2)
            if not CHECKOUT_HOST_RE.search(host):
                targets.add(container.rstrip("/"))
    return targets


def scan_runner(path: Path, name: str | None = None) -> list[str]:
    name = name or str(path)
    lines = shell_lines(path.read_text(encoding="utf-8", errors="replace"))
    checkout = mounts_checkout(lines)
    if not checkout:
        return []
    env = env_values(lines)
    volumes = volume_targets(lines)
    findings = []

    if env.get(BYTECODE_VAR) != "1":
        seen = env.get(BYTECODE_VAR)
        got = f"is set to {seen!r}" if seen is not None else "is never set"
        findings.append(
            f"{name}: mounts the checkout at {checkout[0][2]} but {BYTECODE_VAR} {got}; "
            "the guards in scripts/ci import each other through that mount, and the "
            "bytecode caches they write belong to root inside the checkout"
        )

    for _, host, container in checkout:
        target = f"{container}/target"
        if target in volumes:
            if not precreates_host_dir(lines, host):
                findings.append(
                    f"{name}: {target} is a named volume mounted inside the checkout but the "
                    f"script never creates {host}/target on the host, so the runtime makes that "
                    "mount point as root and the checkout ends up holding a directory its owner "
                    "did not create"
                )
            continue
        cargo_dir = env.get("CARGO_TARGET_DIR", "")
        if cargo_dir and not cargo_dir.startswith(container + "/") and cargo_dir != container:
            continue
        findings.append(
            f"{name}: {target} is not covered by a named volume and CARGO_TARGET_DIR is "
            f"not moved out of {container}, so cargo writes a build directory owned by "
            "root into the checkout"
        )
    return findings


def scan_dockerfile(path: Path, name: str | None = None) -> list[str]:
    name = name or str(path)
    findings = []
    set_var = None
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        stripped = strip_comment(line).strip()
        match = DOCKERFILE_ENV_RE.match(stripped)
        if match and match.group(1) == BYTECODE_VAR:
            set_var = match.group(2)
    if set_var != "1":
        got = f"is set to {set_var!r}" if set_var is not None else "is never set"
        findings.append(
            f"{name}: {BYTECODE_VAR} {got} in the image; a bare `docker run` with this "
            "tree mounted, including the `--shell` the entry point offers, writes caches "
            "as root regardless of what the entry point passes"
        )
    return findings


def candidates(root: Path):
    """Every shell script under scripts/, not just the ones named *-in-docker.sh.

    Naming a runner is not what makes it dangerous; mounting the checkout is. So the
    scan covers all of them and the mount test decides who is judged.
    """
    runners = sorted(root.glob("scripts/*.sh"))
    dockerfiles = sorted((root / "docker").glob("*.Dockerfile"))
    return runners, dockerfiles


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=".", type=Path, help="repository root to scan")
    parser.add_argument("--verbose", action="store_true", help="list what was judged")
    args = parser.parse_args(argv)

    root = args.root.resolve()
    runners, dockerfiles = candidates(root)
    if not runners and not dockerfiles:
        print(
            f"container-hygiene: no shell script under {root / 'scripts'} and no Dockerfile "
            f"under {root / 'docker'}; point --root at the repository",
            file=sys.stderr,
        )
        return 1

    base = Path.cwd()

    def show(path: Path) -> str:
        try:
            return str(path.relative_to(base))
        except ValueError:
            return str(path)

    runners_scanned = []
    mounted = 0
    mounters: list[str] = []
    for runner in runners:
        if mounts_checkout(shell_lines(runner.read_text(encoding="utf-8", errors="replace"))):
            mounted += 1
            mounters.append(show(runner))
        runners_scanned.append((runner, show(runner), scan_runner(runner, show(runner))))
    dockerfiles_scanned = [
        (path, show(path), scan_dockerfile(path, show(path))) for path in dockerfiles
    ]

    findings = [finding for _, _, found in runners_scanned + dockerfiles_scanned for finding in found]
    if args.verbose:
        for path, name, found in runners_scanned + dockerfiles_scanned:
            if not found:
                print(f"  judged {name}")
        # Named, not just counted: "3 scripts were judged" cannot tell a scanner that
        # found the three entry points from one that matched three unrelated files.
        for name in mounters:
            print(f"  mounts the checkout: {name}")

    for finding in findings:
        print(finding, file=sys.stderr)

    if findings:
        print(
            f"container-hygiene: {len(findings)} problem(s) in {len(runners)} shell script(s) "
            f"scanned, {mounted} mounting the checkout, {len(dockerfiles)} Dockerfile(s)",
            file=sys.stderr,
        )
        return 1
    print(
        f"container-hygiene: OK ({len(runners)} shell script(s) scanned, {mounted} mounts the "
        f"checkout, {len(dockerfiles)} Dockerfile(s), 0 problem(s))"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
