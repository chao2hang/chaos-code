#!/usr/bin/env python3
"""Fail when an installer asks for a release asset that nobody publishes.

Why this exists: four consumers derive a release asset name, in four languages, and
one publisher decides what the name is, in a fifth place.

- `.github/workflows/release.yml` uploads `chaos-<os>-<arch>[.exe]`, named by hand in
  six `copy_one` lines;
- `scripts/install.sh` builds `ASSET` in a bash `case` statement;
- `scripts/install.ps1` builds it in a PowerShell function with two branches and an
  environment-variable fallback;
- `scripts/install.bat` builds it with three `set` lines keyed off
  `PROCESSOR_ARCHITECTURE`;
- `xai-grok-update`'s `gh_release_asset_name` builds it again for `chaos update`
  (that one has its own test against this same workflow file, in Rust).

A rename on the publisher's side is invisible to every consumer until a user's install
404s, and a 404 reads as "that version does not exist" rather than as "these two files
disagree". Dropping the Windows arm64 build, for instance, is a one-line edit here and a
silent `install.ps1` failure on every Arm Windows machine -- and the tree already has one
platform name that exists on one side only (`chaos-code-win32-*`, which npm has never
served under the pinned version), so "the names agree" is not a thing to assume.

This check reads the workflow and requires every name each installer can produce to be in
that set. `install.sh`'s function and `install.ps1`'s function are *executed* rather than
parsed, so what is checked is the shipped code: bash runs `detect_platform` once per
`uname` answer the case statement recognises -- including the arms this host is not --
and pwsh runs `Get-AssetName` for this host. The literal names in each are collected
alongside, so an arm added to the installer without a matching `copy_one` line is caught
even if nobody added it to the probe table below.

Locating bash is itself a checked step. Windows runners have `C:\\Windows\\System32\\
bash.exe` on `PATH` ahead of Git for Windows' bash: that one is the WSL launcher, and with
no distro installed it exits non-zero with its complaint on *stdout*, which looks exactly
like `install.sh` failing. `find_bash` therefore prefers `$BASH` (the interpreter a
`shell: bash` step is actually running), skips anything under `System32`, and executes
every candidate before accepting it.

`--require` makes a missing `pwsh` a failure instead of a skip, the way
`check-powershell-syntax.py` does on the platform legs. Without it, the PowerShell side
degrades to its literals and says so. `find_bash` has its own fixtures in
`test-installer-bash-resolution.py`.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from collections.abc import Callable, Mapping
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
WORKFLOW = ROOT / ".github" / "workflows" / "release.yml"
INSTALL_SH = ROOT / "scripts" / "install.sh"
INSTALL_PS1 = ROOT / "scripts" / "install.ps1"
INSTALL_BAT = ROOT / "scripts" / "install.bat"


class Result:
    def __init__(self) -> None:
        self.checks = 0
        self.failures: list[str] = []

    def ok(self, message: str) -> None:
        self.checks += 1
        print(f"   ok  {message}")

    def fail(self, message: str) -> None:
        self.checks += 1
        self.failures.append(message)
        print(f"   FAILED {message}")

    def note(self, message: str) -> None:
        print(f"   note  {message}")


def published_assets(res: Result) -> set[str]:
    """The third word of every `copy_one <artifact> <bin> <dest>` line."""
    text = WORKFLOW.read_text(encoding="utf-8")
    names = []
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped.startswith("copy_one "):
            continue
        words = stripped.split()
        if len(words) >= 4:
            names.append(words[3])
    if len(names) != 6:
        res.fail(f"release.yml publishes {len(names)} assets, expected 6: {names}")
    else:
        res.ok(f"release.yml publishes {len(names)} assets: {', '.join(sorted(names))}")
    if len(set(names)) != len(names):
        res.fail(f"release.yml has duplicate asset names: {names}")
    return set(names)


def bash_runs_commands(path: str) -> bool:
    """True if `path` is an executable that runs a bash one-liner and says `ok`."""
    try:
        proc = subprocess.run(
            [path, "-c", "printf ok"], capture_output=True, text=True, timeout=30, check=False
        )
    except (OSError, subprocess.SubprocessError):
        return False
    return proc.returncode == 0 and proc.stdout.strip() == "ok"


# Where Git for Windows puts a real bash, by the environment variable naming its root.
GIT_FOR_WINDOWS_BASH = (
    ("ProgramFiles", ("Git", "bin", "bash.exe")),
    ("ProgramFiles(x86)", ("Git", "bin", "bash.exe")),
    ("LOCALAPPDATA", ("Programs", "Git", "bin", "bash.exe")),
)


def find_bash(
    environ: Mapping[str, str] | None = None,
    sep: str = os.sep,
    which: Callable[[str], str | None] = shutil.which,
) -> str | None:
    """The first bash on this host that actually runs a command, or None.

    Order is deliberate. `$BASH` first, because in a `shell: bash` workflow step that is
    the interpreter the step is running: the bash the leg was configured for. Then Git
    for Windows by its install root -- `PATH` on a Windows runner lists
    `C:\\Windows\\System32` before `C:\\Program Files\\Git\\bin`, and the System32 entry
    is the WSL launcher, which exits non-zero with its complaint on stdout when no
    distro is installed. Anything under System32 is dropped outright. Every remaining
    candidate is *executed*, because a path that exists is not the same as a bash.
    """
    env = os.environ if environ is None else environ
    on_windows = sep == "\\"
    candidates: list[str] = []
    if env.get("BASH"):
        candidates.append(env["BASH"])
    if on_windows:
        candidates.extend(
            os.path.join(env[root], *parts)
            for root, parts in GIT_FOR_WINDOWS_BASH
            if env.get(root)
        )
    from_path = which("bash")
    if from_path:
        candidates.append(from_path)
    for candidate in candidates:
        if on_windows and "system32" in candidate.lower():
            continue
        if os.path.isfile(candidate) and bash_runs_commands(candidate):
            return candidate
    return None


# (uname -s, uname -m, ASSET, PLATFORM, error message) for each arm detect_platform
# can take. The last three rows must be refused instead of naming an asset; the
# Windows row is what a user gets if they curl install.sh from Git Bash, and its
# message is the contract test-installer-asset-names.py and install.ps1 both rely on.
PROBE_ARMS: tuple[tuple[str, str, str | None, str | None, str], ...] = (
    ("Linux", "x86_64", "chaos-linux-x64", "linux-x86_64", ""),
    ("Linux", "aarch64", "chaos-linux-arm64", "linux-aarch64", ""),
    ("Darwin", "x86_64", "chaos-darwin-x64", "macos-x86_64", ""),
    ("Darwin", "arm64", "chaos-darwin-arm64", "macos-aarch64", ""),
    ("MINGW64_NT-10.0", "x86_64", None, None, "use PowerShell"),
    ("FreeBSD", "x86_64", None, None, "unsupported OS"),
    ("Linux", "ppc64le", None, None, "unsupported arch"),
)


def run_detect_platform(
    bash: str, body: str, uname_s: str, uname_m: str
) -> subprocess.CompletedProcess[str]:
    """Run the shipped `detect_platform` with `uname` answered by a stub.

    The stub is created by bash itself: python's temp dir is a DOS path on Git for
    Windows, and a `PATH` entry built from it would not resolve inside bash.
    """
    script = (
        'stub_dir="$(mktemp -d)"\n'
        'trap \'rm -rf "$stub_dir"\' EXIT\n'
        'cat > "$stub_dir/uname" <<"STUB"\n'
        "#!/bin/sh\n"
        'case "$1" in\n'
        f'  -s) echo "{uname_s}" ;;\n'
        f'  -m) echo "{uname_m}" ;;\n'
        "esac\n"
        "STUB\n"
        'chmod +x "$stub_dir/uname"\n'
        'PATH="$stub_dir:$PATH"\n'
        f"{body}\n"
        "detect_platform\n"
        "printf '%s\\t%s' \"$ASSET\" \"$PLATFORM\"\n"
    )
    return subprocess.run(
        [bash, "-c", script], capture_output=True, text=True, timeout=60, check=False
    )


def extract_shell_function(text: str, name: str) -> str:
    """The shipped text of a top-level bash function, brace counted."""
    lines = text.splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.startswith(f"{name}() {{"))
    except StopIteration:
        raise SystemExit(f"cannot find {name}() in {INSTALL_SH}")
    depth = 0
    for i in range(start, len(lines)):
        depth += lines[i].count("{") - lines[i].count("}")
        if depth == 0 and i > start:
            return "\n".join(lines[start : i + 1])
    raise SystemExit(f"unbalanced braces in {name}()")


def extract_ps_function(text: str, name: str) -> str:
    """The shipped text of a top-level PowerShell function, up to its column-0 brace."""
    lines = text.splitlines()
    try:
        start = next(
            i
            for i, line in enumerate(lines)
            if re.match(rf"^function {re.escape(name)}\s*\{{?\s*$", line)
        )
    except StopIteration:
        raise SystemExit(f"cannot find function {name} in {INSTALL_PS1}")
    for i in range(start + 1, len(lines)):
        if lines[i] == "}":
            return "\n".join(lines[start : i + 1])
    raise SystemExit(f"unbalanced braces in function {name}")


def run_shipped(bash: str, body: str) -> subprocess.CompletedProcess[str]:
    """Run the shipped `detect_platform` with this host's own `uname`."""
    return subprocess.run(
        [bash, "-c", f"{body}\ndetect_platform\nprintf '%s' \"$ASSET\""],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )


def check_install_sh(res: Result, published: set[str]) -> None:
    text = INSTALL_SH.read_text(encoding="utf-8")
    body = extract_shell_function(text, "detect_platform")
    os_keys = sorted(set(re.findall(r"\bOS_KEY=(\w+)", body)))
    arch_keys = sorted(set(re.findall(r"\bARCH_KEY=(\w+)", body)))
    combos = [f"chaos-{os_key}-{arch}" for os_key in os_keys for arch in arch_keys]
    if not os_keys or not arch_keys:
        res.fail(f"could not read OS_KEY/ARCH_KEY from detect_platform: {os_keys} / {arch_keys}")
        return
    missing = [name for name in combos if name not in published]
    if missing:
        res.fail(f"install.sh can ask for {missing}, which release.yml does not publish")
    else:
        res.ok(
            f"install.sh's {len(combos)} possible assets "
            f"({', '.join(sorted(combos))}) are all published"
        )

    # The parse above reads the assignments; it cannot tell whether a pattern in the case
    # statement matches anything real. Each arm is executed instead, and the arms are
    # cross-checked against the parse so a branch added to the installer without a row
    # below is a failure rather than a silent gap in the probe table.
    probed = {asset.split("-")[1] for _, _, asset, _, _ in PROBE_ARMS if asset}
    unprobed = [key for key in os_keys if key not in probed]
    if unprobed:
        res.fail(f"detect_platform assigns OS_KEY {unprobed}, which no PROBE_ARMS row drives")

    bash = find_bash()
    if not bash:
        message = (
            "no usable bash on this host ($BASH, Git for Windows, then PATH); "
            "detect_platform was not executed"
        )
        if os.name == "nt":
            res.note(message + " -- install.sh is not the Windows install path, so its names were compared as text")
        else:
            res.fail(message)
        return
    res.ok(f"executing install.sh's detect_platform with {bash}")

    named: list[str] = []
    refused: list[str] = []
    for uname_s, uname_m, want_asset, want_platform, want_error in PROBE_ARMS:
        proc = run_detect_platform(bash, body, uname_s, uname_m)
        label = f"detect_platform on {uname_s}/{uname_m}"
        out = proc.stdout.strip()
        err = proc.stderr.strip()
        if want_asset is None:
            if proc.returncode == 0:
                res.fail(f"{label} should have been refused; it picked {out!r}")
            elif want_error not in err:
                res.fail(f"{label} exited {proc.returncode} without {want_error!r}: {(err or out)[:200]!r}")
            else:
                refused.append(uname_s)
            continue
        asset, _, platform = out.partition("\t")
        asset, platform = asset.strip(), platform.strip()
        if proc.returncode != 0:
            res.fail(f"{label} exited {proc.returncode}: {(err or out)[:200]!r}")
        elif (asset, platform) != (want_asset, want_platform):
            res.fail(
                f"{label} picked {asset!r} stored as {platform!r}, "
                f"expected {want_asset!r} stored as {want_platform!r}"
            )
        elif asset not in published:
            res.fail(f"{label} picked {asset!r}, which release.yml does not publish")
        else:
            named.append(f"{uname_s}/{uname_m}->{asset}")
    if len(named) == sum(1 for arm in PROBE_ARMS if arm[2]):
        res.ok(f"every asset arm names a published asset when run: {', '.join(named)}")
    if len(refused) == sum(1 for arm in PROBE_ARMS if arm[2] is None):
        res.ok(f"and is refused as designed on {', '.join(refused)}")

    # Once more with the host's untouched `uname`, so the check is not only about stubs.
    host = run_shipped(bash, body)
    if host.returncode != 0:
        # Refusing Windows is the shipped behaviour -- install.sh tells that user to run
        # install.ps1 -- and the Windows leg of the platform matrix runs this very file
        # under Git Bash. The arm table above already proved the refusal's wording.
        if "use PowerShell" in host.stderr:
            res.note("this host's uname is a Windows one, so detect_platform refuses, as designed")
        else:
            res.fail(
                f"running detect_platform here exited {host.returncode}: "
                f"{(host.stderr.strip() or host.stdout.strip())[:200]!r}"
            )
    elif host.stdout.strip() not in published:
        res.fail(
            f"this host's detect_platform picked {host.stdout.strip()!r}, "
            "which release.yml does not publish"
        )
    else:
        res.ok(f"this host's own uname picks {host.stdout.strip()!r}, which is published")


def check_install_ps1(res: Result, published: set[str], require_pwsh: bool) -> None:
    text = INSTALL_PS1.read_text(encoding="utf-8")
    body = extract_ps_function(text, "Get-AssetName")
    literals = sorted(set(re.findall(r'"(chaos-[^"]+)"', body)))
    if not literals:
        res.fail("no chaos-* asset literals inside Get-AssetName; did it change shape?")
        return
    missing = [name for name in literals if name not in published]
    if missing:
        res.fail(f"install.ps1 can ask for {missing}, which release.yml does not publish")
    else:
        res.ok(f"install.ps1's asset literals ({', '.join(literals)}) are all published")

    pwsh = shutil.which("pwsh")
    if not pwsh:
        if require_pwsh:
            res.fail("--require was passed but no pwsh is on PATH")
        else:
            res.note("no pwsh on PATH; install.ps1 was checked by literals only")
        return
    proc = subprocess.run(
        [pwsh, "-NoProfile", "-NonInteractive", "-Command", f"{body}\nGet-AssetName"],
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    asset = proc.stdout.strip()
    if proc.returncode != 0:
        res.fail(f"running the shipped Get-AssetName failed: {proc.stderr.strip()[:200]}")
    elif asset not in published:
        res.fail(f"Get-AssetName returned '{asset}' here, which release.yml does not publish")
    else:
        res.ok(f"the shipped Get-AssetName returns '{asset}' under pwsh, and it is published")


def check_install_bat(res: Result, published: set[str]) -> None:
    text = INSTALL_BAT.read_text(encoding="utf-8")
    literals = sorted(set(re.findall(r'set\s+"ASSET=(chaos-[^"]+)"', text)))
    if len(literals) < 2:
        res.fail(f"expected x64 and arm64 ASSET lines in install.bat, found {literals}")
        return
    missing = [name for name in literals if name not in published]
    if missing:
        res.fail(f"install.bat can ask for {missing}, which release.yml does not publish")
    else:
        res.ok(f"install.bat's asset names ({', '.join(literals)}) are all published")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--require",
        action="store_true",
        help="fail instead of skipping the executed PowerShell check when pwsh is absent",
    )
    args = parser.parse_args()

    res = Result()
    print("== the asset a release publishes, and the four places that ask for it")
    published = published_assets(res)
    check_install_sh(res, published)
    check_install_ps1(res, published, args.require)
    check_install_bat(res, published)

    print(f"\n{res.checks} check(s), {len(res.failures)} failure(s)")
    for failure in res.failures:
        print(f"  - {failure}")
    return 1 if res.failures else 0


if __name__ == "__main__":
    sys.exit(main())
