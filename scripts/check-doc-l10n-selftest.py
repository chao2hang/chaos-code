#!/usr/bin/env python3
"""Adversarial self-test for scripts/check-doc-l10n.py.

The gate is what protects the user guide while it is being translated, so the
gate itself needs a test: a check that silently passes damage is worse than no
check at all. Each case names the invariant it exercises and the direction it
must decide; a mutation that does not apply to the real file is a FAILURE, not
a skip, so no case can quietly stop testing anything.

Run:  python3 scripts/check-doc-l10n-selftest.py
"""
from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
GUIDE = REPO / "crates/codegen/xai-grok-pager/docs/user-guide"
GATE = str(REPO / "scripts/check-doc-l10n.py")

TARGET = "26-config-reference.md"
PLUGINS = "09-plugins.md"


def renamed(s: str) -> str:
    """The intended fork localization: same meaning, fork spelling."""
    return (s.replace("~/.grok/", "~/.chaos/")
             .replace("`grok inspect`", "`chaos inspect`")
             .replace("`$GROK_HOME/managed_config.toml`",
                      "`$CHAOS_HOME/managed_config.toml`"))


def sub(old: str, new: str, count: int = 1):
    def mutate(s: str) -> str:
        assert old in s, f"mutation target absent: {old!r}"
        return s.replace(old, new, count)
    return mutate


# (name, file, mutate, expect_drift, extra_args)
CASES = (
    ("legit fork rename (fences + inline)", TARGET, renamed, False, ()),
    ("inline: dropped backticks", TARGET,
     sub("Run `grok inspect` or", "Run grok inspect or"), True, ()),
    ("inline: invented flag", TARGET,
     sub("Run `grok inspect` or", "Run `chaos inspect --all` or"), True, ()),
    ("inline: added span is a note", TARGET,
     sub("Run `grok inspect` or", "Run `grok inspect` or see `/provider` or"),
     False, ()),
    ("inline: added span with --strict-spans", TARGET,
     sub("Run `grok inspect` or", "Run `grok inspect` or see `/provider` or"),
     True, ("--strict-spans",)),
    ("env var: forged CHAOS twin", TARGET,
     sub("`GROK_CONFIG` (inline JSON)", "`CHAOS_CONFIG` (inline JSON)"), True, ()),
    ("env var: allowed twin GROK_HOME", TARGET,
     sub("`$GROK_HOME/config.toml` (your settings",
         "`$CHAOS_HOME/config.toml` (your settings)"), False, ()),
    ("fence: flag typo", PLUGINS,
     sub("grok plugin list", "grok plugin ls"), True, ()),
    ("fence: legit command rename", PLUGINS,
     sub("grok plugin list", "chaos plugin list"), False, ()),
    ("literal: /etc/grok is not renamed", TARGET,
     sub("`/etc/grok/managed_config.toml`", "`/etc/chaos/managed_config.toml`"),
     True, ()),
    ("table: extra cell in a data row", TARGET,
     sub("| `config.toml` | The developer |",
         "| `config.toml` | The developer | x |"), True, ()),
    ("link: changed anchor", TARGET,
     sub("](05-configuration.md)", "](05-configuration.md#bogus)"), True, ()),
    ("heading: changed level", TARGET, sub("\n## ", "\n### "), True, ()),
    ("numbers: changed default", TARGET,
     sub("RFC 3339 UTC timestamp", "RFC 3338 UTC timestamp"), True, ()),
)


def run_case(name: str, fname: str, mutate, expect_drift: bool,
             extra_args: tuple[str, ...] = ()) -> int:
    src = GUIDE / fname
    before = src.read_text(encoding="utf-8")
    rel = f"docs/user-guide/{fname}"

    tmp = Path(tempfile.mkdtemp())
    target = tmp / rel
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(before, encoding="utf-8")

    def git(*a: str):
        return subprocess.run(["git", *a], cwd=tmp, capture_output=True,
                              text=True)

    git("init", "-q")
    git("config", "user.email", "t@t")
    git("config", "user.name", "t")
    git("add", "-A")
    git("commit", "-qm", "before")

    try:
        after = mutate(before)
    except AssertionError as err:
        print(f"FAIL {name}: {err}")
        return 1
    if after == before:
        print(f"FAIL {name}: mutation was a no-op")
        return 1

    target.write_text(after, encoding="utf-8")
    proc = subprocess.run(
        [sys.executable, GATE, "--before", "HEAD", "--after", "WORKTREE",
         "--glob", rel, *extra_args],
        cwd=tmp, capture_output=True, text=True)
    drifted = proc.returncode != 0
    if drifted == expect_drift:
        print(f"ok   {name}: drift={drifted} (expected {expect_drift})")
        return 0
    print(f"FAIL {name}: drift={drifted} but expected {expect_drift}")
    print("     " + proc.stdout.strip().replace("\n", "\n     ")[:500])
    return 1


def main() -> int:
    failures = sum(run_case(*case) for case in CASES)
    print(f"\n{len(CASES) - failures}/{len(CASES)} self-test case(s) passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
