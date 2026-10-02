#!/usr/bin/env python3
"""Parse every PowerShell script in the repository with a real PowerShell.

`scripts/install.ps1` is one of the two commands the README tells a Windows
user to run, and for a stretch of history it could not be run at all: commit
21f5a186 restructured the signature block and left a stray closing brace, so
`irm .../install.ps1 | iex` died on "The Try statement is missing its Catch or
Finally block" before downloading anything. Nothing noticed, because the
installer is not executed by any Linux job and the macOS/Windows legs build and
test the Rust workspace without touching the installers.

A parse check is the cheapest gate that would have caught it. It is not a
substitute for running the installer -- that needs a real Windows, and no job
here does that; the equivalent for `scripts/install.sh` is
`scripts/install-sh-in-docker.sh`, which installs the real release in a clean
Debian container. But a script that does not parse cannot do anything else
either, and this half costs a second.

The two files under `crates/codegen/xai-grok-pager/scripts/` are upstream Grok
installers kept for reference (`https://x.ai/cli/*.ps1`, `GROK_*` variables);
they are not part of this fork's release path and are not covered by the
signing contract in `docs/release-signing.md`. This gate parses them too, which
is all it can honestly claim about them.

`--require` turns "no PowerShell on PATH" into a failure. CI passes it on the
legs where PowerShell is guaranteed to exist, so the gate cannot quiet itself by
running somewhere without `pwsh`.
"""

import argparse
import os
import pathlib
import shutil
import subprocess
import sys

PARSER_SCRIPT = """
$errs = $null
$path = (Resolve-Path $env:CHAOS_PS_TARGET).Path
[System.Management.Automation.Language.Parser]::ParseFile($path, [ref]$null, [ref]$errs) | Out-Null
foreach ($e in $errs) {
    Write-Output ('{0}: {1}' -f $e.Extent.StartLineNumber, $e.Message)
}
exit ([int][bool]$errs)
"""


def powershell():
    for name in ("pwsh", "pwsh-lts", "powershell", "powershell.exe"):
        found = shutil.which(name)
        if found:
            return found
    return None


def tracked_scripts():
    listed = subprocess.run(
        ["git", "ls-files", "-z", "--", "*.ps1"],
        capture_output=True,
        check=True,
    )
    return sorted(pathlib.Path(p) for p in listed.stdout.decode().split("\0") if p)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--require", action="store_true",
                    help="fail instead of skipping when PowerShell is missing")
    args = ap.parse_args()

    exe = powershell()
    if exe is None:
        msg = "powershell-syntax: SKIPPED (no pwsh/powershell on PATH)"
        if args.require:
            print(msg.replace("SKIPPED", "FAILED"), file=sys.stderr)
            print("  this leg is supposed to have PowerShell", file=sys.stderr)
            return 1
        print(msg)
        return 0

    scripts = tracked_scripts()
    if not scripts:
        print("powershell-syntax: FAILED (git ls-files found no *.ps1 at all)",
              file=sys.stderr)
        return 1

    failures = 0
    for script in scripts:
        proc = subprocess.run(
            [exe, "-NoProfile", "-NonInteractive", "-Command", PARSER_SCRIPT],
            capture_output=True,
            text=True,
            # Passed through the environment because `-Command` does not reliably
            # forward trailing arguments into `$args`.
            env={**os.environ, "CHAOS_PS_TARGET": str(script)},
        )
        if proc.returncode == 0:
            print(f"powershell-syntax: {script} OK")
            continue
        failures += 1
        detail = proc.stdout.strip() or proc.stderr.strip() or "no detail"
        for line in detail.splitlines():
            print(f"powershell-syntax: {script}:{line}", file=sys.stderr)

    if failures:
        print(f"powershell-syntax: {failures} of {len(scripts)} script(s) "
              "will not run", file=sys.stderr)
        return 1
    print(f"powershell-syntax: OK ({len(scripts)} script(s) parsed by {exe})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
