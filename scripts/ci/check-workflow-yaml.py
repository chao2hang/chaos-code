#!/usr/bin/env python3
"""Reject a workflow line whose YAML is not what its author meant.

Not a YAML parser -- a scanner for one specific mistake, because that one mistake
switches the whole repository's CI off. GitHub refuses to run a workflow file it
cannot parse, so the push that breaks the syntax gets no jobs at all, and every
guard in the file -- secret scan, installer labs, platform legs -- silently does
nothing for that commit.

The mistake: an unquoted scalar value containing `": "`. YAML reads a colon
followed by a space as a mapping separator, so

    - name: cargo test (target-OS crates: xai-grok-tools)

is not a step name with parentheses in it; it is a key `cargo test (target-OS crates`
whose value starts with `xai-grok-tools)`, and the file dies with
"mapping values are not allowed here". This came from writing exactly that step
name while splitting the platform test step in two, and nothing in the repository
noticed: `check-workflow-shells.py` and `check-workflow-toolchain.py` are both
line-based and both reported OK, and the error only surfaced when the file was
parsed. Quoting the value -- `name: "cargo test (target-OS crates: xai-grok-tools)"`
-- is the fix.

What is exempt, because YAML really does allow it:
  * a scalar that *begins* with a single or double quote. Quotes inside a plain scalar
    protect nothing -- `run: echo "a: b"` is a plain scalar and is refused, which is the
    case a quote-aware scanner waves through;
  * the content of a block scalar (`run: |`, `run: >-`) -- a shell script may hold
    anything, which is why most of these files is invisible to this check;
  * comments;
  * a colon with no space after it (`image: alpine:3.19`, `docker://x:1`), which YAML
    treats as part of the scalar.

Usage: python3 scripts/ci/check-workflow-yaml.py [workflow.yml ...]
Exit: 0 = every workflow parses at the level this check models, 1 = a value needs quoting.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

WORKFLOW_DIR = Path(".github/workflows")


def default_workflows() -> list[Path]:
    """Every workflow in `.github/workflows`, rather than a list of names in a file.

    All three workflow gates used to hard-code `ci.yml` and `release.yml`, so a third
    file -- `docker-labs.yml`, added in the same change as this comment -- would be
    checked by none of them. For this gate that is the worst case of the three: a file
    nobody parses is a file whose syntax error switches off the guards inside it.
    """
    return sorted(WORKFLOW_DIR.glob("*.yml"))

# `key: value`, `key:` and `- key: value`, with the value split off. Keys here are
# workflow vocabulary or shell text; anything else falls through to the scalar branch.
KEY_VALUE = re.compile(r"^(?:- +)?([A-Za-z0-9_.-]+):(?:[ \t]+(.*))?$")
BLOCK_SCALAR = re.compile(r"^[|>][0-9]*[-+]?$")


def strip_comment(line: str) -> str:
    """Drop a trailing `#` comment, honouring quotes (YAML: `#` needs a space before it)."""
    out = []
    quote = ""
    i = 0
    while i < len(line):
        ch = line[i]
        if quote:
            if quote == '"' and ch == "\\":
                out.append(line[i : i + 2])
                i += 2
                continue
            if ch == quote:
                quote = ""
        elif ch in "\"'":
            quote = ch
        elif ch == "#" and (i == 0 or line[i - 1] in " \t"):
            break
        out.append(ch)
        i += 1
    return "".join(out).rstrip()


def find_colon_in_plain(value: str) -> int:
    """Index of a `: ` (or a trailing `:`) inside a plain scalar, else -1.

    No quote handling, deliberately: a quote protects a scalar only when it *starts* it,
    and callers only pass values that do not. `run: echo "a: b"` is a plain scalar as far
    as YAML is concerned, and PyYAML refuses it -- which is the case a quote-aware scanner
    would wave through.
    """
    for i, ch in enumerate(value):
        if ch == ":" and (i + 1 >= len(value) or value[i + 1] in " \t"):
            return i
    return -1


def check_text(text: str) -> list[str]:
    problems: list[tuple[int, str]] = []
    block_indent: int | None = None
    for lineno, raw in enumerate(text.splitlines(), 1):
        if not raw.strip():
            continue
        indent = len(raw) - len(raw.lstrip(" "))
        if block_indent is not None:
            # Everything indented deeper than the key that opened `|`/`>` is scalar text.
            if indent > block_indent:
                continue
            block_indent = None
        body = strip_comment(raw.lstrip())
        if not body or body.startswith("#"):
            continue

        match = KEY_VALUE.match(body)
        if match:
            value = match.group(2)
            if value is None:
                continue  # `key:` -- a nested block, no scalar to misread
            value = value.strip()
            if not value:
                continue
            if BLOCK_SCALAR.match(value):
                block_indent = indent
                continue
        elif body.startswith("- "):
            value = body[2:].strip()
        else:
            continue

        if value[0] in "\"'":
            # Quoted at the start, so the scalar is quoted (or spans lines, which this
            # scanner leaves alone). Either way the mapping separator is data, not syntax.
            continue
        at = find_colon_in_plain(value)
        if at >= 0:
            trailing = value[at + 1 :].strip() == ""
            how = "ends with a colon" if trailing else "contains ': '"
            problems.append(
                (
                    lineno,
                    f"unquoted value {value!r} {how}; YAML reads that as a mapping "
                    "separator, so the file will not parse -- quote the whole value",
                )
            )
    return problems


def main(argv: list[str]) -> int:
    paths = [Path(a) for a in argv[1:]] or default_workflows()
    if not paths:
        print("workflow yaml: no workflow files found -- refusing to pass", file=sys.stderr)
        return 1
    missing = [p for p in paths if not p.is_file()]
    if missing:
        print("workflow yaml: no such file(s): " + ", ".join(str(m) for m in missing), file=sys.stderr)
        return 1
    bad = 0
    for path in paths:
        for lineno, problem in check_text(path.read_text(encoding="utf-8")):
            print(f"{path}:{lineno}: {problem}")
            bad += 1
    if bad:
        print(
            f"\nworkflow yaml: {bad} value(s) GitHub's parser will refuse; a workflow that "
            "will not parse runs no jobs, so this is not only a cosmetic error",
            file=sys.stderr,
        )
        return 1
    print(f"workflow yaml: {len(paths)} file(s), no unquoted scalar with a mapping separator")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
