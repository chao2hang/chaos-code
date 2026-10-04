#!/usr/bin/env python3
"""Fixtures for `scripts/ci/check-e2e-registration.py`.

The gate's subject is a set of hand-written lists that decide which browser specs run, and every way
of getting them wrong looks like a green run:

- a `*.pw.ts` no `testMatch` mentions is not collected, and Playwright exits 0 anyway;
- a project that collects nothing is not reported either, because Playwright only complains when
  nothing at all is found;
- a name left in a list after the spec it named was renamed keeps claiming coverage that is gone;
- a config the runner does not name never runs, since CI types `npm run test:e2e`, not a path.

`playwright.config.ts` in this repository also carries the sharpest shape of the mistake, so it is
a fixture of its own: it sets `testMatch` at the top level and again inside all three of its
projects, and a project's `testMatch` replaces the top-level one. A new spec name added only at the
top level -- where anyone would put it -- registers nothing, and the pattern still matches the other
specs, so nothing about it looks wrong.

The templates below are that file's structure in miniature, including the two things a naive text
search would trip over: a `url: \\`http://127.0.0.1:${port}/health\\`` template whose `//` is not a
comment, and a `testMatch: gitSpec` that points at a `const`.

    python3 scripts/ci/test-check-e2e-registration.py
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
GATE = HERE / "check-e2e-registration.py"

#: `playwright.config.ts` in miniature: a top-level `testMatch`, and three projects that each set
#: their own, which is the precedence trap `test_top_level_name_omitted_by_every_project_*` is about.
#: The top level and the viewport list are separate placeholders because that file writes both, and
#: the difference between them is the whole point of one of the cases below.
CONFIG_ONE = """/** The suite that talks to the demo responder. */
import { defineConfig, devices } from '@playwright/test'

const uiPort = Number(process.env.CHAOS_E2E_UI_PORT || 5174)

export default defineConfig({
  testDir: './e2e',
  testMatch: @@TOP@@,
  projects: [
    {
      name: 'desktop-chromium',
      use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 1000 } },
      testMatch: @@VIEWPORT@@,
    },
    {
      name: 'mobile-chromium',
      use: { ...devices['Desktop Chrome'], viewport: { width: 390, height: 844 } },
      testMatch: @@VIEWPORT@@,
    },
    {
      name: 'observer-browser',
      use: { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 900 } },
      testMatch: @@OBSERVER@@,
    },
  ],
  webServer: [
    {
      command: `../../target/debug/chaos-web`,
      url: `http://127.0.0.1:${uiPort}/health`,
      timeout: 60_000,
    },
  ],
})
"""

#: `playwright.git.config.ts` in miniature: one `const` holding the pattern, projects that do not
#: override it, and so inherit the top level.
CONFIG_TWO = """import { defineConfig, devices } from '@playwright/test'

const gitSpec = @@GITSPEC@@

export default defineConfig({
  testDir: './e2e',
  testMatch: gitSpec,
  projects: [
    { name: 'desktop-chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'mobile-chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 390 } } },
  ],
})
"""

#: A config whose only `testMatch` lines are commented out, which is what Playwright sees: the
#: default pattern, which no `*.pw.ts` matches.
ALL_COMMENTS = """import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  // testMatch: gitSpec,
  // testMatch: /alpha\\.pw\\.ts/,
  /* testMatch: /beta\\.pw\\.ts/, */
})
"""

TOP = "/(?:alpha|beta|gamma)\\.pw\\.ts/"
OBSERVER = "/observer\\.pw\\.ts/"
GITSPEC = "/commit-form\\.pw\\.ts/"

#: The specs the clean tree registers: three by the top level and its two viewport projects, one by
#: the observer project, one by the second config.
CLEAN_SPECS = ("alpha", "beta", "gamma", "observer", "commit-form")

RUNNER = """import { spawn } from 'node:child_process'

// Two configs, because the second needs a host started with a real provider.
const configs = ['playwright.config.ts', 'playwright.git.config.ts']

function run(config) {
  return spawn(process.execPath, [cli, 'test', '--config', config], { stdio: 'inherit' })
}
"""

PACKAGE = {"name": "mini-ui", "scripts": {"build": "vite build",
                                          "test:e2e": "node ./e2e-runner.mjs"}}


def run_gate(root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(GATE), "--root", str(root)],
                          capture_output=True, text=True)


def run_list(root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(GATE), "--root", str(root), "--list"],
                          capture_output=True, text=True)


def gate_module():
    """The shipped guard as a module, so its text reader can be driven on its own."""
    import importlib.util

    spec = importlib.util.spec_from_file_location("check_e2e_registration", GATE)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


class TextReader(unittest.TestCase):
    """The shipped scanner, driven directly, because every rule above is built on it.

    A reader that mistakes a `//` inside a URL for a comment, or a division for a pattern, silently
    stops seeing registrations, and the only symptom is a config that reads as empty. These are the
    three places that judgement lives, so they are tested as functions rather than through a tree.
    """

    @classmethod
    def setUpClass(cls) -> None:
        cls.gate = gate_module()

    def test_a_url_inside_a_template_is_not_a_comment(self) -> None:
        source = ("  url: `http://127.0.0.1:${port}/health`, // the host's own health route\n"
                  "  testMatch: /alpha\\.pw\\.ts/,\n")
        out = self.gate.strip_js_comments(source)
        self.assertIn("http://127.0.0.1:${port}/health", out)
        self.assertIn("testMatch: /alpha\\.pw\\.ts/,", out)
        self.assertNotIn("the host's own health route", out)

    def test_a_block_comment_keeps_the_line_count(self) -> None:
        source = "before\n/* one\ntwo\nthree */\nafter\n"
        out = self.gate.strip_js_comments(source)
        self.assertNotIn("one", out)
        self.assertIn("after", out)
        self.assertEqual(out.count("\n"), source.count("\n"), out)
        self.assertEqual(out.splitlines().index("after"), 4, out)

    def test_a_regex_character_class_holds_a_slash(self) -> None:
        """`[/]` is one character of a pattern, not the end of it."""
        source = "const spec = /[a/]+\\.pw\\.ts/g\n"
        at = source.index("/")
        literal, after = self.gate._read_regex(source, at)
        self.assertEqual(literal, "/[a/]+\\.pw\\.ts/g")
        self.assertEqual(source[after:], "\n")
        self.assertEqual(self.gate.strip_js_comments(source), source)

    def test_a_slash_after_a_value_divides_and_does_not_open_a_pattern(self) -> None:
        source = "const half = total / 2\n"
        self.assertFalse(self.gate._regex_start(source, source.index("/")))
        opener = "testMatch: /alpha\\.pw\\.ts/,"
        self.assertTrue(self.gate._regex_start(opener, opener.index("/")))

    def test_alternatives_are_read_only_from_a_group_of_names(self) -> None:
        big = r"(?:workspace-flow|tool-activity|phone-shell)\.pw\.ts"
        names = [name for name, _ in self.gate.alternation_variants(big)]
        self.assertEqual(names, ["workspace-flow", "tool-activity", "phone-shell"])
        variant = dict(self.gate.alternation_variants(big))["tool-activity"]
        self.assertEqual(variant, r"tool-activity\.pw\.ts")
        optional = r"commit-message(?:-safe-mode)?\.pw\.ts"
        self.assertEqual(self.gate.alternation_variants(optional), [])

    def test_an_alternative_spanning_nested_groups_is_one_branch(self) -> None:
        """A branch holding a group of its own is one name, and its inner names are also checked."""
        source = r"(?:a|(?:b|c)x)\.pw\.ts"
        names = [name for name, _ in self.gate.alternation_variants(source)]
        self.assertEqual(sorted(names), sorted(["a", "(?:b|c)x", "b", "c"]))


class Fixtures(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="e2e-registration-")
        self.addCleanup(self._tmp.cleanup)
        self.ui = Path(self._tmp.name) / "apps" / "chaos-ui"
        self.ui.mkdir(parents=True)
        (self.ui / "e2e").mkdir()

    def build(self, *, specs=CLEAN_SPECS, config_one=CONFIG_ONE, config_two=CONFIG_TWO,
              top=TOP, viewport=TOP, observer=OBSERVER, git_spec=GITSPEC,
              runner: str | None = RUNNER, package: dict | None = PACKAGE,
              extra: dict[str, str] | None = None) -> Path:
        """Writes a mini `apps/chaos-ui` whose defaults are the clean case."""
        for name in specs:
            (self.ui / "e2e" / f"{name}.pw.ts").write_text(f"test('{name}', async () => {{}}))\n",
                                                           encoding="utf-8")
        one = (config_one.replace("@@TOP@@", top).replace("@@VIEWPORT@@", viewport)
               .replace("@@OBSERVER@@", observer))
        two = config_two.replace("@@GITSPEC@@", git_spec)
        (self.ui / "playwright.config.ts").write_text(one, encoding="utf-8")
        (self.ui / "playwright.git.config.ts").write_text(two, encoding="utf-8")
        if runner is not None:
            (self.ui / "e2e-runner.mjs").write_text(runner, encoding="utf-8")
        if package is not None:
            (self.ui / "package.json").write_text(json.dumps(package), encoding="utf-8")
        for rel, content in (extra or {}).items():
            target = self.ui / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content, encoding="utf-8")
        return Path(self._tmp.name)

    def assert_clean(self) -> subprocess.CompletedProcess[str]:
        proc = run_gate(Path(self._tmp.name))
        self.assertEqual(proc.returncode, 0, f"stdout: {proc.stdout}\nstderr: {proc.stderr}")
        self.assertIn("check-e2e-registration: OK", proc.stdout)
        return proc

    def assert_fails(self, *needles: str) -> str:
        proc = run_gate(Path(self._tmp.name))
        self.assertNotEqual(proc.returncode, 0, f"expected a failure, got: {proc.stdout}")
        self.assertIn("check-e2e-registration: FAIL", proc.stdout)
        for needle in needles:
            self.assertIn(needle, proc.stdout, f"missing {needle!r} in:\n{proc.stdout}")
        return proc.stdout

    # -- the controls ------------------------------------------------------------------------------

    def test_the_clean_tree_passes_and_counts_every_spec(self) -> None:
        self.build()
        proc = self.assert_clean()
        self.assertIn("5 spec file(s)", proc.stdout)
        self.assertIn("2 config(s)", proc.stdout)

    def test_the_clean_tree_maps_each_spec_to_its_projects(self) -> None:
        """The two viewport projects must be named, not collapsed into the config."""
        self.build()
        listed = run_list(Path(self._tmp.name)).stdout.splitlines()
        self.assertEqual(len(listed), 5, listed)
        alpha = next(line for line in listed if line.startswith("e2e/alpha.pw.ts"))
        self.assertIn("project `desktop-chromium`", alpha)
        self.assertIn("project `mobile-chromium`", alpha)
        self.assertNotIn("observer", alpha)
        observer = next(line for line in listed if line.startswith("e2e/observer.pw.ts"))
        self.assertIn("project `observer-browser`", observer)
        self.assertFalse([line for line in listed if "UNCLAIMED" in line])

    def test_the_real_repository_is_clean_and_actually_scanned(self) -> None:
        proc = run_gate(REPO)
        self.assertEqual(proc.returncode, 0, f"stdout: {proc.stdout}\nstderr: {proc.stderr}")
        counted = int(proc.stdout.split("(")[1].split(" spec file")[0])
        self.assertGreaterEqual(counted, 14, f"only {counted} specs were read: {proc.stdout}")
        listed = run_list(REPO).stdout.splitlines()
        self.assertGreaterEqual(len(listed), 14, listed)
        self.assertNotIn("UNCLAIMED", run_list(REPO).stdout)
        for spec in ("commit-message-safe-mode.pw.ts", "approval-competition.pw.ts",
                     "phone-shell.pw.ts"):
            self.assertIn(spec, run_list(REPO).stdout, f"{spec} was not read at all")

    def test_helper_and_tool_files_are_not_specs(self) -> None:
        self.build(extra={"e2e/support/commit-form.ts": "export const x = 1\n",
                          "e2e/support/notes.md": "not a spec\n",
                          "e2e/fixtures/workspace/a.txt": "fixture\n"})
        proc = self.assert_clean()
        self.assertIn("5 spec file(s)", proc.stdout)

    def test_a_dependency_own_spec_is_not_part_of_the_suite(self) -> None:
        self.build(extra={"node_modules/some-dep/e2e/dep-flow.pw.ts": "test('dep', () => {})\n"})
        proc = self.assert_clean()
        self.assertIn("5 spec file(s)", proc.stdout)
        self.assertNotIn("dep-flow", run_list(Path(self._tmp.name)).stdout)

    def test_a_url_template_is_not_read_as_a_comment(self) -> None:
        """`http://host` inside a template literal must not swallow the rest of the file."""
        self.build()
        self.assert_clean()
        text = (self.ui / "playwright.config.ts").read_text(encoding="utf-8")
        self.assertIn("http://127.0.0.1:", text)

    # -- a spec nothing would run ------------------------------------------------------------------

    def test_a_spec_named_by_nothing_fails_and_names_it(self) -> None:
        self.build(specs=(*CLEAN_SPECS, "brand-new"))
        out = self.assert_fails("e2e/brand-new.pw.ts: no config claims it")
        self.assertIn("would never run", out)
        # The top level and both viewport projects write the same list. The finding says that
        # list once, because what it has to convey is which patterns exist -- three copies of
        # one line bury the two patterns that differ, which are the ones worth reading.
        self.assertEqual(out.count(r"pattern /(?:alpha|beta|gamma)\.pw\.ts/"), 1)
        self.assertIn(r"pattern /observer\.pw\.ts/", out)
        self.assertIn(r"pattern /commit-form\.pw\.ts/", out)

    def test_a_spec_outside_every_test_dir_fails_even_when_named(self) -> None:
        """`testDir` is honored: a pattern cannot reach above it."""
        self.build(top="/(?:alpha|beta|gamma|stray)\\.pw\\.ts/",
                   extra={"src/stray.pw.ts": "test('stray', () => {})\n"})
        self.assert_fails("src/stray.pw.ts: no config claims it",
                          "outside every config's `testDir`")

    def test_two_configs_claiming_one_spec_fails_and_names_both(self) -> None:
        self.build(git_spec="/(?:commit-form|alpha)\\.pw\\.ts/")
        out = self.assert_fails("e2e/alpha.pw.ts: claimed by playwright.config.ts and "
                                "playwright.git.config.ts")
        self.assertIn("twice", out)

    # -- the precedence trap -----------------------------------------------------------------------

    def test_top_level_name_omitted_by_every_project_is_not_registered(self) -> None:
        """The exact mistake `playwright.config.ts` invites: the top level reads as registered."""
        self.build(specs=(*CLEAN_SPECS, "delta"),
                   top="/(?:alpha|beta|gamma|delta)\\.pw\\.ts/")
        out = self.assert_fails("e2e/delta.pw.ts: the top-level `testMatch` of "
                                "playwright.config.ts matches it")
        self.assertIn("replaces the top level", out)
        # ...and it is reported once, not also as a plain unclaimed file.
        self.assertEqual(out.count("e2e/delta.pw.ts"), 1, out)

    def test_a_name_written_at_the_top_level_and_in_the_projects_is_registered(self) -> None:
        """The control for the case above: the same spec, named in the lists that are consulted."""
        self.build(specs=(*CLEAN_SPECS, "delta"),
                   top="/(?:alpha|beta|gamma|delta)\\.pw\\.ts/",
                   viewport="/(?:alpha|beta|gamma|delta)\\.pw\\.ts/")
        self.assert_clean()

    def test_a_project_that_adds_a_name_still_registers_it(self) -> None:
        """The opposite case, which is correct: one project selecting a spec is enough to run it."""
        observer_one = CONFIG_ONE.replace("@@OBSERVER@@", "/(?:observer|delta)\\.pw\\.ts/")
        self.build(specs=(*CLEAN_SPECS, "delta"), config_one=observer_one)
        self.assert_clean()

    def test_a_project_selecting_nothing_fails(self) -> None:
        """A viewport variant matching no spec is invisible from the Playwright run itself."""
        three_projects = CONFIG_ONE.replace(
            "@@OBSERVER@@",
            "/tablet-flow\\.pw\\.ts/",
        ).replace("observer-browser", "tablet-chromium")
        specs = tuple(name for name in CLEAN_SPECS if name != "observer")
        self.build(specs=specs, config_one=three_projects)
        out = self.assert_fails("project `tablet-chromium` collects no spec file")
        self.assertIn("never exercised", out)

    # -- stale names -------------------------------------------------------------------------------

    def test_a_pattern_matching_nothing_fails(self) -> None:
        specs = tuple(name for name in CLEAN_SPECS if name != "commit-form")
        self.build(specs=specs)
        self.assert_fails("playwright.git.config.ts", "/commit-form\\.pw\\.ts/ matches no spec")

    def test_a_stale_alternative_inside_a_pattern_fails_by_name(self) -> None:
        specs = tuple(name for name in CLEAN_SPECS if name != "gamma")
        self.build(specs=specs)
        self.assert_fails("`gamma` inside top-level `testMatch` pattern",
                          "matches no spec")

    def test_an_optional_group_is_not_treated_as_a_list_of_names(self) -> None:
        """`commit-message(?:-safe-mode)?` is one name with an optional part, not two names."""
        self.build(git_spec="/commit-form(?:-safe-mode)?\\.pw\\.ts/",
                   specs=(*CLEAN_SPECS, "commit-form-safe-mode"))
        self.assert_clean()

    # -- the two lists above the configs -----------------------------------------------------------

    def test_a_config_the_runner_does_not_name_fails(self) -> None:
        extra_config = ("export default { testDir: './e2e', "
                        "testMatch: /extra-flow\\.pw\\.ts/, projects: [] }\n")
        self.build(specs=(*CLEAN_SPECS, "extra-flow"),
                   extra={"playwright.extra.config.ts": extra_config})
        self.assert_fails("playwright.extra.config.ts: nothing runs it")

    def test_a_runner_naming_a_config_that_is_not_there_fails(self) -> None:
        self.build(runner=RUNNER.replace("'playwright.git.config.ts'",
                                         "'playwright.gone.config.ts'"))
        self.assert_fails("lists `playwright.gone.config.ts`, which is not a file")

    def test_a_config_listed_twice_by_the_runner_fails(self) -> None:
        self.build(runner=RUNNER.replace("const configs = [",
                                         "const configs = ['playwright.config.ts',"))
        self.assert_fails("`playwright.config.ts` is listed twice")

    def test_a_runner_without_a_config_list_fails_rather_than_passing(self) -> None:
        self.build(runner="import { spawn } from 'node:child_process'\n")
        self.assert_fails("no `configs = [...]` array")

    def test_a_missing_runner_file_fails(self) -> None:
        self.build(runner=None)
        self.assert_fails("e2e-runner.mjs: not found")

    def test_a_package_script_that_bypasses_the_runner_fails(self) -> None:
        """A bare `playwright test` runs one config and calls the suite green."""
        bypassed = {**PACKAGE, "scripts": {**PACKAGE["scripts"],
                                           "test:e2e": "playwright test --config "
                                                       "playwright.config.ts"}}
        self.build(package=bypassed)
        self.assert_fails("which does not invoke", "calls the suite green")

    def test_a_package_without_the_script_fails(self) -> None:
        self.build(package={**PACKAGE, "scripts": {"build": "vite build"}})
        self.assert_fails("package.json: no `test:e2e` script")

    # -- shapes this guard refuses to read silently --------------------------------------------------

    def test_a_glob_test_match_is_refused(self) -> None:
        """Playwright accepts a glob string; reading it as nothing would be the silent failure."""
        self.build(config_one=CONFIG_ONE.replace(f"testMatch: @@TOP@@,",
                                                 "testMatch: '**/*.pw.ts',"))
        self.assert_fails("`testMatch` is the glob")

    def test_an_identifier_bound_to_nothing_is_refused(self) -> None:
        self.build(config_two=CONFIG_TWO.replace("const gitSpec = @@GITSPEC@@",
                                                 "const otherSpec = @@GITSPEC@@"))
        self.assert_fails("points at `gitSpec`, which this file never binds to a pattern")

    def test_an_identifier_bound_after_its_use_is_refused(self) -> None:
        """A `const` has a temporal dead zone, so a later binding resolves to nothing."""
        late = CONFIG_TWO.replace("const gitSpec = @@GITSPEC@@\n", "") + \
            "\nconst gitSpec = /commit-form\\.pw\\.ts/\n"
        self.build(config_two=late)
        self.assert_fails("points at `gitSpec`, which this file never binds to a pattern")

    def test_an_array_of_patterns_is_honored(self) -> None:
        """Playwright accepts an array; both halves have to be read, not just the first."""
        self.build(config_two=CONFIG_TWO.replace(
            "testMatch: gitSpec,", "testMatch: [/commit-form\\.pw\\.ts/, /second\\.pw\\.ts/],"),
            specs=(*CLEAN_SPECS, "second"))
        self.assert_clean()

    def test_a_pattern_that_does_not_compile_is_reported(self) -> None:
        self.build(config_one=CONFIG_ONE.replace("@@OBSERVER@@", "/(?:observer\\.pw\\.ts/"))
        self.assert_fails("does not compile as a Python regular expression")

    def test_an_unknown_flag_is_reported_rather_than_ignored(self) -> None:
        self.build(config_one=CONFIG_ONE.replace("@@OBSERVER@@", "/observer\\.pw\\.ts/d"))
        self.assert_fails("flag(s) d, which this reader does not apply")

    def test_a_comment_cannot_fake_a_registration(self) -> None:
        """A commented-out `testMatch` is dead text; treating it as a claim is going blind."""
        self.build(config_two=ALL_COMMENTS)
        out = self.assert_fails("playwright.git.config.ts: no `testMatch` and no `projects`")
        self.assertIn("default pattern", out)
        # The commented pattern names `alpha`, which the top level also claims, so the only spec
        # left unclaimed is the one that second config was supposed to run.
        self.assertIn("e2e/commit-form.pw.ts: no config claims it", out)

    def test_a_block_comment_cannot_fake_a_registration_either(self) -> None:
        commented = CONFIG_ONE.replace("@@OBSERVER@@",
                                       "/* testMatch: /alpha\\.pw\\.ts/, */ /observer\\.pw\\.ts/")
        self.build(config_one=commented)
        self.assert_clean()

    # -- failing closed when there is nothing to compare -------------------------------------------

    def test_a_tree_with_no_specs_is_an_error(self) -> None:
        self.build(specs=())
        proc = run_gate(Path(self._tmp.name))
        self.assertNotEqual(proc.returncode, 0, proc.stdout)
        self.assertIn("no `*.pw.ts` spec file", proc.stdout)

    def test_a_tree_with_no_configs_is_an_error(self) -> None:
        self.build()
        for name in ("playwright.config.ts", "playwright.git.config.ts"):
            (self.ui / name).unlink()
        proc = run_gate(Path(self._tmp.name))
        self.assertNotEqual(proc.returncode, 0, proc.stdout)
        self.assertIn("no `playwright*.config.*` file", proc.stdout)

    def test_a_missing_package_dir_is_a_usage_error(self) -> None:
        proc = subprocess.run([sys.executable, str(GATE), "--root", self._tmp.name,
                               "--ui-dir", "nope"], capture_output=True, text=True)
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("is not a directory", proc.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
