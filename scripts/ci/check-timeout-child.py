#!/usr/bin/env python3
"""Reject a child process that a timeout is allowed to walk away from.

The premise is the dependency's own text. `tokio-1.52.3/src/process/mod.rs:201-203` says it in
the module documentation:

    Similar to the behavior to the standard library, and unlike the futures
    paradigm of dropping-implies-cancellation, a spawned process will, by
    default, continue to execute even after the `Child` handle has been dropped.

`Command::kill_on_drop` is what changes that, and the default is off -- `:641`: "By default,
this value is assumed to be `false`, meaning the next spawned process will not be killed on
drop". `Command::output()` and `Command::status()` call `self.spawn()` themselves, at `:1069`
and `:1003`, so the child exists before the future is ever awaited, and each documents the one
way out -- `:1037` and `:974`: "The destructor of the future returned by this function will kill
the child if [`kill_on_drop`] is set to true." So `timeout(budget, cmd.output())` stops the child
only if somebody said so on `cmd`; otherwise it stops the *waiting* and leaves the work running.

Nothing else in this repository sees the omission:

- the compiler: `kill_on_drop` is an ordinary `&mut self` builder method, and leaving a builder
  method out of a chain has never been a type error.
- the lint that looks like it covers this. `clippy.toml` bans `std::process::Command::spawn`
  and `tokio::process::Command::spawn` with the reason "an unenrolled child outlives its
  session; use xai_tty_utils::ProcessScope::enroll". But `ProcessScope::enroll` and `enroll_std`
  (`crates/codegen/xai-tty-utils/src/process_scope.rs:137,158`) both take `&Child`, and
  `.output()` / `.status()` never hand one out. The banned call is the one call that has an
  alternative; the two calls with no alternative are not on the list. A site can satisfy the
  lint and still orphan its child, so this script checks the half the lint cannot reach and
  refuses to let the other half be read as the whole rule.
- the test suite: a test that takes the timeout asserts on the timeout, so it goes green while
  the child it abandoned keeps running.
- the numbers, which look tidy either way. Of the 12 timeout-abandoned `.output()`/`.status()`
  sites on the tree this was written against, 10 already killed their child -- 8 through the
  `git_command` builder at `xai-grok-shell/src/session/goal_classifier/evidence.rs:49` and two
  through `detach_search_command` (`xai-tty-utils/src/lib.rs:216`) and `kill_on_drop(true)` set
  inline in `auto_update.rs`. Two did not, behind the identical `timeout(budget, cmd.output())`
  shape, and nothing outside this script distinguished the two groups.
- `detach_command`, which makes an unmarked site worse rather than better: it `setsid`s the
  child into a new session, so once the future is dropped there is no process group left that
  the parent's teardown could signal. One of the two unmarked sites was exactly that shape.

The rule, kept as narrow as the evidence: where a `timeout(...)` call is handed a future that
ends in `.output()` or `.status()` on a `tokio::process::Command`, that command has to be marked
`kill_on_drop(true)` on the path from `Command::new` to the call. That path is read three ways:
the method chain itself, a local binding plus the statements that touch it afterwards, and a
builder or mutator function whose body is readable. Helper names are resolved the way Rust
resolves them -- inside the calling crate first, then through an explicit `other_crate::` prefix,
and only then by a repository-unique name -- because `git_command` alone is defined in three
crates here and two of them build a `std::process::Command`.

Not covered, stated rather than pretended:

- a command reaching `.output()` without a timeout. Cancellation can still come from the
  surrounding task, but "somebody might drop this task" is not a locally checkable property, so
  those sites are left alone. This gate judges only the sites where the code itself schedules
  the drop.
- a future wrapped before it reaches `timeout`: `timeout(d, run(cmd))` or
  `timeout(d, cmd.output().map(..))`. If the identifier in front of `.output()` names a command
  this function bound with `let mut` (the only form `Command::output(&mut self)` can be
  called on), the site is reported as `unreadable-future`; otherwise the call is not about a
  child at all (a test's `drained.output()` returning a tool output is the real example) and
  is left alone.
- a command that is not a plain local (`self.cmd`, a field, a `&mut` parameter of another
  function): `unreadable-receiver`.
- `std::process::Command`: its `.output()` blocks the calling thread until the child exits, so
  there is no future to walk away from and the rule does not apply. Those sites are counted as
  `std commands` in the summary line so the two kinds are never added together.
- what the flag itself does. `kill_on_drop` signals the child's pid, not its group, so a child
  that forks can still leave grandchildren behind. `ProcessScope` is the answer for that case.

Usage: python3 scripts/ci/check-timeout-child.py [--root DIR] [--allowlist FILE]
                                                 [--print] [--quiet] [--json]
Exit: 0 = every timeout-abandoned child either dies with its future or is recorded with a reason.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

TIMEOUT_CALL = re.compile(r"\b(?:[A-Za-z_]\w*::)*timeout(?:_at)?\s*\(")
# The abandoned future has to end in the spawn-and-wait call itself.
ABANDONED_TAIL = re.compile(r"\.(output|status)\s*\(\s*\)\s*$")
ANY_TAIL = re.compile(r"(?:^|[^:\w])([A-Za-z_]\w*)\.(output|status)\s*\(\s*\)")
PLAIN_BINDING = re.compile(r"^([A-Za-z_]\w*)$")
CALL_HEAD = re.compile(r"^(?:(?P<quals>[A-Za-z_]\w*::)*)?(?P<name>[A-Za-z_]\w*)\s*\(")
COMMAND_NEW = re.compile(r"\bCommand::new\b")
NEW_TOKIO_QUALIFIED = re.compile(r"\btokio::process::Command::new\b")
NEW_STD_QUALIFIED = re.compile(r"\bstd::process::Command::new\b")
TOKIO_MODULE = "tokio::process"
STD_MODULES = ("std::process", "core::process")
USE_STMT = re.compile(r"^[ \t]*use\s+([^;]+);", re.MULTILINE)
KILL_TRUE = re.compile(r"\.\s*kill_on_drop\s*\(\s*true\s*\)")
KILL_FALSE = re.compile(r"\.\s*kill_on_drop\s*\(\s*false\s*\)")
FN_DEF = re.compile(r"\bfn\s+([A-Za-z_]\w*)\s*(?:<[^>]*>)?\s*\(", re.MULTILINE)
PACKAGE_NAME = re.compile(r'^\[package\][^\[]*?^name\s*=\s*"([^"]+)"', re.MULTILINE | re.DOTALL)
AMBIGUOUS = "?"
NO_ANSWER = ""

# What the ledger may record. The first two are the defect; the rest are "this gate cannot
# tell", and each of them has to be answered by a person once rather than by the tool guessing.
CATEGORIES = (
    "no-kill-on-drop",
    "kill-on-drop-false",
    "unreadable-future",
    "unreadable-receiver",
    "unknown-helper",
    "ambiguous-command",
)
REASON_MIN = 20
SKIP_DIRS = {"target", ".git", "node_modules", "dist", "build", ".cargo"}

# The clippy ban this gate pairs up with. Its two `Command::spawn` entries are what makes
# "clippy is clean" mean something about `.spawn()`; if they go away, the argument in the
# docstring above is wrong and has to be rewritten together with the config.
CLIPPY_SPAWN_BANS = ("std::process::Command::spawn", "tokio::process::Command::spawn")


def blank_comments(text: str) -> str:
    """Blank comments, keep string literals.

    Comments have to go, or a commented-out `timeout(` reads as a live site. Strings have to
    stay, because `balanced` has to be able to tell `"a ) b"` from real code.
    """
    out: list[str] = []
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            j = text.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
            i = j
        elif c == "/" and i + 1 < n and text[i + 1] == "*":
            j = text.find("*/", i + 2)
            j = n if j < 0 else j + 2
            out.append("".join(ch if ch == "\n" else " " for ch in text[i:j]))
            i = j
        elif c in "\"'":
            quote, j = c, i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == quote:
                    j += 1
                    break
                j += 1
            out.append(text[i:j])
            i = j
        else:
            out.append(c)
            i += 1
    return "".join(out)


def string_end(text: str, i: int) -> int:
    """The index just past the string literal whose opening quote is at `i`."""
    j = i + 1
    while j < len(text):
        if text[j] == "\\":
            j += 2
            continue
        if text[j] == "\"":
            return j + 1
        j += 1
    return j


def balanced(text: str, open_idx: int) -> tuple[str, int]:
    """The text inside the bracket pair opening at `open_idx`, and its close index."""
    depth, i = 0, open_idx
    while i < len(text):
        c = text[i]
        if c == "\"":
            i = string_end(text, i)
            continue
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
            if depth == 0:
                return text[open_idx + 1:i], i
        i += 1
    return text[open_idx + 1:], i


def split_args(args: str) -> list[str]:
    """Split an argument list on top-level commas, keeping nested brackets intact."""
    parts: list[str] = []
    depth, cur = 0, []
    i, n = 0, len(args)
    while i < n:
        c = args[i]
        if c == "\"":
            j = string_end(args, i)
            cur.append(args[i:j])
            i = j
            continue
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        if c == "," and depth == 0:
            parts.append("".join(cur))
            cur = []
        else:
            cur.append(c)
        i += 1
    parts.append("".join(cur))
    return [p.strip() for p in parts]


def body_span(text: str, fn_start: int) -> tuple[int, int] | None:
    """Where the body of the `fn` starting at `fn_start` begins and ends, or None.

    The scan stops at the first top-level `;` -- a trait or impl declaration has no body, and
    reading on would grab the next function's brace and attribute its body to this name -- or
    at the first top-level `{`. Depth is tracked so the `;` inside a `-> [u8; 4]` return type
    is not mistaken for a body-less declaration.
    """
    _, params_close = balanced(text, text.index("(", fn_start))
    depth, i = 0, params_close + 1
    while i < len(text):
        c = text[i]
        if c == "\"":
            i = string_end(text, i)
            continue
        if c == "{" and depth == 0:
            _, close = balanced(text, i)
            return i, close
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == ";" and depth == 0:
            return None
        i += 1
    return None


def function_spans(text: str) -> list[tuple[str, int, int]]:
    """(name, body start, body end) for every `fn` in the file."""
    spans: list[tuple[str, int, int]] = []
    for m in FN_DEF.finditer(text):
        span = body_span(text, m.start())
        if span is None:
            continue
        spans.append((m.group(1), span[0], span[1]))
    return spans


def enclosing_function(spans: list[tuple[str, int, int]], idx: int) -> tuple[str, int, int] | None:
    """The innermost `fn` whose body contains `idx`, which is the one a reader would name."""
    best: tuple[str, int, int] | None = None
    for name, start, end in spans:
        if start <= idx < end and (best is None or start >= best[1]):
            best = (name, start, end)
    return best


def first_param_is_self(text: str, fn_start: int) -> bool:
    probe = text.index("(", fn_start)
    params, _ = balanced(text, probe)
    head = params.split(",")[0] if params.strip() else ""
    return bool(re.match(r"^[ \t]*(&|self\b)", head))


class Helper:
    """A function that builds or mutates a `Command`, and what it does on drop.

    `kills` means its body calls `kill_on_drop(true)`, `plain` that a readable body does not,
    `false` that it calls `kill_on_drop(false)`, and `?` that the name resolves to two
    definitions that disagree -- refused rather than resolved by picking one.
    """

    def __init__(self, name: str, verdict: str, where: str) -> None:
        self.name = name
        self.verdict = verdict
        self.where = where


class Helpers:
    """Command helpers, indexed the way Rust resolves them.

    Resolution order for a call `a::b::name(...)`: the crate named `a` if a crate by that name
    exists, then the calling crate, then a repository-unique bare name. The calling crate comes
    before the bare-name index because `git_command` here means three different functions in
    three crates and only the crate tells them apart.
    """

    def __init__(self) -> None:
        self.by_crate: dict[tuple[str, str], Helper] = {}
        self.by_name: dict[str, Helper] = {}

    def add(self, crate: str, helper: Helper) -> None:
        prior = self.by_crate.get((crate, helper.name))
        if prior is not None:
            if prior.verdict != helper.verdict:
                self.by_crate[(crate, helper.name)] = Helper(
                    helper.name, AMBIGUOUS, f"{prior.where} and {helper.where}")
            return
        self.by_crate[(crate, helper.name)] = helper
        other = self.by_name.get(helper.name)
        if other is None:
            self.by_name[helper.name] = helper
        elif other.verdict != helper.verdict or other.where != helper.where:
            self.by_name[helper.name] = Helper(
                helper.name, AMBIGUOUS, f"{other.where} and {helper.where}")

    def lookup(self, quals: list[str], name: str, crate: str) -> Helper | None:
        if quals:
            head = quals[0]
            if head in ("crate", "self"):
                found = self.by_crate.get((crate, name))
                if found is not None:
                    return found
            else:
                for (other, other_name), helper in self.by_crate.items():
                    if other_name == name and other.replace("-", "_") == head:
                        return helper
        found = self.by_crate.get((crate, name))
        if found is not None:
            return found
        return self.by_name.get(name)


def command_helpers(text: str, rel: str) -> list[Helper]:
    """Every `fn` in the file whose signature mentions `Command`.

    The signature is what makes a function a command helper at all: `fn git_command(cwd: &Path)
    -> Command`, `fn detach_command(cmd: &mut tokio::process::Command)`. Its body is then read
    for one token, because `kill_on_drop(true)` is the whole question. Methods are left out --
    `x.git_command()` takes no `&mut Command` argument, so it cannot be the mutator this gate
    looks for. The cheap window test in front of `body_span` is not decoration: the tree holds
    tens of thousands of functions, and walking each one's braces to learn that the signature
    never mentioned `Command` is what made the first version of this gate take four minutes.
    """
    if "Command" not in text:
        return []
    out: list[Helper] = []
    for m in FN_DEF.finditer(text):
        if "Command" not in text[m.start():m.start() + 400]:
            continue
        span = body_span(text, m.start())
        if span is None or first_param_is_self(text, m.start()):
            continue
        if "Command" not in text[m.start():span[0]]:
            continue
        body = text[span[0]:span[1]]
        if KILL_FALSE.search(body):
            verdict = "false"
        elif KILL_TRUE.search(body):
            verdict = "kills"
        else:
            verdict = "plain"
        out.append(Helper(m.group(1), verdict,
                          f"{rel}:{text.count(chr(10), 0, m.start()) + 1}"))
    return out


def crate_index(root: Path, files: list[Path]) -> dict[Path, str]:
    """Each source file's Cargo package name, by the nearest `Cargo.toml` above it."""
    manifests: list[tuple[Path, str]] = []
    for manifest in root.rglob("Cargo.toml"):
        if SKIP_DIRS.intersection(manifest.parts):
            continue
        try:
            found = PACKAGE_NAME.search(manifest.read_text(encoding="utf-8"))
        except OSError:
            continue
        if found:
            manifests.append((manifest.parent, found.group(1)))
    manifests.sort(key=lambda m: len(m[0].parts), reverse=True)
    out: dict[Path, str] = {}
    for path in files:
        for directory, name in manifests:
            if directory in path.parents:
                out[path] = name
                break
    return out


class Site:
    """One `timeout(budget, cmd.output())` call and what could be learned about `cmd`."""

    def __init__(self, file: str, line: int, func: str, future: str) -> None:
        self.file = file
        self.line = line
        self.func = func
        self.future = future
        self.category = ""
        self.detail = ""
        self.kills = False
        self.std_command = False

    def key(self) -> str:
        return f"{self.file}::{self.func}"

    def at(self) -> str:
        return f"{self.file}:{self.line}"


def command_bindings(body: str) -> set[str]:
    """Names this function mutably binds to something built from a `Command`.

    A `mut` binding and nothing else: `Command::output` and `Command::status` both take
    `&mut self`, so code that compiles and calls `.output()` on an immutable binding is
    calling some other type's method, and reporting it would teach a reader to ignore
    the finding.
    """
    out: set[str] = set()
    for m in re.finditer(r"\blet\s+(?P<pattern>[^=;]*?)\s*=\s*", body):
        end = m.end()
        depth = 0
        while end < len(body):
            c = body[end]
            if c in "([{":
                depth += 1
            elif c in ")]}":
                depth -= 1
            elif c == ";" and depth == 0:
                break
            end += 1
        value = body[m.end():end]
        pattern = m.group("pattern").split(":")[0]
        names = re.findall(r"\bmut\s+[A-Za-z_]\w*", pattern)
        if names and (COMMAND_NEW.search(value) or re.search(
                r"^(?:(?:[A-Za-z_]\w*::)+)?[A-Za-z_]\w*\s*\(", value.strip())):
            out.update(n.split()[-1] for n in names)
    return out


def answers_from_mutators(region: str, binding: str, helpers: Helpers,
                          crate: str) -> tuple[list[str], str]:
    """What the statements between the binding and the site do to it.

    Two shapes matter: the chain written on the binding itself
    (`cmd.arg(..).kill_on_drop(true);`), and a mutator handed the binding
    (`detach_search_command(&mut command);`). The second is how most correct sites in this
    repository are correct, so missing it would report working code as broken.
    """
    answers: list[str] = []
    direct = re.compile(r"\b" + re.escape(binding) + r"\b[^;]*")
    for m in direct.finditer(region):
        if KILL_TRUE.search(m.group(0)):
            answers.append("kills")
        if KILL_FALSE.search(m.group(0)):
            answers.append("false")
    called = re.compile(r"\b([A-Za-z_][\w:]*)\s*\(\s*&+mut\s+" + re.escape(binding) + r"\s*\)")
    for m in called.finditer(region):
        parts = m.group(1).split("::")
        helper = helpers.lookup(parts[:-1], parts[-1], crate)
        if helper is None:
            return answers, (f"{binding} is mutated by {m.group(1)}(), which has no readable "
                             "Command body to take an answer from")
        if helper.verdict == AMBIGUOUS:
            return answers, f"{m.group(1)}() is defined more than once with different answers"
        answers.append(helper.verdict)
    return answers, ""


def source_verdict(source: str, helpers: Helpers, crate: str, tokio_import: bool,
                   std_import: bool) -> tuple[str, str, str, bool]:
    """Read the expression a command came from: (verdict, category, detail, is_std)."""
    if COMMAND_NEW.search(source):
        if NEW_STD_QUALIFIED.search(source) or (std_import and not tokio_import
                                               and not NEW_TOKIO_QUALIFIED.search(source)):
            return NO_ANSWER, "", "std::process::Command has no droppable future", True
        if tokio_import and std_import and not NEW_TOKIO_QUALIFIED.search(source):
            return (NO_ANSWER, "ambiguous-command",
                    "this file binds the name Command to both std and tokio", False)
        if KILL_TRUE.search(source):
            return "kills", "", "", False
        if KILL_FALSE.search(source):
            return "false", "", "", False
        return "plain", "", "", False
    call = CALL_HEAD.match(source.strip())
    if call:
        quals = [q for q in (call.group("quals") or "").split("::") if q]
        helper = helpers.lookup(quals, call.group("name"), crate)
        if helper is None:
            return (NO_ANSWER, "unknown-helper",
                    f"{call.group('name')}() has no readable Command body to take an answer from",
                    False)
        if helper.verdict == AMBIGUOUS:
            return (NO_ANSWER, "unknown-helper",
                    f"{call.group('name')}() is defined more than once with different answers",
                    False)
        if helper.verdict == "plain" and KILL_TRUE.search(source):
            # `timeout(d, git_command(cwd).kill_on_drop(true).output())` marks it on the chain.
            return "kills", "", f"built by {call.group('name')}() at {helper.where}", False
        return helper.verdict, "", f"built by {call.group('name')}() at {helper.where}", False
    return (NO_ANSWER, "unreadable-receiver",
            f"cannot read {source.strip()[:60]!r} as a command", False)


def analyse(text: str, spans: list[tuple[str, int, int]], helpers: Helpers, crate: str,
            tokio_import: bool, std_import: bool, rel: str, pos: int, future: str) -> Site:
    func = enclosing_function(spans, pos)
    site = Site(rel, text.count("\n", 0, pos) + 1, func[0] if func else "?",
                " ".join(future.split()))
    tail = ABANDONED_TAIL.search(future)
    if tail is None:
        # A wrapped future is still this gate's business when the thing being wrapped is a
        # command built in this same function. `drained.output()` in a test module is not, and
        # reporting it would teach a reader to ignore the finding.
        body = text[func[1]:func[2]] if func else ""
        named = command_bindings(body)
        wrapped = [m.group(1) for m in ANY_TAIL.finditer(future) if m.group(1) in named]
        if not wrapped:
            return Site(rel, 0, site.func, site.future)
        site.category = "unreadable-future"
        site.detail = (f"{wrapped[0]} is a command, but the timeout drops a wrapper around its "
                       "future, so what reaches the child cannot be read here")
        return site
    receiver = future[:tail.start()].strip()
    source = receiver
    mutator_region = ""
    binding = ""

    if not COMMAND_NEW.search(receiver):
        named = PLAIN_BINDING.match(receiver)
        # A builder handed straight to the timeout, `timeout(d, git_command(cwd).output())`, has
        # no local binding to trace, but its body can still answer the question.
        direct_call = named is None and CALL_HEAD.match(receiver.strip()) is not None
        if not direct_call:
            if named is None or func is None:
                site.category = "unreadable-receiver"
                site.detail = f"receiver {receiver[:60]!r} is not a plain local command"
                return site
            binding = named.group(1)
            body = text[func[1]:func[2]]
            offset = pos - func[1]
            init = None
            for m in re.finditer(
                    r"\blet\s+(?:mut\s+)?" + re.escape(binding) + r"\s*(?::[^=;]*)?=\s*", body):
                if m.end() <= offset:
                    init = m
            if init is None:
                site.category = "unreadable-receiver"
                site.detail = f"{binding} is not initialised inside {site.func}()"
                return site
            end = init.end()
            depth = 0
            while end < len(body):
                c = body[end]
                if c in "([{":
                    depth += 1
                elif c in ")]}":
                    depth -= 1
                elif c == ";" and depth == 0:
                    break
                end += 1
            source = body[init.end():end]
            mutator_region = body[end:offset] if offset > end else ""

    verdict, category, detail, is_std = source_verdict(
        source, helpers, crate, tokio_import, std_import)
    if category:
        site.category = category
        site.detail = detail
        return site
    if is_std:
        site.std_command = True
        site.kills = True
        site.detail = detail
        return site

    answers = [verdict]
    if binding:
        mutator_answers, problem = answers_from_mutators(mutator_region, binding, helpers, crate)
        if problem:
            site.category = "unknown-helper"
            site.detail = problem
            return site
        answers.extend(mutator_answers)

    if "false" in answers:
        site.category = "kill-on-drop-false"
        site.detail = detail
    elif "kills" in answers:
        site.kills = True
        site.detail = detail
    else:
        site.category = "no-kill-on-drop"
        site.detail = detail
    return site


def imports_bare_command(text: str, module_prefixes: tuple[str, ...] | str) -> bool:
    """Does this file bind the name `Command` to one of these modules?

    Aliases are excluded on purpose: `use std::process::Command as StdCommand;` inside a test
    module says nothing about what a bare `Command::new` means three hundred lines earlier. The
    first version of this gate read that line as a second `Command` in scope and reported a
    tokio-only file as ambiguous.
    """
    if isinstance(module_prefixes, str):
        module_prefixes = (module_prefixes,)
    for m in USE_STMT.finditer(text):
        path = m.group(1).strip()
        for prefix in module_prefixes:
            head = f"{prefix}::"
            if not path.startswith(head):
                continue
            rest = path[len(head):]
            if rest.startswith("{"):
                for entry in rest[1:rest.rfind("}")].split(","):
                    entry = entry.strip()
                    if entry and " as " not in entry and entry.split("::")[0] == "Command":
                        return True
            elif " as " not in rest and rest.split("::")[0] == "Command":
                return True
    return False


def scan_file(text: str, rel: str, helpers: Helpers, crate: str) -> list[Site]:
    # `function_spans` walks every brace in the file, and the answer is only ever needed for a
    # file that actually hands a spawn-and-wait future to a timeout.
    if not TIMEOUT_CALL.search(text):
        return []
    spans = function_spans(text)
    tokio_import = imports_bare_command(text, TOKIO_MODULE)
    std_import = imports_bare_command(text, STD_MODULES)
    sites: list[Site] = []
    for m in TIMEOUT_CALL.finditer(text):
        args, _ = balanced(text, text.index("(", m.start()))
        parts = split_args(args)
        if len(parts) < 2:
            continue
        future = parts[1]
        if ".output()" not in future and ".status()" not in future:
            continue
        site = analyse(text, spans, helpers, crate, tokio_import, std_import, rel, m.start(),
                       future)
        if site.line:
            sites.append(site)
    return sites


def rust_files(root: Path) -> list[Path]:
    out = []
    for p in sorted(root.rglob("*.rs")):
        if SKIP_DIRS.intersection(p.parts):
            continue
        out.append(p)
    return out


def read_allowlist(path: Path) -> tuple[dict[tuple[str, str], str], list[str]]:
    rows: dict[tuple[str, str], str] = {}
    errors: list[str] = []
    if not path.exists():
        return rows, [f"{path}: not found"]
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        cols = line.split("\t")
        if len(cols) != 3:
            errors.append(f"{path}:{lineno}: expected 3 tab-separated columns")
            continue
        key, category, reason = (c.strip() for c in cols)
        if category not in CATEGORIES:
            errors.append(f"{path}:{lineno}: unknown category {category!r}, use one of {CATEGORIES}")
            continue
        if len(reason) < REASON_MIN:
            errors.append(f"{path}:{lineno}: reason is {len(reason)} characters, "
                          f"write at least {REASON_MIN} so a reader can judge the exemption")
            continue
        rows[(key, category)] = reason
    return rows, errors


def clippy_bans(root: Path) -> list[str]:
    """The `disallowed-methods` paths in the repository's clippy config, as written."""
    path = root / "clippy.toml"
    if not path.exists():
        return []
    return re.findall(r'\{\s*path\s*=\s*"([^"]+)"', path.read_text(encoding="utf-8"))


def check(root: Path, allowlist: Path, show: bool, quiet: bool, as_json: bool) -> int:
    files = rust_files(root)
    if not files:
        print(f"no Rust sources under {root}", file=sys.stderr)
        return 2

    texts: dict[Path, str] = {}
    for path in files:
        try:
            texts[path] = blank_comments(path.read_text(encoding="utf-8"))
        except OSError as exc:
            print(f"cannot read {path}: {exc}", file=sys.stderr)
            return 2

    crates = crate_index(root, files)
    helpers = Helpers()
    for path in files:
        crate = crates.get(path, "?")
        for helper in command_helpers(texts[path], str(path.relative_to(root))):
            helpers.add(crate, helper)

    sites: list[Site] = []
    for path in files:
        try:
            rel = str(path.relative_to(root))
            sites.extend(scan_file(texts[path], rel, helpers, crates.get(path, "?")))
        except (OSError, ValueError) as exc:
            print(f"cannot scan {path}: {exc}", file=sys.stderr)
            return 2

    problems: list[str] = []
    bans = clippy_bans(root)
    if bans:
        missing = [b for b in CLIPPY_SPAWN_BANS if b not in bans]
        if missing:
            problems.append(
                "clippy.toml: no longer bans " + ", ".join(missing)
                + "; this gate argues that the ban reaches `.spawn()` only, and that argument "
                  "has to be rewritten together with the config")

    recorded, errors = read_allowlist(allowlist)
    problems.extend(errors)

    seen: set[tuple[str, str]] = set()
    findings: list[Site] = []
    for site in sorted(sites, key=lambda s: (s.file, s.line)):
        if not site.category:
            continue
        key = (site.key(), site.category)
        seen.add(key)
        if key in recorded:
            continue
        findings.append(site)
        problems.append(
            f"{site.at()}: {site.category}: {site.key()} hands `{site.future}` to a timeout, "
            "which drops the future while the child it spawned is still running"
            + (f" ({site.detail})" if site.detail else "")
            + f"; mark the command kill_on_drop(true) or record it in {allowlist.name}")

    for (key, category), reason in sorted(recorded.items()):
        if (key, category) not in seen:
            problems.append(
                f"{allowlist.name}: recorded {category} for {key} ({reason}), but nothing here "
                "matches that row any more; delete it or fix the code it excuses")

    covered = sum(1 for s in sites if not s.category and s.kills and not s.std_command)
    std_sites = sum(1 for s in sites if s.std_command)

    if as_json:
        print(json.dumps({
            "sites": len(sites),
            "killed_on_drop": covered,
            "std_commands": std_sites,
            "recorded": len(seen & set(recorded)),
            "findings": [{"at": s.at(), "key": s.key(), "category": s.category,
                          "future": s.future, "detail": s.detail} for s in findings],
        }, indent=2))
    elif not quiet:
        if show:
            for site in sorted(sites, key=lambda s: (s.file, s.line)):
                flag = "killed" if not site.category and site.kills else (site.category or "plain")
                print(f"{site.at():70s} {flag:20s} {site.future[:64]}")
        for line in problems:
            print(line)

    if problems:
        print(f"FAIL timeout children: {len(findings)} unrecorded site(s), "
              f"{len(problems) - len(findings)} ledger/config problem(s)", file=sys.stderr)
        return 1
    if not quiet and not as_json:  # `--json` leaves stdout to the JSON alone
        print(f"timeout children hold: {len(sites)} timeout-abandoned output/status site(s), "
              f"{covered} kill their child on drop, {std_sites} are std commands, "
              f"{len(seen & set(recorded))} recorded")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="check timeout-abandoned children are killed")
    parser.add_argument("--root", default=".", type=Path)
    parser.add_argument("--allowlist", default=None, type=Path)
    parser.add_argument("--print", action="store_true", dest="show")
    parser.add_argument("--quiet", action="store_true")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    allowlist = args.allowlist or Path(__file__).resolve().parent / "timeout-child-allowlist.tsv"
    return check(root, allowlist, args.show, args.quiet, args.json)


if __name__ == "__main__":
    sys.exit(main())
