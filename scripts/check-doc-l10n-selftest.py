#!/usr/bin/env python3
"""Adversarial self-test for scripts/check-doc-l10n.py.

The gate is what protects the user guide while it is being translated, so the
gate itself needs a test: a check that silently passes damage is worse than no
check at all. Each case names the invariant it exercises and the direction it
must decide; a mutation that does not apply is a FAILURE, not a skip, so no
case can quietly stop testing anything.

The structural cases run against `FIXTURE`, not against a chapter. A chapter
is the thing under translation, so a case anchored in one rots the moment its
target sentence is translated; the fixture carries the same features (a fence,
inline spans, a five-column table, a link, headings, prose numbers) and stays
put. The cell cases run the worktree modes against throwaway documents.

Run:  python3 scripts/check-doc-l10n-selftest.py
"""
from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
GATE = str(REPO / "scripts/check-doc-l10n.py")

FIXTURE = """# Configuration reference

运行 `grok inspect` 或 `grok inspect --json`，查看哪些文件与值最终生效。
This paragraph still names `GROK_CONFIG` (inline JSON) and
`$GROK_HOME/config.toml` (your settings).

```sh
grok plugin list
  # comment-only line keeps its indentation
~/.grok/config.toml       # Global settings
+-----------------------------+
|          grok plugin list   |
+-----------------------------+
tail -f /tmp/grok.log
# flags after -- are not parsed by grok.
# the hosted portal is grok.com, not the local binary
```

| Key | Type / Values | Requirements | Managed | Details |
| --- | --- | --- | --- | --- |
| `config.toml` | `string` | `yes` | `user` | Set personal defaults for this machine. |
| `agent.name` | `string` | `yes` | `user` | Built-in or discovered agent definition name. |

路径 `/etc/grok/managed_config.toml`：RFC 3339 UTC timestamp。详见
[Configuration](05-configuration.md) 与 [Hooks](10-hooks.md#when-it-fires)。

## How to configure

正文段落。
"""

REL = "docs/user-guide/26-config-reference.md"


def renamed(s: str) -> str:
    """The intended fork localization: same meaning, fork spelling."""
    return (s.replace("`grok inspect`", "`chaos inspect`")
             .replace("$GROK_HOME", "$CHAOS_HOME")
             .replace("~/.grok/config.toml", "~/.chaos/config.toml")
             .replace("grok plugin list", "chaos plugin list")
             .replace("parsed by grok.", "parsed by chaos."))


def sub(old: str, new: str, count: int = 1):
    def mutate(s: str) -> str:
        assert old in s, f"mutation target absent: {old!r}"
        return s.replace(old, new, count)
    return mutate


# (name, mutate, expect_drift, extra_args)
CASES = (
    ("fork rename: command, env var, fence", renamed, False, ()),
    ("inline: dropped backticks", sub("`grok inspect`", "grok inspect"), True, ()),
    ("inline: invented flag", sub("`grok inspect`", "`chaos inspect --all`"),
     True, ()),
    ("inline: added span is a note",
     sub("`grok inspect`", "`grok inspect` 或 `/provider`"), False, ()),
    ("inline: added span with --strict-spans",
     sub("`grok inspect`", "`grok inspect` 或 `/provider`"), True,
     ("--strict-spans",)),
    ("env var: forged CHAOS twin",
     sub("`GROK_CONFIG`", "`CHAOS_CONFIG`"), True, ()),
    ("env var: allowed twin GROK_HOME",
     sub("$GROK_HOME/config.toml", "$CHAOS_HOME/config.toml"), False, ()),
    ("literal: /etc/grok is never renamed",
     sub("`/etc/grok/managed_config.toml`", "`/etc/chaos/managed_config.toml`"),
     True, ()),
    ("fence: flag typo", sub("grok plugin list", "grok plugin ls"), True, ()),
    ("fence: legit command rename",
     sub("grok plugin list", "chaos plugin list"), False, ()),
    ("fence: comment padding is cosmetic after a rename",
     sub("~/.grok/config.toml       # Global settings",
         "~/.chaos/config.toml    # Global settings"), False, ()),
    ("fence: comment text is still compared",
     sub("~/.grok/config.toml       # Global settings",
         "~/.grok/config.toml       # Global preferences"), True, ()),
    ("fence: comment-only line keeps its indentation",
     sub("\n  # comment-only line", "\n    # comment-only line"), True, ()),
    ("fence: ASCII box border is re-aligned by a rename",
     sub("|          grok plugin list   |",
         "|          chaos plugin list  |"), False, ()),
    ("fence: ASCII box content is still compared",
     sub("|          grok plugin list   |",
         "|          grok plugin lx     |"), True, ()),
    ("fence: a sentence-final command name is renamed",
     sub("parsed by grok.", "parsed by chaos."), False, ()),
    ("fence: the upstream service is not renamed",
     sub("grok.com", "chaos.com"), True, ()),
    ("fence: a name inside a path is left alone",
     sub("/tmp/grok.log", "/tmp/chaos.log"), True, ()),
    ("table: extra cell in a data row",
     sub("| `config.toml` | `string` | `yes` |", "| `config.toml` | `string` | `yes` | x |"),
     True, ()),
    ("table: dropped column from the separator",
     sub("| --- | --- | --- | --- | --- |", "| --- | --- | --- | --- |"), True, ()),
    ("link: changed anchor", sub("](10-hooks.md#when-it-fires)",
                                 "](10-hooks.md#when-it-fired)"), True, ()),
    ("link: dropped target", sub(" 与 [Hooks](10-hooks.md#when-it-fires)", ""),
     True, ()),
    ("heading: changed level", sub("\n## ", "\n### "), True, ()),
    ("numbers: changed default", sub("RFC 3339", "RFC 3338"), True, ()),
)


def run_case(name: str, mutate, expect_drift: bool,
             extra_args: tuple[str, ...] = ()) -> int:
    tmp = Path(tempfile.mkdtemp())
    target = tmp / REL
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(FIXTURE, encoding="utf-8")

    def git(*a: str):
        return subprocess.run(["git", *a], cwd=tmp, capture_output=True,
                              text=True)

    git("init", "-q")
    git("config", "user.email", "t@t")
    git("config", "user.name", "t")
    git("add", "-A")
    git("commit", "-qm", "before")

    try:
        after = mutate(FIXTURE)
    except AssertionError as err:
        print(f"FAIL {name}: {err}")
        return 1
    if after == FIXTURE:
        print(f"FAIL {name}: mutation was a no-op")
        return 1

    target.write_text(after, encoding="utf-8")
    proc = subprocess.run(
        [sys.executable, GATE, "--before", "HEAD", "--after", "WORKTREE",
         "--glob", REL, *extra_args],
        cwd=tmp, capture_output=True, text=True)
    drifted = proc.returncode != 0
    if drifted == expect_drift:
        print(f"ok   {name}: drift={drifted} (expected {expect_drift})")
        return 0
    print(f"FAIL {name}: drift={drifted} but expected {expect_drift}")
    print("     " + proc.stdout.strip().replace("\n", "\n     ")[:500])
    return 1


# ---------------------------------------------------------------------------
# Cell cases: these run the worktree modes (`--cells`, `--apply-cell-glossary`)
# against a throwaway document, so a case states the glossary it is judged by.

TABLE = """# T

| 键 | 说明 |
| --- | --- |
| `a` | {a} |
| `b` | {b} |
"""

CELL_CASES = (
    ("cells: English prose cell is reported", TABLE.format(
        a="Set personal defaults for this machine.", b="说明"), "", False),
    ("cells: translated cell passes", TABLE.format(
        a="为本机设置个人默认值。", b="说明"), "", True),
    ("cells: short literal is reported without a decision", TABLE.format(
        a="array", b="说明"), "", False),
    ("cells: short literal allowlisted as =keep", TABLE.format(
        a="array", b="说明"), "array\t=keep\n", True),
    ("cells: short literal translated", TABLE.format(
        a="array", b="说明"), "array\t数组\n", True),
    ("cells: unlisted short literal still fails", TABLE.format(
        a="Whatever", b="说明"), "array\t=keep\n", False),
    ("cells: glossary key is matched without its padding", TABLE.format(
        a="array", b="说明"), "  array  \t=keep\n", True),
    ("cells: escaped pipe does not split a cell", TABLE.format(
        a="`array` \\| `list` or `map`", b="说明"),
     "`array` \\| `list` or `map`\t=keep\n", True),
    ("glossary: value that drops a backticked literal is refused",
     TABLE.format(a="说明", b="说明"), "`a`\t就是 a\n", False),
    ("glossary: command rename is not a dropped literal",
     TABLE.format(a="说明", b="说明"),
     "`grok inspect` 的输出\t`chaos inspect` 的输出\n", True),
    ("glossary: value with no Han characters is refused",
     TABLE.format(a="说明", b="说明"), "Yes\tyes\n", False),
    ("glossary: value with an unescaped pipe is refused",
     TABLE.format(a="说明", b="说明"), "Yes\t是 | 否\n", False),
    ("glossary: value that changes a number is refused",
     TABLE.format(a="说明", b="说明"), "30 seconds\t3 秒后\n", False),
    ("glossary: same number keeps the entry valid",
     TABLE.format(a="说明", b="说明"), "30 seconds\t30 秒\n", True),
    ("glossary: literal value passes as =keep",
     TABLE.format(a="说明", b="说明"), "Yes\t=keep\n", True),
    ("glossary: a duplicate key is refused",
     TABLE.format(a="说明", b="说明"), "Yes\t是\nYes\t否\n", False),
)

# The residue detector is the gate that decides whether the sweep is over, so
# its allowlist and its exemption need the same two-way coverage: a leftover
# upstream name must be reported, and each literal the fork deliberately kept
# must not be.
FORK_NAME_CASES = (
    ("fork names: a leftover command word is reported",
     "运行 `grok inspect` 查看。\n", False),
    ("fork names: a leftover config path is reported",
     "写入 `~/.grok/config.toml`。\n", False),
    ("fork names: a leftover bare name is reported",
     "顶层目录是 .grok。\n", False),
    ("fork names: the dual-read note may name the legacy path",
     "路径请按兼容规则把 `~/.grok` 理解为配置根。\n", True),
    ("fork names: a sentence about the upstream may name it",
     "（上游官方安装脚本安装的是 `grok`，与本 fork 无关。）\n", True),
    ("fork names: literals the fork did not rename are allowed",
     "见 `grok.com`、`xai-grok-pager`、`grok-4.5`、`/etc/grok`、"
     "`GROK_AGENT_DASHBOARD`。\n", True),
    ("fork names: a name inside a path is allowed",
     "日志写在 `/tmp/grok.log`。\n", True),
)

# Whole-cell matching: the longer cell must survive an apply untouched.
APPLY_DOC = """# T

| 键 | 说明 |
| --- | --- |
| array | 数组是定长的。 |
| `array` or a list | 说明 |
"""
APPLY_GLOSSARY = "array\t数组\n"
APPLY_EXPECT = """# T

| 键 | 说明 |
| --- | --- |
| 数组 | 数组是定长的。 |
| `array` or a list | 说明 |
"""


def run_cell_case(name: str, doc: str, glossary: str, expect_pass: bool) -> int:
    tmp = Path(tempfile.mkdtemp())
    (tmp / "doc.md").write_text(doc, encoding="utf-8")
    (tmp / "g.tsv").write_text(glossary, encoding="utf-8")
    proc = subprocess.run(
        [sys.executable, GATE, "--cells", "--strict", "--glob", "*.md",
         "--cell-glossary", "g.tsv"],
        cwd=tmp, capture_output=True, text=True)
    passed = proc.returncode == 0
    if passed == expect_pass:
        print(f"ok   {name}: pass={passed} (expected {expect_pass})")
        return 0
    print(f"FAIL {name}: pass={passed} but expected {expect_pass}")
    print("     " + proc.stdout.strip().replace("\n", "\n     ")[:500])
    return 1


def run_fork_name_case(name: str, doc: str, expect_pass: bool) -> int:
    tmp = Path(tempfile.mkdtemp())
    (tmp / "doc.md").write_text(doc, encoding="utf-8")
    proc = subprocess.run(
        [sys.executable, GATE, "--fork-names", "--strict", "--glob", "*.md"],
        cwd=tmp, capture_output=True, text=True)
    passed = proc.returncode == 0
    if passed == expect_pass:
        print(f"ok   {name}: pass={passed} (expected {expect_pass})")
        return 0
    print(f"FAIL {name}: pass={passed} but expected {expect_pass}")
    print("     " + proc.stdout.strip().replace("\n", "\n     ")[:500])
    return 1


def run_apply_case() -> int:
    name = "apply: whole-cell match only, longer cell untouched"
    tmp = Path(tempfile.mkdtemp())
    (tmp / "doc.md").write_text(APPLY_DOC, encoding="utf-8")
    (tmp / "g.tsv").write_text(APPLY_GLOSSARY, encoding="utf-8")
    proc = subprocess.run(
        [sys.executable, GATE, "--apply-cell-glossary", "--glob", "*.md",
         "--cell-glossary", "g.tsv"],
        cwd=tmp, capture_output=True, text=True)
    got = (tmp / "doc.md").read_text(encoding="utf-8")
    if proc.returncode == 0 and got == APPLY_EXPECT:
        print(f"ok   {name}: {got.splitlines()[4]}")
        return 0
    print(f"FAIL {name}: exit={proc.returncode}")
    print("     " + got.replace("\n", "\n     ")[:500])
    return 1


def run_removal_case() -> int:
    """A declared span removal is a note; an undeclared one is still drift."""
    name = "span removal: declared passes, undeclared fails"
    tmp = Path(tempfile.mkdtemp())
    target = tmp / REL
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(FIXTURE, encoding="utf-8")

    def git(*a: str):
        return subprocess.run(["git", *a], cwd=tmp, capture_output=True,
                              text=True)

    git("init", "-q")
    git("config", "user.email", "t@t")
    git("config", "user.name", "t")
    git("add", "-A")
    git("commit", "-qm", "before")

    target.write_text(sub("`GROK_CONFIG`", "GROK_CONFIG")(FIXTURE),
                      encoding="utf-8")
    (tmp / "declared.tsv").write_text(
        "GROK_CONFIG\t上游变量，本分叉没有孪生\n", encoding="utf-8")
    (tmp / "empty.tsv").write_text("# nothing declared\n", encoding="utf-8")

    results = []
    for listing, expect_drift in (("declared.tsv", False), ("empty.tsv", True)):
        proc = subprocess.run(
            [sys.executable, GATE, "--before", "HEAD", "--after", "WORKTREE",
             "--glob", REL, "--span-removals", listing],
            cwd=tmp, capture_output=True, text=True)
        results.append((listing, proc.returncode != 0, expect_drift, proc))
    if all(drifted == expect for _, drifted, expect, _ in results):
        print(f"ok   {name}: declared=no-drift undeclared=drift")
        return 0
    print(f"FAIL {name}")
    for listing, drifted, expect, proc in results:
        print(f"     {listing}: drift={drifted} but expected {expect}")
        print("     " + proc.stdout.strip().replace("\n", "\n     ")[:400])
    return 1


FIX_ANCHORS_BEFORE_A = (
    "## Overview\n\n## Trust and Security\n\nSee [self](#trust-and-security).\n"
)
FIX_ANCHORS_AFTER_A = (
    "## 概述\n\n## 信任与安全\n\nSee [self](#信任与安全).\n"
)
FIX_ANCHORS_BEFORE_B = "See [trust](a.md#trust-and-security).\n"
FIX_ANCHORS_EXPECT_B = "See [trust](a.md#信任与安全).\n"


def run_fix_anchors_case() -> int:
    """A rewritten anchor must not gain a second closing paren.

    `LINK` deliberately stops before the `)`, so the replacement has to
    supply only what the match consumed. Emitting one more yields
    `](a.md#anchor))`, which still passes `--links` (the target parses and
    the stray paren is trailing text) but renders as a broken link with a
    visible `)`. That is why this needs its own case.

    Both directions are covered: a cross-file anchor (`b.md` -> `a.md`) and
    a same-file one, which resolves against the linking file's own table.
    """
    name = "fix-anchors: rewritten links stay well formed"
    tmp = Path(tempfile.mkdtemp())
    (tmp / "a.md").write_text(FIX_ANCHORS_BEFORE_A, encoding="utf-8")
    (tmp / "b.md").write_text(FIX_ANCHORS_BEFORE_B, encoding="utf-8")

    def git(*a: str):
        return subprocess.run(["git", *a], cwd=tmp, capture_output=True,
                              text=True)

    git("init", "-q")
    git("config", "user.email", "t@t")
    git("config", "user.name", "t")
    git("add", "-A")
    git("commit", "-qm", "before")

    (tmp / "a.md").write_text(FIX_ANCHORS_AFTER_A, encoding="utf-8")
    proc = subprocess.run(
        [sys.executable, GATE, "--fix-anchors", "--before", "HEAD",
         "--glob", "*.md"],
        cwd=tmp, capture_output=True, text=True)
    got_a = (tmp / "a.md").read_text(encoding="utf-8")
    got_b = (tmp / "b.md").read_text(encoding="utf-8")
    if got_a == FIX_ANCHORS_AFTER_A and got_b == FIX_ANCHORS_EXPECT_B:
        print(f"ok   {name}: {got_b.strip()}")
        return 0
    print(f"FAIL {name}: exit={proc.returncode}")
    print(f"     want a.md {FIX_ANCHORS_AFTER_A!r}")
    print(f"     got  a.md {got_a!r}")
    print(f"     want b.md {FIX_ANCHORS_EXPECT_B!r}")
    print(f"     got  b.md {got_b!r}")
    return 1


def main() -> int:
    failures = sum(run_case(*case) for case in CASES)
    failures += sum(run_cell_case(*case) for case in CELL_CASES)
    failures += sum(run_fork_name_case(*case) for case in FORK_NAME_CASES)
    failures += run_apply_case()
    failures += run_removal_case()
    failures += run_fix_anchors_case()
    total = (len(CASES) + len(CELL_CASES) + len(FORK_NAME_CASES) + 3)
    print(f"\n{total - failures}/{total} self-test case(s) passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
