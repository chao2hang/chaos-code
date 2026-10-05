#!/usr/bin/env python3
"""Read a Rust `cfg(...)` predicate and fold it, the way rustc's cfg matcher does.

Two guards need this and have to agree:

- `scripts/ci/check-dead-cfg.py` refuses a predicate that can never hold, because rustc drops the
  item behind one before name resolution and nothing warns about it -- 23 tests in
  `crates/codegen/xai-grok-shell/src/agent/auth_method.rs` hid behind `#[cfg(any())]` for ten
  weeks, compiled by no build, absent from every ledger, and still reading as covered code;
- `scripts/ci/panic-site-census.py` decides whether a `.unwrap()` is in the released binary. It
  already asked "does this predicate require `test`", and `any()` answered no, so the 7
  `.unwrap()`s in that same dropped module were counted as production panic sites -- the ledger
  that the whole audit line is built on, wrong by a number nobody could see.

One question per function, so the two guards cannot drift apart on the same text:

- `fold(args)` -> TRUE / FALSE / UNKNOWN. Three-valued, because the guard cannot know the feature
  tree, the target or whether `test` is on: every leaf is UNKNOWN, `all` falls to its FALSE part,
  `any` rises to its TRUE part, `not` flips, `any()` with no parts is FALSE and `all()` with no
  parts is TRUE. A conjunction that carries a leaf both with and without `not()` is FALSE
  (`x && !x`), and the same pair inside an `any` is TRUE. A leaf keeps its value, because
  `target_os = "linux"` and `not(target_os = "linux")` are one leaf while `target_os = "linux"`
  and `target_os = "macos"` are two.
- `never_holds(args)` -> whether `fold` came back FALSE.
- `requires_test(args)` -> whether the predicate can only be satisfied by a test build. Different
  question, different lattice: `cfg(not(test))` requires nothing (it is the code that ships when
  tests are off), and `any(target_os = "linux", all(unix, test))` requires nothing because one
  branch alone builds the item. This is the census's original rule, kept verbatim here.

`fold` raises `ValueError` on text it cannot parse. Skipping unreadable predicates is the same
going-blind the guards exist to close, so the callers report the error instead.

Each caller keeps its own comment/literal blanker, because they need different things: the census
blanks string literals away entirely (a `.unwrap()` written inside a message is not a call, and an
unbalanced brace in a message would derail its module walk), while this module's caller needs the
literal text of `feature = "..."` to survive.
"""

from __future__ import annotations

import re

TRUE, FALSE, UNKNOWN = "true", "false", "unknown"

# Every place a cfg predicate is written: `#[cfg(..)]`, `#![cfg(..)]`, `#[cfg_attr(..)]` and the
# `cfg!(..)` macro. Both alternatives demand the opening parenthesis, which is what keeps a
# function *named* `cfg` out of the results -- `fn cfg() -> Config` and its `&cfg()` call sites in
# `crates/common/xai-grok-compaction/src/code_compaction/compact.rs` matched a looser pattern 10
# times, and each match became a bogus finding.
CFG_USE = re.compile(r"#\s*!?\s*\[\s*(cfg_attr|cfg)\s*\(|\bcfg!\s*\(")
# One token of a predicate: a condition name, a quoted value, or punctuation.
TOKEN = re.compile(r"[A-Za-z_][A-Za-z0-9_]*|\"[^\"]*\"|[(),=]")
# The census's own reading of a predicate: names, the halves of `key = "value"`, and the
# punctuation that nests them. Strings are already blanked to spaces by the time an argument list
# reaches this, so a missing value has to parse as one.
CFG_TOKEN = re.compile(r"[A-Za-z_][A-Za-z0-9_]*|\(|\)|,|=")


class Fold:
    """One predicate's folded value plus the undecided leaves of a conjunction.

    The leaf sets are only carried through `all(..)`, which is where a leaf and its own negation
    sitting side by side proves the conjunction false. `any(..)` drops them: a leaf in one branch
    says nothing about the branch that would have to hold for the whole `any` to be false.
    """

    __slots__ = ("state", "pos", "neg")

    def __init__(
        self,
        state: str,
        pos: frozenset[str] = frozenset(),
        neg: frozenset[str] = frozenset(),
    ) -> None:
        self.state = state
        self.pos = pos
        self.neg = neg


def parse_predicate(tokens: list[str]) -> Fold:
    """Fold a tokenised predicate. Raises ValueError on anything malformed."""

    def pred(index: int, joiner: str) -> tuple[Fold, int]:
        """The comma-separated parts from `index`, closed by `)`, joined as `joiner`.

        The caller hands over the text *inside* the outer parentheses, so running out of tokens is
        the end of the list; only a stray token left over afterwards is malformed.
        """
        parts: list[Fold] = []
        while True:
            if index >= len(tokens):
                return (combine_all(parts) if joiner == "all" else combine_any(parts)), index
            if tokens[index] == ")":
                return (combine_all(parts) if joiner == "all" else combine_any(parts)), index + 1
            if tokens[index] == ",":
                index += 1
                continue
            part, index = condition(index)
            parts.append(part)

    def condition(index: int) -> tuple[Fold, int]:
        name = tokens[index]
        if name in ("all", "any", "not") and index + 1 < len(tokens) and tokens[index + 1] == "(":
            inner, after = pred(index + 2, "any" if name == "any" else "all")
            if name != "not":
                return inner, after
            flip = {TRUE: FALSE, FALSE: TRUE, UNKNOWN: UNKNOWN}
            return Fold(flip[inner.state], inner.neg, inner.pos), after
        if name in ("(", ")", ",", "="):
            raise ValueError(f"unexpected {name!r}")
        if index + 1 < len(tokens) and tokens[index + 1] == "=":
            if index + 2 >= len(tokens):
                raise ValueError("key = with no value")
            return Fold(UNKNOWN, frozenset({f"{name}={tokens[index + 2]}"})), index + 3
        return Fold(UNKNOWN, frozenset({name})), index + 1

    def combine_all(parts: list[Fold]) -> Fold:
        if any(part.state == FALSE for part in parts):
            return Fold(FALSE)
        if not parts:
            return Fold(TRUE)
        if all(part.state == TRUE for part in parts):
            return Fold(TRUE)
        pos: set[str] = set()
        neg: set[str] = set()
        for part in parts:
            if part.state == UNKNOWN:
                pos |= part.pos
                neg |= part.neg
        if pos & neg:
            return Fold(FALSE)
        return Fold(UNKNOWN, frozenset(pos), frozenset(neg))

    def combine_any(parts: list[Fold]) -> Fold:
        if any(part.state == TRUE for part in parts):
            return Fold(TRUE)
        if not parts:
            return Fold(FALSE)
        if all(part.state == FALSE for part in parts):
            return Fold(FALSE)
        pos: set[str] = set()
        neg: set[str] = set()
        for part in parts:
            if part.state == UNKNOWN:
                pos |= part.pos
                neg |= part.neg
        if pos & neg:
            # `x || !x` holds whatever `x` names, so the branch is compiled after all.
            return Fold(TRUE)
        return Fold(UNKNOWN)

    result, consumed = pred(0, "all")
    if consumed < len(tokens):
        raise ValueError("trailing tokens after the predicate")
    return result


def fold(args: str) -> Fold:
    """Fold the text of a `cfg(...)` argument list."""
    tokens = TOKEN.findall(args)
    if not tokens:
        return Fold(TRUE)
    return parse_predicate(tokens)


def never_holds(args: str) -> bool:
    """Whether rustc drops whatever this predicate gates, in every build, for every target."""
    return fold(args).state == FALSE


def requires_test(args: str) -> bool:
    """Does a `cfg(...)` argument list apply only to a test build?

    The question is whether `test` is *required*, not whether the word appears. `all` requires
    whatever any of its parts requires, because rustc builds the item only when every part holds;
    `any` requires only whatever every part requires, because one branch alone is enough to build
    it. So `all(unix, test)` is a test gate and `any(target_os = "linux", all(unix, test))` is not,
    and a file carrying the second one is in the Linux release build with its panics attached.
    `not(...)` requires nothing, so `cfg(not(test))` marks code that ships precisely when tests
    are off.
    """
    tokens = CFG_TOKEN.findall(args)

    def condition(index: int) -> tuple[bool, int]:
        """Whether one condition requires `test`, and where the tokens go on."""
        name = tokens[index]
        if index + 1 < len(tokens) and tokens[index + 1] == "=":
            return False, index + 2
        if name in ("all", "any", "not") and index + 1 < len(tokens) and tokens[index + 1] == "(":
            parts, after = branch(index + 2)
            if name == "all":
                return any(parts), after
            if name == "any":
                return bool(parts) and all(parts), after
            return False, after
        return name == "test", index + 1

    def branch(index: int) -> tuple[list[bool], int]:
        """The requirement of each comma-separated part, and where the list ends."""
        parts: list[bool] = []
        while index < len(tokens):
            if tokens[index] == ")":
                return parts, index + 1
            if tokens[index] == ",":
                index += 1
                continue
            part, index = condition(index)
            parts.append(part)
        return parts, index

    top: list[bool] = []
    index = 0
    while index < len(tokens):
        if tokens[index] in (",", ")"):
            index += 1
            continue
        part, index = condition(index)
        top.append(part)
    return bool(top) and all(top)


def predicate_span(clean: str, open_at: int) -> tuple[str, int] | None:
    """The text inside a `cfg(`-style parenthesis, and the index just past its `)`.

    `clean` is expected to be blanked text, so an unbalanced bracket cannot come from a literal.
    Returns `None` when the parenthesis never closes, which the caller reports rather than skips.
    """
    depth = 0
    index = clean.find("(", open_at)
    if index < 0:
        return None
    start = index + 1
    while index < len(clean):
        char = clean[index]
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0:
                return clean[start:index], index + 1
        index += 1
    return None


def attribute_end(clean: str, open_at: int) -> int:
    """The index just past the `]` closing the attribute that starts at `open_at`.

    Falls back to the end of the text when there is no `]`, which leaves the caller's item walk
    with no span to report -- an attribute that never closes is not valid Rust anyway.
    """
    read = predicate_span(clean, open_at)
    if read is None:
        return len(clean)
    closing = clean.find("]", read[1])
    return len(clean) if closing < 0 else closing + 1


def up_to_top_level_comma(args: str) -> str:
    """The text of a `cfg_attr` predicate: everything before its first top-level comma.

    `cfg_attr(cond, attrs)` puts a condition first and the attributes that condition would apply
    after it; only the condition is a predicate.
    """
    depth = 0
    for index, char in enumerate(args):
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
        elif char == "," and depth == 0:
            return args[:index]
    return args
