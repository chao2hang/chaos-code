#!/usr/bin/env python3
"""Fixtures for `check-workflow-yaml.py`.

The check exists because a step name with a colon in it --
`cargo test (target-OS crates: xai-grok-tools)` -- is not a step name to YAML, it is a
mapping, and a workflow file that will not parse runs *no jobs at all*: no secret scan, no
installer labs, no platform legs, and a green-looking push. Nothing in this repository
noticed; the line-based workflow guards both said OK.

Each case is a whole workflow around one step, so the case under test sits at a known line
and the surrounding structure stays valid:

  * the unquoted step name that started this is refused, by line number;
  * the same name double-quoted, and single-quoted, is accepted;
  * `: ` inside a `run: |` block script is accepted -- a shell script is scalar text, and
    a check that flagged it would have to be switched off, which is how such checks die;
  * `: ` in a comment is accepted;
  * `alpine:3.19` and `docker://ghcr.io/x/y:1.36` are accepted: a colon with no space
    after it is part of the scalar;
  * a value ending in a bare colon is refused;
  * `run: echo "a: b"` is refused -- quotes only protect a scalar they *start*, and this
    is the shape a scanner that tracked quotes naively would miss;
  * `check-workflow-yaml.py` run against this repository's own workflows is clean.

Where the host has PyYAML, every case is additionally cross-checked against a real parser:
this check's verdict must agree with `yaml.safe_load` refusing or accepting the same file.
Without PyYAML the assertions above still run, and the script says which mode it ran in.

    python3 scripts/ci/test-check-workflow-yaml.py
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-workflow-yaml.py")
REPO = SCRIPT.parents[2]

try:  # optional: the cross-check is a bonus, not a dependency
    import yaml  # type: ignore
except Exception:  # pragma: no cover - depends on the host's python
    yaml = None

WORKFLOW = """\
name: CI
on: push
jobs:
  j:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
{case}
"""

# (label, step text, expected exit code of the check)
CASES: list[tuple[str, str, int]] = [
    (
        "unquoted colon in a step name",
        '      - name: cargo test (target-OS crates: xai-grok-tools)\n        run: echo hi\n',
        1,
    ),
    (
        "double-quoted step name",
        '      - name: "cargo test (target-OS crates: xai-grok-tools)"\n        run: echo hi\n',
        0,
    ),
    (
        "single-quoted step name",
        "      - name: 'cargo test (target-OS crates: xai-grok-tools)'\n        run: echo hi\n",
        0,
    ),
    (
        "colon inside a block script",
        '      - name: build\n        run: |\n          sed -i \'s/a: b/c/\' f  # note: scalar text\n',
        0,
    ),
    (
        "colon inside a `>` block script",
        "      - name: build\n        run: >-\n          echo one: two\n",
        0,
    ),
    (
        "colon inside a comment",
        "      # name: a: b\n      - name: build\n        run: echo hi\n",
        0,
    ),
    ("colon with no space after it", "      - name: alpine:3.19\n", 0),
    ("a docker reference", "      - uses: docker://ghcr.io/x/y:1.36\n", 0),
    ("value ending in a colon", "      - name: build:\n", 1),
    ("plain name, nothing to misread", "      - name: cargo test (target-OS crates)\n", 0),
    (
        "quotes that do not start the scalar",
        '      - run: echo "a: b"\n',
        1,
    ),
    (
        "a quoted one-line run",
        '      - name: greet\n        run: "echo a: b"\n',
        0,
    ),
]


class WorkflowYaml(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="workflow-yaml-")
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def write(self, case: str) -> Path:
        path = self.tmp / "ci.yml"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(WORKFLOW.format(case=case), encoding="utf-8")
        return path

    def run_check(self, path: Path) -> tuple[int, str]:
        proc = subprocess.run(
            [sys.executable, str(SCRIPT), str(path)], capture_output=True, text=True
        )
        return proc.returncode, proc.stdout + proc.stderr

    def test_every_case_gets_the_expected_verdict(self) -> None:
        for label, case, expected in CASES:
            with self.subTest(label):
                path = self.write(case)
                code, out = self.run_check(path)
                self.assertEqual(code, expected, f"{label}\n{out}")

    def test_the_refused_line_is_named(self) -> None:
        # The step is the last thing in the template, so it sits at a computable line.
        case = CASES[0][1]
        path = self.write(case)
        code, out = self.run_check(path)
        self.assertEqual(code, 1, out)
        want_line = WORKFLOW.format(case=case).splitlines().index(case.splitlines()[0]) + 1
        self.assertIn(f"{path}:{want_line}: unquoted value", out)
        self.assertIn("quote the whole value", out)

    def test_block_scalar_text_is_invisible_afterwards(self) -> None:
        # A step after a multi-line script must still be checked: if the block-scalar
        # state never ended, every later step would be exempt.
        case = (
            "      - name: build\n"
            "        run: |\n"
            "          echo scalar text\n"
            "          echo more\n"
            "      - name: cargo test (target-OS crates: xai-grok-tools)\n"
        )
        code, out = self.run_check(self.write(case))
        self.assertEqual(code, 1, out)
        self.assertIn("xai-grok-tools", out)

    def test_a_later_key_at_the_step_indent_ends_the_block(self) -> None:
        # `timeout-minutes` sits at the step's own indent, so the block has ended by then;
        # the following step still has to be scanned.
        case = (
            "      - name: build\n"
            "        run: |\n"
            "          echo scalar text\n"
            "        timeout-minutes: 35\n"
            "      - name: cargo test (target-OS crates: tools)\n"
        )
        code, out = self.run_check(self.write(case))
        self.assertEqual(code, 1, out)
        self.assertIn("timeout-minutes", self.write(case).read_text())
        self.assertIn("crates: tools", out)

    def test_missing_file_is_an_error_not_a_pass(self) -> None:
        proc = subprocess.run(
            [sys.executable, str(SCRIPT), str(self.tmp / "absent.yml")],
            capture_output=True,
            text=True,
        )
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("no such file", proc.stdout + proc.stderr)

    def test_shipped_workflows_are_clean(self) -> None:
        code, out = self.run_check(REPO / ".github" / "workflows" / "ci.yml")
        self.assertEqual(code, 0, out)
        code, out = self.run_check(REPO / ".github" / "workflows" / "release.yml")
        self.assertEqual(code, 0, out)

    @unittest.skipIf(yaml is None, "PyYAML is not installed on this host")
    def test_verdicts_agree_with_a_real_yaml_parser(self) -> None:
        for label, case, expected in CASES:
            with self.subTest(label):
                text = WORKFLOW.format(case=case)
                try:
                    yaml.safe_load(text)
                    parses = True
                except Exception:
                    parses = False
                self.assertEqual(
                    parses,
                    expected == 0,
                    f"{label}: the check says exit {expected}, yaml.safe_load "
                    f"{'accepts' if parses else 'refuses'} this file",
                )


if __name__ == "__main__":
    if yaml is None:
        print("note: PyYAML absent -- running the check's own assertions only")
    unittest.main(verbosity=2)
