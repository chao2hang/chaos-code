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
parsed, so what is checked is the shipped code: bash runs `detect_platform` for this host,
pwsh runs `Get-AssetName` for this host, and the literal names in each are collected
alongside so the branches this host cannot reach (arm64) are still covered.

`--require` makes a missing `pwsh` a failure instead of a skip, the way
`check-powershell-syntax.py` does on the platform legs. Without it, the PowerShell side
degrades to its literals and says so.
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
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

    # Now run the real thing for this host, rather than trusting the parse above.
    probe = f"{body}\ndetect_platform\nprintf '%s' \"$ASSET\"\n"
    proc = subprocess.run(
        ["bash", "-c", probe], capture_output=True, text=True, timeout=30, check=False
    )
    asset = proc.stdout.strip()
    if proc.returncode != 0:
        # detect_platform exits 1 on a Windows host on purpose -- install.sh tells the
        # user to run install.ps1 -- and the platform legs run this file under Git Bash.
        # Refusing there is the shipped behaviour, not a defect.
        if "use PowerShell" in proc.stderr:
            res.note("detect_platform refuses Windows by design; the executed probe is a Linux/macOS path")
        else:
            res.fail(f"running the shipped detect_platform failed: {proc.stderr.strip()[:200]}")
    elif asset not in published:
        res.fail(f"this host's detect_platform picked '{asset}', which release.yml does not publish")
    else:
        res.ok(f"the shipped detect_platform picks '{asset}' for this host, and it is published")


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
