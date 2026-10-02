#!/usr/bin/env python3
"""Fail when "the version" stops meaning one thing.

Why this exists: a release carries the number in four places that are edited by
three different mechanisms.

- `crates/codegen/xai-grok-pager/npm/chaos/package.json` is what
  `release.yml`'s `resolve-version` step reads when no version was typed into
  the workflow dispatch, so it decides the tag;
- the six `npm/chaos-<platform>/package.json` files and the meta package's
  `optionalDependencies` pins decide what `npm install chaos-code` resolves to;
- `crates/codegen/xai-grok-pager/Cargo.toml` and
  `xai-grok-pager-bin/Cargo.toml` are the crates whose build produces the binary
  whose `--version` line the auto-updater compares against a release feed;
- the desktop, web and engine crates carry their own numbers, which upstream
  bumped independently and which are not the product version at all.

Nothing in the tree said which of these is the product version, so the same
commit could be released twice and produce two numbers -- a tag from one file, a
`chaos --version` line from another, and an updater that compares the two and
calls an upgrade a downgrade. This check pins the answer: the npm meta package is
the release version, the binary's crates and the platform packages must equal it,
and anything allowed to differ has to be listed here *and* named in
CONTRIBUTING.md so the exception is written down somewhere a human reads.

`--published` adds the registry side: what npm actually serves for those names.
That half needs network, so it is opt-in; it is the check to run before a
release, because npm resolving a name to a security placeholder is not something
publishing fixes.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
NPM_ROOT = ROOT / "crates" / "codegen" / "xai-grok-pager" / "npm"
META = NPM_ROOT / "chaos" / "package.json"
PLATFORMS = ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64", "win32-arm64", "win32-x64"]

# Crates whose build output is the released artifact: their version is printed by
# `chaos --version` and compared by the auto-updater, so a drift from the release
# version is a user-visible contradiction, not a bookkeeping detail.
SHIPPED_VERSION_CRATES = ["xai-grok-pager", "xai-grok-pager-bin"]

# Crates deliberately outside the release version. Each has to appear in
# CONTRIBUTING.md as well -- an exception nobody can read about is how the next
# person "fixes" it by bumping one file.
INDEPENDENT_VERSION_CRATES = {
    "xai-grok-web": "served assets; versioned by its own bundle",
    "xai-grok-desktop": "native shell; versioned by its own bundle",
    "chaos-engine": "engine protocol crate; versioned by the protocol it speaks",
    "xai-grok-update": "updater; version numbers inherited from upstream",
}

REGISTRY = "https://registry.npmjs.org"


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def cargo_version(crate: str) -> str:
    """The `version = "..."` of a workspace crate, without pulling in toml deps."""
    text = (ROOT / "crates" / "codegen" / crate / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if not match:
        raise SystemExit(f"{crate}: no `version = ` line in Cargo.toml")
    return match.group(1)


def check_release_yml(res: "Result") -> None:
    """The version has exactly one source in the workflow, and it is the meta package."""
    text = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
    expected = "require('./crates/codegen/xai-grok-pager/npm/chaos/package.json').version"
    if expected in text:
        res.ok("release.yml resolves the version from the npm meta package")
    else:
        res.fail(
            "release.yml no longer reads the version from "
            "crates/codegen/xai-grok-pager/npm/chaos/package.json; if the source moved, "
            "update this check and CONTRIBUTING.md together"
        )
    if text.count(expected) > 1:
        res.fail("release.yml reads the release version from the meta package more than once")


def check_meta(res: "Result") -> str:
    meta = read_json(META)
    version = meta.get("version", "")
    if re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?", version):
        res.ok(f"{META.relative_to(ROOT)} version {version} is a plain semver")
    else:
        res.fail(f"{META.relative_to(ROOT)} version {version!r} is not a release version")
    return version


def check_platform_packages(res: "Result", version: str) -> None:
    for platform in PLATFORMS:
        path = NPM_ROOT / f"chaos-{platform}" / "package.json"
        if not path.is_file():
            res.fail(f"{path.relative_to(ROOT)} is missing; the release job publishes it")
            continue
        pkg = read_json(path)
        if pkg.get("version") != version:
            res.fail(
                f"{path.relative_to(ROOT)} carries {pkg.get('version')!r}, "
                f"release version is {version!r}"
            )
    if all((NPM_ROOT / f"chaos-{p}" / "package.json").is_file() for p in PLATFORMS):
        res.ok("all six platform packages carry the release version")


def check_optional_pins(res: "Result", version: str) -> None:
    pins = read_json(META).get("optionalDependencies", {})
    expected = {f"chaos-code-{p}" for p in PLATFORMS}
    missing = sorted(expected - set(pins))
    extra = sorted(set(pins) - expected)
    if missing:
        res.fail(f"meta package does not depend on: {', '.join(missing)}")
    if extra:
        res.fail(f"meta package depends on unknown platform packages: {', '.join(extra)}")
    wrong = {name: pin for name, pin in pins.items() if pin != version}
    if wrong:
        detail = ", ".join(f"{name}@{pin}" for name, pin in sorted(wrong.items()))
        res.fail(
            f"optionalDependencies pins must equal the release version {version}: {detail}"
        )
    if not missing and not extra and not wrong:
        res.ok(f"six optionalDependencies pins all equal {version}")


def check_shipped_crates(res: "Result", version: str) -> None:
    for crate in SHIPPED_VERSION_CRATES:
        got = cargo_version(crate)
        if got != version:
            res.fail(
                f"crates/codegen/{crate}/Cargo.toml says {got!r} but the release version is "
                f"{version!r}; this crate builds the binary whose --version the updater compares"
            )
    if all(cargo_version(c) == version for c in SHIPPED_VERSION_CRATES):
        res.ok(f"{' and '.join(SHIPPED_VERSION_CRATES)} match the release version")


def check_independent_are_documented(res: "Result") -> None:
    contributing = (ROOT / "CONTRIBUTING.md").read_text(encoding="utf-8")
    undocumented = [
        crate for crate in INDEPENDENT_VERSION_CRATES if f"`{crate}`" not in contributing
    ]
    for crate in undocumented:
        res.fail(
            f"{crate} is exempted from the release version in this script but is not named "
            f"in CONTRIBUTING.md; write the exception down or bring the crate into lockstep"
        )
    if not undocumented:
        res.ok(f"{len(INDEPENDENT_VERSION_CRATES)} independently versioned crates are documented")
    for crate, why in INDEPENDENT_VERSION_CRATES.items():
        res.note(f"{crate:<18} {cargo_version(crate):<18} {why}")


def check_changelog(res: "Result", version: str) -> None:
    text = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    if re.search(rf"^## {re.escape(version)}\b", text, re.MULTILINE):
        res.ok(f"CHANGELOG.md describes {version}")
    else:
        res.fail(
            f"CHANGELOG.md has no `## {version}` section; the version being released must be "
            "described before it is tagged"
        )


def check_published(res: "Result", version: str) -> None:
    """What npm actually serves. Needs network; the reserved-name case is a blocker
    that publishing does not fix, so it fails, while a name whose latest is simply
    behind the repository is the normal state between releases."""
    names = ["chaos-code"] + [f"chaos-code-{p}" for p in PLATFORMS]
    for name in names:
        try:
            out = subprocess.run(
                ["npm", "view", name, "versions", "--json", "--registry", REGISTRY],
                capture_output=True,
                text=True,
                timeout=90,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as exc:
            res.fail(f"npm view {name} did not run: {exc}")
            continue
        if out.returncode != 0:
            res.fail(f"npm view {name} exited {out.returncode}: {out.stderr.strip()[:120]}")
            continue
        try:
            published = json.loads(out.stdout)
        except json.JSONDecodeError:
            res.fail(f"npm view {name} returned something that is not JSON: {out.stdout[:80]!r}")
            continue
        if isinstance(published, str):
            published = [published]
        if version in published:
            res.ok(f"{name}@{version} is published")
            continue
        usable = [
            v for v in published if not v.startswith("0.0.1-security")
        ]
        if not usable:
            res.fail(
                f"{name} has no publishable history on npm (published: {', '.join(published) or 'nothing'}); "
                "npm holds the name for a security placeholder, so `npm install` of this "
                "platform package can never resolve"
            )
        else:
            highest = max(usable, key=version_key)
            res.note(f"{name}: published latest {highest}, repository is at {version}")


def version_key(text: str) -> tuple[int, ...]:
    """Order published versions without a semver dependency.

    Only the numeric core matters here: the question is "what is the newest thing
    npm actually has", and a prerelease tag or a build string does not change that
    answer for the names this check looks at.
    """
    core = re.split(r"[-+]", text, maxsplit=1)[0]
    return tuple(int(part) if part.isdigit() else 0 for part in core.split("."))


class Result:
    def __init__(self) -> None:
        self.checks = 0
        self.failures: list[str] = []

    def ok(self, text: str) -> None:
        self.checks += 1
        print(f"   ok    {text}")

    def fail(self, text: str) -> None:
        self.checks += 1
        self.failures.append(text)
        print(f"   FAILED {text}")

    def note(self, text: str) -> None:
        print(f"   note  {text}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--published",
        action="store_true",
        help="also ask npm registry what it serves for these names (needs network)",
    )
    args = parser.parse_args()

    res = Result()
    print("== the release version and everything that must equal it")
    check_release_yml(res)
    version = check_meta(res)
    check_platform_packages(res, version)
    check_optional_pins(res, version)
    check_shipped_crates(res, version)
    check_changelog(res, version)

    print("\n== crates that are deliberately not the release version")
    check_independent_are_documented(res)

    if args.published:
        print("\n== what npm serves")
        check_published(res, version)

    print(f"\n{res.checks} check(s), {len(res.failures)} failure(s)")
    for failure in res.failures:
        print(f"  - {failure}")
    return 1 if res.failures else 0


if __name__ == "__main__":
    sys.exit(main())
