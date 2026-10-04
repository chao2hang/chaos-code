#!/usr/bin/env python3
"""Reject a browser spec that no Playwright configuration will run.

`npm run test:e2e` in `apps/chaos-ui` is `node ./e2e-runner.mjs`, which loops over a hand-written
list of Playwright config files and hands each one to `playwright test --config`. Each config then
decides what to collect by matching `testMatch` against the file path. That is the entire
registration mechanism, and it fails quietly in both directions:

- a `*.pw.ts` that no pattern matches is not an error. Playwright collects the files it does match,
  reports the tests it found, and exits 0. A spec can be committed, driven by hand, and be green,
  and still appear in no CI run ever again;
- a pattern that matches nothing is not an error either, so a name left in a list after the spec it
  named was renamed keeps claiming coverage that is no longer there.

As of 2026-10-05 the suite is 14 spec files across 2 configs, and the names are maintained in five
places: the top-level `testMatch` of `playwright.config.ts`, that same list re-typed inside each of
its three projects, and one `const` in `playwright.git.config.ts`. `docs/ci-test-debt.md` recorded
this as the one gap left open by the batch that added `commit-message-safe-mode.pw.ts`: it had to be
added to that `const` by hand, and forgetting it would have printed 12 passed where 14 were expected.

The likeliest way to get it wrong here is not a typo but Playwright's own precedence: a project's
`testMatch` replaces the top-level one rather than narrowing it, and all three projects in
`playwright.config.ts` set their own. Adding a name only at the top level -- where a reader looks
first -- registers nothing at all, and the pattern still looks alive because it matches the other
twelve files. Check 2 is that case, and it is why patterns are judged where they were written
instead of only as the union that happens to run.

What is checked, all of it a fact about text and file names, so this runs in the container, which
cannot start a browser:

1. Every spec is claimed by exactly one config. Claimed means a `testMatch` that Playwright will
   actually consult for that config matches the path, and the spec sits under that config's
   `testDir`. Two configs claiming one spec means `npm run test:e2e` runs it twice, against two
   different hosts, one of which the spec was not written for.
2. A config whose every project overrides `testMatch` is asked whether the top-level list selects
   anything the projects do not, and says so if it does.
3. Every project collects at least one spec. A viewport variant matching nothing is invisible,
   because Playwright only complains when nothing at all is found.
4. Every pattern matches at least one spec, and so does every alternative inside a `(?:a|b|c)`
   group: the stale-name case, judged per pattern and per name.
5. The runner's `configs` list is the set of config files on disk, with no duplicates and no
   missing file. CI types `npm run test:e2e`, not a config path, so a config the runner does not
   name is a config whose specs never run.
6. `package.json`'s `test:e2e` still invokes that runner. A script that went back to a bare
   `playwright test` runs one config and reports a clean suite.

Anything this guard cannot read is a failure rather than a skip: no configs, no specs, a `testMatch`
that is neither a regex literal, nor an array of them, nor a `const` naming one, a glob string, an
unresolvable `const`, an uncompilable pattern. Going blind here is worse than a false alarm, because
the symptom of going blind is the green run this gate exists to doubt.

Spec names come from the filesystem rather than from git's index, because Playwright reads the
filesystem: a spec nobody staged is still collected, and one nobody will ever stage still runs,
which is why being unregistered is a real hazard and not a bookkeeping nit. Paths are matched the
way Playwright matches them, against the absolute path.

Usage: python3 scripts/ci/check-e2e-registration.py [--root DIR] [--ui-dir DIR] [--list]
Exit: 0 = every spec is claimed by exactly one config that the runner actually names.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import NamedTuple

UI_DIR = "apps/chaos-ui"
RUNNER = "e2e-runner.mjs"
RUNNER_SCRIPT = "test:e2e"
CONFIG_NAME_RE = re.compile(r"playwright[^/]*\.config\.[cm]?[jt]s$")
SPEC_NAME_RE = re.compile(r"[^/]*\.pw\.[cm]?[jt]s$")
# A dependency's own specs are nobody's suite, and Playwright would not scan them either, since both
# configs put `testDir` inside the package. Dot-directories are tool scratch space, which is where
# the traces of previous runs land (`.chaos/playwright-results/`).
SKIP_DIRS = frozenset({"node_modules"})
JS_FLAGS = {"i": re.IGNORECASE, "m": re.MULTILINE, "s": re.DOTALL}
# A `/` opens a regular-expression literal only where a value can start. Anywhere else it divides.
# This is the distinction that decides whether a commented-out line can fake a registration.
VALUE_START_AFTER = "=:[,({!?;+-*%&|<>~^"
IDENT_RE = re.compile(r"[A-Za-z_$][A-Za-z0-9_$]*")
CONST_RE = re.compile(r"\b(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*")
MATCH_RE = re.compile(r"\btestMatch\s*:")
PROJECTS_RE = re.compile(r"\bprojects\s*:\s*\[\s*\{")
NAME_RE = re.compile(r"\bname\s*:\s*(['\"])(.*?)\1")
TEST_DIR_RE = re.compile(r"\btestDir\s*:\s*(['\"])(.*?)\1")
ALTERNATION_RE = re.compile(r"\(\?:")
CONFIGS_ARRAY_RE = re.compile(r"\bconfigs\s*=\s*\[")


# -- reading JavaScript text without a JavaScript parser -------------------------------------------------

def _read_string(text: str, i: int) -> tuple[str, int]:
    """The literal starting at the quote `text[i]`, and the index just past it."""
    quote = text[i]
    out = [quote]
    j, n = i + 1, len(text)
    while j < n:
        if text[j] == "\\":
            out.append(text[j:j + 2])
            j += 2
            continue
        if text[j] == quote:
            out.append(quote)
            return "".join(out), j + 1
        out.append(text[j])
        j += 1
    return "".join(out), j


def _regex_start(text: str, i: int) -> bool:
    """Whether the `/` at `i` opens a regex literal rather than dividing."""
    if text[i:i + 1] != "/":
        return False
    if text[i + 1:i + 2] in ("/", "*"):
        return False
    j = i - 1
    while j >= 0 and text[j].isspace():
        j -= 1
    return j < 0 or text[j] in VALUE_START_AFTER


def _read_regex(text: str, i: int) -> tuple[str | None, int]:
    """The `/pattern/flags` literal at `i`, or `None` if it does not close on its own line."""
    if text[i + 1:i + 2] in ("/", "*"):
        return None, i + 1
    j, n = i + 1, len(text)
    in_class = False
    while j < n:
        char = text[j]
        if char == "\\":
            j += 2
            continue
        if char == "\n":
            return None, i + 1
        if char == "[":
            in_class = True
        elif char == "]":
            in_class = False
        elif char == "/" and not in_class:
            # Every flag JavaScript defines, including the ones this reader refuses to guess at:
            # stopping at an unknown flag would silently read the pattern without its meaning.
            flags = re.match(r"[dgimsuvy]*", text[j + 1:])
            end = j + 1 + flags.end()
            return text[i:end], end
        j += 1
    return None, i + 1


def strip_js_comments(text: str) -> str:
    """`text` with comments removed and literals intact, keeping the line count.

    Newlines inside a removed block comment are kept, so an offset in the result still maps to the
    line a message should name. A template literal is read to its closing backtick, `${}`
    substitutions included, so a `${port}` holding a slash cannot be mistaken for a comment.
    """
    out: list[str] = []
    i, n = 0, len(text)
    while i < n:
        char = text[i]
        if char == "/" and text[i + 1:i + 2] == "/":
            newline = text.find("\n", i)
            i = n if newline < 0 else newline
            continue
        if char == "/" and text[i + 1:i + 2] == "*":
            close = text.find("*/", i + 2)
            end = n if close < 0 else close + 2
            out.append("\n" * text[i:end].count("\n"))
            i = end
            continue
        if char in "\"'`":
            literal, i = _read_string(text, i)
            out.append(literal)
            continue
        if _regex_start(text, i):
            literal, after = _read_regex(text, i)
            if literal is not None:
                out.append(literal)
                i = after
                continue
        out.append(char)
        i += 1
    return "".join(out)


def _skip_space(text: str, i: int) -> int:
    while i < len(text) and text[i].isspace():
        i += 1
    return i


def _skip_literal(text: str, i: int) -> int:
    """The index past a string or regex literal at `i`, or `i` if there is none."""
    if text[i:i + 1] in ("'", '"', "`"):
        return _read_string(text, i)[1]
    if _regex_start(text, i):
        literal, after = _read_regex(text, i)
        if literal is not None:
            return after
    return i


def _match_bracket(text: str, i: int, opening: str, closing: str) -> int:
    """The index of the delimiter closing `text[i]`, or -1. Literals are skipped."""
    depth = 0
    j, n = i, len(text)
    while j < n:
        after = _skip_literal(text, j)
        if after != j:
            j = after
            continue
        char = text[j]
        if char == opening:
            depth += 1
        elif char == closing:
            depth -= 1
            if depth == 0:
                return j
        j += 1
    return -1


def _split_top_level(text: str, sep: str) -> list[tuple[int, int]]:
    """Spans of `text` cut at `sep` outside any nesting, so a pattern containing `|` survives."""
    spans: list[tuple[int, int]] = []
    depth = 0
    start = 0
    i, n = 0, len(text)
    while i < n:
        after = _skip_literal(text, i)
        if after != i:
            i = after
            continue
        char = text[i]
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif char == sep and depth == 0:
            spans.append((start, i))
            start = i + 1
        i += 1
    spans.append((start, n))
    return spans


# -- the shapes this guard reads -------------------------------------------------------------------------

#: One `testMatch` entry: `("regex", source, flags)` or `("glob", text, "")`, or `("opaque", ...)`.
Value = tuple[str, str, str]


class Project(NamedTuple):
    """One entry of a config's `projects` array."""
    name: str
    line: int
    values: list[Value]


def _parse_value(text: str, i: int, where: str, problems: list[str]) -> Value | None:
    """One pattern literal, one quoted glob, or one identifier, parsed at `i`."""
    i = _skip_space(text, i)
    if i >= len(text):
        problems.append(f"{where}: the value is missing")
        return None
    if text[i] == "/":
        literal, _ = _read_regex(text, i)
        if literal is None:
            problems.append(f"{where}: a `/` at offset {i} is not a regex literal that closes on "
                            f"its own line")
            return None
        body = literal[1:]
        slash = body.rindex("/")
        return ("regex", body[:slash], body[slash + 1:])
    if text[i] in "\"'`":
        literal, _ = _read_string(text, i)
        return ("glob", literal, "")
    match = IDENT_RE.match(text, i)
    if match:
        return ("ident", match.group(0), "")
    return ("opaque", text[i:i + 24].split(",")[0].split("}")[0].strip() or "?", "")


def compile_pattern(value: Value, where: str, problems: list[str]) -> re.Pattern[str] | None:
    kind, source, flags = value
    unknown = set(flags) - set(JS_FLAGS)
    if unknown:
        problems.append(f"{where}: /{source}/{flags} uses flag(s) "
                        f"{', '.join(sorted(unknown))}, which this reader does not apply; extend "
                        f"JS_FLAGS rather than judge a pattern by a meaning you did not give it")
        return None
    try:
        return re.compile(source, sum(JS_FLAGS[f] for f in set(flags)))
    except re.error as exc:
        problems.append(f"{where}: /{source}/ does not compile as a Python regular expression "
                        f"({exc}); a pattern Playwright can read should compile here too")
        return None


def alternation_variants(source: str) -> list[tuple[str, str]]:
    """Each branch of each `(?:a|b)` group, as (name, pattern carrying only that branch).

    Only the alternative is substituted, so the rest of the pattern -- the `\\.pw\\.ts` suffix above
    all -- still applies to it. A group without a `|`, such as the `(?:-safe-mode)?` of
    `playwright.git.config.ts`, is one optional piece of a single name and not a list of names, so
    it is left alone.
    """
    out: list[tuple[str, str]] = []
    for match in ALTERNATION_RE.finditer(source):
        close = _match_bracket(source, match.start(), "(", ")")
        if close < 0:
            continue
        branches = _split_top_level(source[match.end():close], "|")
        if len(branches) < 2:
            continue
        for start, end in branches:
            branch = source[match.end():close][start:end]
            out.append((branch, source[:match.start()] + branch + source[close + 1:]))
    return out


class Config:
    """One `playwright*.config.*` file, read for the four facts that decide what runs."""

    def __init__(self, path: Path, problems: list[str]) -> None:
        self.path = path
        self.name = path.name
        self.problems = problems
        self.code = strip_js_comments(path.read_text(encoding="utf-8"))
        self.test_dir = self._test_dir()
        self.consts: dict[str, list[Value]] = {}
        self.const_at: dict[str, int] = {}
        self._read_consts()
        self.projects: list[Project] = []
        self.top: list[Value] = []
        self._read()

    # -- text helpers ---------------------------------------------------------------------------------

    def line_of(self, offset: int) -> int:
        return self.code[:offset].count("\n") + 1

    def complain(self, offset: int, message: str,
                 sink: list[str] | None = None) -> None:
        (self.problems if sink is None else sink).append(
            f"{self.name}:{self.line_of(offset) if offset else 1}: {message}")

    def _test_dir(self) -> Path:
        found = TEST_DIR_RE.search(self.code)
        if not found:
            return self.path.parent.resolve()
        return (self.path.parent / found.group(2)).resolve()

    # -- values ---------------------------------------------------------------------------------------

    def _read_consts(self) -> None:
        """Names this file binds to a pattern, because `testMatch: gitSpec` is the house style.

        The position of each binding is kept and compared against the use, so a `const` written
        after the `testMatch` that names it stays unresolved. That is the JavaScript reading too: a
        `const` has a temporal dead zone, and a pattern pointing at a name bound later resolves to
        nothing at the moment the config is loaded.

        Only a binding whose value is a pattern is recorded, and a binding whose value is a port
        number or a `process.env` read says nothing here: this table exists to resolve the names
        `testMatch` uses, not to audit the config.
        """
        scratch: list[str] = []
        for found in CONST_RE.finditer(self.code):
            values = self._values(found.end(), f"`{found.group(1)}`", sink=scratch)
            patterns = [value for value in values if value[0] == "regex"]
            if patterns:
                self.consts[found.group(1)] = patterns
                self.const_at[found.group(1)] = found.start()

    def _values(self, at: int, where: str,
                sink: list[str] | None = None) -> list[Value]:
        """The `testMatch` value at `at`: a literal, an array of them, or a name bound to either.

        `where` names the place being read, such as ``top-level `testMatch` ``; the file and line
        are added by `complain`, from the offset, so a message never has to guess them.
        """
        report = self.problems if sink is None else sink
        code = self.code
        i = _skip_space(code, at)
        if i >= len(code):
            self.complain(at, f"{where}: the value is missing", report)
            return []
        if code[i] == "[":
            close = _match_bracket(code, i, "[", "]")
            if close < 0:
                self.complain(i, f"{where}: the array is never closed", report)
                return []
            inner, base = code[i + 1:close], i + 1
            out: list[Value] = []
            for start, end in _split_top_level(inner, ","):
                if not inner[start:end].strip():
                    continue
                value = _parse_value(inner, start, where, report)
                out.extend(self._resolve(value, where, base + start, report))
            return out
        return self._resolve(_parse_value(code, i, where, report), where, i, report)

    def _resolve(self, value: Value | None, where: str, offset: int,
                 sink: list[str]) -> list[Value]:
        if value is None:
            return []
        kind, text, _flags = value
        if kind == "regex":
            return [value]
        if kind == "ident":
            bound = self.consts.get(text)
            if bound and self.const_at[text] > offset:
                bound = None
            if not bound:
                self.complain(offset, f"{where}: points at `{text}`, which this file never binds "
                                      f"to a pattern before that point; a name resolving to "
                                      f"nothing registers nothing", sink)
                return []
            return bound
        if kind == "glob":
            self.complain(offset, f"{where}: `testMatch` is the glob {text}; this reader "
                                  f"understands a regex literal, an array of them, or a `const` "
                                  f"naming one. Extend it rather than let a pattern go unread",
                          sink)
            return []
        self.complain(offset, f"{where}: `testMatch` is `{text}`, which this reader cannot parse; "
                              f"extend it rather than let a pattern go unread", sink)
        return []

    # -- structure ------------------------------------------------------------------------------------

    def _project_spans(self) -> list[tuple[str, int, int]]:
        found = PROJECTS_RE.search(self.code)
        if not found:
            return []
        open_at = self.code.index("[", found.start())
        close = _match_bracket(self.code, open_at, "[", "]")
        if close < 0:
            self.complain(found.start(), "the `projects` array is never closed")
            return []
        body, base = self.code[open_at + 1:close], open_at + 1
        out: list[tuple[str, int, int]] = []
        for start, end in _split_top_level(body, ","):
            item = body[start:end]
            brace = item.find("{")
            if brace < 0:
                continue
            stop = _match_bracket(item, brace, "{", "}")
            if stop < 0:
                self.complain(base + start, "a `projects` entry is never closed")
                continue
            obj = item[brace:stop + 1]
            named = NAME_RE.search(obj)
            name = named.group(2) if named else f"project #{len(out) + 1}"
            out.append((name, base + start + brace, base + start + stop + 1))
        return out

    def _read(self) -> None:
        """Every `testMatch`, filed under the project that wrote it or under the top level."""
        spans = self._project_spans()
        for name, start, end in spans:
            values: list[Value] = []
            for found in MATCH_RE.finditer(self.code[start:end]):
                # `found` indexes the slice, so the offset has to be put back before it is read.
                values += self._values(start + found.end(), f"project `{name}` `testMatch`")
            self.projects.append(Project(name, self.line_of(start), values))

        def inside(offset: int) -> bool:
            return any(first <= offset < last for _, first, last in spans)

        for found in MATCH_RE.finditer(self.code):
            if inside(found.start()):
                continue
            self.top += self._values(found.end(), "top-level `testMatch`")

    # -- what Playwright would collect ----------------------------------------------------------------

    def compiled(self, value: Value, where: str) -> re.Pattern[str] | None:
        if value[0] != "regex":
            return None
        return compile_pattern(value, where, self.problems)

    def patterns(self) -> list[tuple[Value, str, int]]:
        """(value, label, line) for every pattern this file writes, for the stale-name check."""
        out: list[tuple[Value, str, int]] = []
        for value in self.top:
            out.append((value, "top-level `testMatch`", 0))
        for project in self.projects:
            for value in project.values:
                out.append((value, f"project `{project.name}` `testMatch`", project.line))
        return out

    def collects(self, specs: list[Path]) -> list[tuple[Path, str]]:
        """(spec, who selected it) for the specs this config runs, as Playwright would."""
        hits: list[tuple[Path, str]] = []
        for project in self.projects or [Project("", 0, [])]:
            values = project.values or self.top
            for value in values:
                label = f"project `{project.name}`" if project.name else "top-level `testMatch`"
                pattern = self.compiled(value, f"{self.name} ({label})")
                if pattern is None:
                    continue
                for spec in specs:
                    if _under(spec, self.test_dir) and pattern.search(str(spec)):
                        hits.append((spec, label))
        return hits


def _under(spec: Path, directory: Path) -> bool:
    return directory in spec.parents


# -- the rule --------------------------------------------------------------------------------------------------

def spec_files(ui_dir: Path) -> list[Path]:
    out: list[Path] = []
    for path in sorted(ui_dir.rglob("*")):
        if not path.is_file() or not SPEC_NAME_RE.fullmatch(path.name):
            continue
        if SKIP_DIRS & set(path.parts):
            continue
        if any(part.startswith(".") for part in path.relative_to(ui_dir).parts):
            continue
        out.append(path)
    return out


def config_files(ui_dir: Path) -> list[Path]:
    return sorted(path for path in ui_dir.iterdir() if CONFIG_NAME_RE.fullmatch(path.name))


def check(ui_dir: Path, problems: list[str]) -> dict[Path, list[str]]:
    specs = spec_files(ui_dir)
    configs = [Config(path, problems) for path in config_files(ui_dir)]
    if not specs:
        problems.append(f"{ui_dir.name}: no `*.pw.ts` spec file anywhere under it, so there is no "
                        f"suite to register; a guard that found nothing must not report success")
    if not configs:
        problems.append(f"{ui_dir.name}: no `playwright*.config.*` file, so nothing decides what "
                        f"`playwright test` collects")
        return {}

    claims: dict[Path, list[str]] = {spec: [] for spec in specs}
    for config in configs:
        # Check 1, with the selected-by lists kept apart because check 2 needs the contrast.
        selected = config.collects(specs)
        for spec, label in selected:
            owner = f"{config.name} ({label})"
            if owner not in claims[spec]:
                claims[spec].append(owner)

        # Check 2: the top level selects a spec that no project will, because every project
        # replaces it. This is the shape that reads as registered and is not.
        if config.projects and config.top:
            by_projects = {spec for spec, label in selected if label != "top-level `testMatch`"}
            for value in config.top:
                pattern = config.compiled(value, f"{config.name} (top-level `testMatch`)")
                if pattern is None:
                    continue
                for spec in specs:
                    if _under(spec, config.test_dir) and pattern.search(str(spec)) \
                            and spec not in by_projects:
                        problems.append(f"{spec.relative_to(ui_dir)}: the top-level `testMatch` of "
                                        f"{config.name} matches it, but every project in that "
                                        f"config sets its own `testMatch`, which replaces the "
                                        f"top level, so the config collects nothing for it")

        # Check 3: a project that collects nothing is invisible from the run itself.
        for project in config.projects:
            values = project.values or config.top
            hits = 0
            for value in values:
                pattern = config.compiled(value, f"{config.name} (project `{project.name}`)")
                if pattern is None:
                    continue
                hits += sum(1 for spec in specs
                            if _under(spec, config.test_dir) and pattern.search(str(spec)))
            if not hits:
                problems.append(f"{config.name}:{project.line}: project `{project.name}` collects "
                                f"no spec file, so the viewport or browser it configures is never "
                                f"exercised")
        if not config.projects and not config.top:
            problems.append(f"{config.name}: no `testMatch` and no `projects`, so Playwright's "
                            f"default pattern applies, and no `*.pw.ts` matches it")

        # Check 4: stale names, per pattern and per alternative inside it.
        for value, label, line in config.patterns():
            where = f"{config.name} ({label})"
            pattern = config.compiled(value, where)
            if pattern is None:
                continue
            if not [spec for spec in specs if _under(spec, config.test_dir)
                    and pattern.search(str(spec))]:
                prefix = f"{config.name}:{line}: " if line else f"{config.name}: "
                problems.append(f"{prefix}{label} pattern /{value[1]}/ matches no spec under "
                                f"{config.test_dir.name}/, so it is a name nothing satisfies any "
                                f"more")
            for name, variant in alternation_variants(value[1]):
                compiled = compile_pattern(("regex", variant, value[2]), where, problems)
                if compiled is None:
                    continue
                if not [spec for spec in specs if _under(spec, config.test_dir)
                        and compiled.search(str(spec))]:
                    prefix = f"{config.name}:{line}: " if line else f"{config.name}: "
                    problems.append(f"{prefix}`{name}` inside {label} pattern /{value[1]}/ matches "
                                    f"no spec under {config.test_dir.name}/")

    explained: set[Path] = set()
    for problem in problems:
        for spec in specs:
            if problem.startswith(f"{spec.relative_to(ui_dir)}:"):
                explained.add(spec)
    for spec in specs:
        owners = sorted({owner.split(" (")[0] for owner in claims[spec]})
        if len(owners) > 1:
            problems.append(f"{spec.relative_to(ui_dir)}: claimed by {' and '.join(owners)}; "
                            f"`npm run test:e2e` runs every config in turn, so this spec would run "
                            f"twice, against two different hosts")
        elif not owners and spec not in explained:
            under = [config for config in configs if _under(spec, config.test_dir)]
            if not under:
                reason = ("it sits outside every config's `testDir`, so no pattern there can "
                          "reach it")
            else:
                # The projects re-type the top-level list on purpose, so the same pattern is
                # usually on the table several times over. Saying it three times makes the
                # finding unreadable without saying anything more.
                listed: list[str] = []
                for config in under:
                    for value, _label, _line in config.patterns():
                        candidate = f"{config.name} pattern /{value[1]}/"
                        if candidate not in listed:
                            listed.append(candidate)
                reason = "the patterns on the table were " + "; ".join(listed)
            problems.append(f"{spec.relative_to(ui_dir)}: no config claims it; {reason}. It would "
                            f"never run, and the run would exit 0")

    check_runner(ui_dir, [config.path for config in configs], problems)
    return claims


def check_runner(ui_dir: Path, configs: list[Path], problems: list[str]) -> None:
    """Checks 5 and 6, the two hand-written lists sitting above the configs."""
    runner = ui_dir / RUNNER
    if not runner.is_file():
        problems.append(f"{RUNNER}: not found, so the list of configs the suite runs does not exist")
    else:
        code = strip_js_comments(runner.read_text(encoding="utf-8"))
        found = CONFIGS_ARRAY_RE.search(code)
        if not found:
            problems.append(f"{RUNNER}: no `configs = [...]` array, so which configs run cannot be "
                            f"read here; extend this guard rather than let that list go unseen")
        else:
            open_at = code.index("[", found.start())
            close = _match_bracket(code, open_at, "[", "]")
            if close < 0:
                problems.append(f"{RUNNER}: the `configs` array is never closed")
            else:
                body = code[open_at + 1:close]
                listed: list[str] = []
                for start, end in _split_top_level(body, ","):
                    item = re.search(r"(['\"`])(.*?)\1", body[start:end])
                    if item:
                        listed.append(item.group(2))
                seen: set[str] = set()
                for name in listed:
                    if name in seen:
                        problems.append(f"{RUNNER}: `{name}` is listed twice, so its suite runs "
                                        f"twice in one `npm run test:e2e`")
                    seen.add(name)
                    if not (ui_dir / name).is_file():
                        problems.append(f"{RUNNER}: lists `{name}`, which is not a file, so that "
                                        f"`--config` argument fails the run")
                for path in configs:
                    if path.name not in seen:
                        problems.append(f"{path.name}: nothing runs it -- {RUNNER} lists "
                                        f"{', '.join(sorted(seen)) or 'nothing'} -- so its specs are "
                                        f"absent from `npm run test:e2e`, which is what CI types")

    package = ui_dir / "package.json"
    if not package.is_file():
        problems.append("package.json: not found, so the entry point of the suite is unknown")
        return
    try:
        scripts = json.loads(package.read_text(encoding="utf-8")).get("scripts", {})
    except json.JSONDecodeError as exc:
        problems.append(f"package.json: does not parse ({exc})")
        return
    script = scripts.get(RUNNER_SCRIPT)
    if script is None:
        problems.append(f"package.json: no `{RUNNER_SCRIPT}` script, so nothing in the package "
                        f"starts the browser suite")
    elif RUNNER not in script:
        problems.append(f"package.json: `{RUNNER_SCRIPT}` is `{script}`, which does not invoke "
                        f"`{RUNNER}`; the runner holds the list of configs, so a bare "
                        f"`playwright test` runs one config and calls the suite green")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=".", type=Path)
    parser.add_argument("--ui-dir", default=UI_DIR, type=Path,
                        help="the package holding the specs, relative to --root")
    parser.add_argument("--list", action="store_true", help="print what claims each spec and exit")
    ns = parser.parse_args(argv)

    ui_dir = ns.root.resolve() / ns.ui_dir
    if not ui_dir.is_dir():
        print(f"check-e2e-registration: {ui_dir} is not a directory; run this from the repository "
              f"root or pass --ui-dir", file=sys.stderr)
        return 2

    problems: list[str] = []
    claims = check(ui_dir, problems)
    if ns.list:
        for spec in sorted(claims):
            print(f"{spec.relative_to(ui_dir)}\t{' + '.join(claims[spec]) or 'UNCLAIMED'}")
        return 0

    if problems:
        print(f"check-e2e-registration: FAIL ({len(problems)} problem(s))")
        for problem in problems:
            print(f"  {problem}")
        return 1
    print(f"check-e2e-registration: OK ({len(claims)} spec file(s), each claimed by exactly one of "
          f"{len(config_files(ui_dir))} config(s) that {RUNNER} names)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
