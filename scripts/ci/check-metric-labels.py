#!/usr/bin/env python3
"""Fail when a Prometheus label-values call cannot match the metric it names.

`MetricVec::with_label_values` is documented as the neat form and is implemented as an
unwrap of the checked one. In `prometheus-0.14.0` the function is at `src/vec.rs:292` and the
unwrap that panics is at `src/vec.rs:296`:

    pub fn with_label_values<V>(&self, vals: &[V]) -> T::M {
        self.get_metric_with_label_values(vals).unwrap()
    }

The checked path counts what it was given, inside `hash_label_values` (`src/vec.rs:118`):

    if vals.len() != self.desc.variable_labels.len() {
        return Err(Error::InconsistentCardinality { expect: ..., got: ... });
    }

So one extra or one missing entry in the array is a process abort at the call site. Nothing
in this repository sees it:

* It carries no `unwrap`, `expect`, `panic!` or `unsafe` token, so `panic-site-census.py`,
  which counts panic-capable sites by the tokens that cause them, cannot count it. The panic
  lives inside a dependency.
* It is invisible to the compiler because the labels are a runtime slice: `&[&str]` of any
  length typechecks against `&[V] where V: AsRef<str>`.
* A test catches it only where a test reaches that call with the same array. 101 of the 155
  label-value call sites here are in production code, on startup, drain, recovery, swap and OOM
  paths, and which of them a test reaches is a coverage question this gate deliberately does not
  take on: it judges all 155 without compiling or running anything.
* `register_*_vec!` returns `Err` for a duplicate metric name and for a name or label that
  Prometheus rejects, and all 82 macro registrations consume that `Result` inside a `LazyLock`
  (36 by `unwrap`, 46 by `expect`), so the abort lands in whatever code first touches the
  metric, not in the module that registered it. Dropping the `Result` instead is already a
  clippy error, because CI runs `cargo clippy --workspace --all-targets -- -D warnings` and
  `Result` is `#[must_use]`; the case no compiler covers is the aborting one.

The check, on every `*.rs` file in the repository:

1. Registration side. For each `register_*!` / `register_*_vec!` / `register_*_with_registry!`
   macro of the `prometheus` crate: the metric name is a valid Prometheus metric name and is
   registered exactly once across the tree; label names are valid and are not repeated inside
   one metric. This is what makes the `unwrap`/`expect` on the 82 macro registrations
   unreachable, and the gate, not the comment next to it, is the reason.
2. Call side. For each `with_label_values`, `get_metric_with_label_values`,
   `remove_label_values` and `delete_label_values` call whose argument is an array literal (or
   a local binding of one), the number of values has to equal the number of labels of the
   metric it names. `with_label_values` and `with` unwrap; the other two return the error, so
   a mismatch there is a metric that silently never reports. Both are reported.

Resolution is by name: the receiver's last path segment is matched against the identifiers
that registrations are bound to (`static NAME`, `const NAME`, `let NAME`). That is exact for
the way metrics are used here, which is a `static` behind a `LazyLock`, and it is said
plainly rather than dressed up as type checking: two metrics in different modules sharing one
identifier are reported as ambiguous instead of guessed at.

Not covered, stated rather than pretended:

* `MetricVec::with(&HashMap<..>)`, the other unwrapping form. The method name is shared with
  `Cell::with`, `RefCell::with` and this repository's own lock helpers: 244 lines in this tree
  call `.with(`, and judging any one of them needs the receiver's type. Two of the 244 sit in a
  file that also declares metric vectors, and both are `tracing_subscriber::registry().with(
  layer)`, so no metric here is looked up that way today; a future one would not be seen.
* Label values that are not an array literal and not a local array binding, for instance a
  `&[&str]` threaded through a function. Those are reported as `dynamic-labels` so they are
  reviewed and recorded, never assumed fine.
* A receiver obtained through a field or a function call (`self.metrics.with_label_values`),
  reported as `unresolved-receiver` for the same reason.
* Global uniqueness for a family built by hand, `IntCounterVec::new(Opts::new(..), &[..])`,
  or registered through `register_*_with_registry!`. Their names, labels and call arity are
  checked, but two families of the same name are only a defect if they reach the same
  `Registry`, and which registry that is comes from an argument this gate does not follow.
  Every hand-built family in this tree is a test with a `Registry::new()` of its own.
* A metric name assembled at runtime, reported as `unreadable-name` so it is reviewed.
* The *values* of labels: a label whose value differs only in spelling (`Ok` against `ok`) is
  a query bug, not an abort, and no static rule can tell which spelling is the intended one.

Usage:
    check-metric-labels.py [--root DIR] [--allowlist FILE] [--print] [--quiet]
Exit: 0 = every call matches its metric, 1 = problems, 2 = tree or allowlist unreadable.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

# Method names that take a label-values slice. The first two unwrap the Result inside the
# dependency, so a mismatch aborts; the last two hand it back.
LABEL_VALUE_METHODS = {
    "with_label_values": "aborts on a mismatch (prometheus unwraps inside the dependency)",
    "get_metric_with_label_values": "returns Err on a mismatch, so the metric silently stops reporting",
    "remove_label_values": "returns Err on a mismatch, so the child is never removed",
    "delete_label_values": "returns Err on a mismatch, so the child is never removed",
}

# `register_*` macros exported by prometheus 0.14. Kept as an explicit list so that a macro
# this repository defines itself, `register_resource!` for one, is not mistaken for one of
# these; the first string argument of that macro is a tool name, not a metric name.
PROMETHEUS_MACROS = {
    "register_counter", "register_counter_vec",
    "register_counter_with_registry", "register_counter_vec_with_registry",
    "register_gauge", "register_gauge_vec",
    "register_gauge_with_registry", "register_gauge_vec_with_registry",
    "register_histogram", "register_histogram_vec",
    "register_histogram_with_registry", "register_histogram_vec_with_registry",
    "register_int_counter", "register_int_counter_vec",
    "register_int_counter_with_registry", "register_int_counter_vec_with_registry",
    "register_int_gauge", "register_int_gauge_vec",
    "register_int_gauge_with_registry", "register_int_gauge_vec_with_registry",
}

# Metric families built by hand instead of through the macros, which is what a test does when
# it wants a registry of its own: `IntCounterVec::new(Opts::new(name, help), &["reason"])`.
# Covered for name validity, label validity and call arity; see the note on `via` below for
# what is not claimed about them.
CONSTRUCTOR_VEC = re.compile(
    r"\b(?:Int(?:Counter|Gauge)Vec|CounterVec|GaugeVec|HistogramVec|SummaryVec)"
    r"::(?:new|with_opts)\s*\("
)

MACRO_START = re.compile(
    r"\b(" + "|".join(sorted(PROMETHEUS_MACROS, key=len, reverse=True)) + r")!\s*\("
)
CALL = re.compile(
    r"(?<![.\w])([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)"
    r"\s*\.\s*(" + "|".join(sorted(LABEL_VALUE_METHODS, key=len, reverse=True)) + r")\s*\(\s*&?"
)
# A registration is bound to the nearest preceding `static`/`const`/`let` with no `;` in
# between, which is how `static NAME: LazyLock<IntCounterVec> = LazyLock::new(|| register…!())`
# reads, `pub(crate)` and `pub` prefixes included.
BINDING = re.compile(
    r"\b(?:static|const)\s+(?:pub(?:\([^)]*\))?\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:|"
    r"\blet\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*(?::[^=;]*)?="
)
# A local `let labels = [a, b];` that a call then passes as `&labels`.
LET_ARRAY = re.compile(
    r"\blet\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*(?::[^=;]*)?=\s*(?:&)?\["
)

METRIC_NAME = re.compile(r"^[a-zA-Z_:][a-zA-Z0-9_:]*$")
LABEL_NAME = re.compile(r"^[a-zA-Z_][a-zA-Z0-9_]*$")

CATEGORIES = (
    "dynamic-labels",       # the values are computed, so no static count exists
    "unresolved-receiver",  # the receiver is not a name a registration is bound to
    "ambiguous-receiver",   # the name is bound to metrics with different label counts
    "not-prometheus",       # the method belongs to another type with the same name
)

REASON_MIN = 20
SKIP_DIRS = {"target", ".git", "node_modules", "dist", "build", ".cargo"}


def blank_comments(text: str) -> str:
    """Blank comments, keep string literals.

    Comments have to go, or a commented-out call is reported as a live one. Strings have to
    stay, because the metric name and the label names being checked are string literals.
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


def balanced(text: str, open_idx: int) -> tuple[str, int]:
    """Return the text inside the bracket pair opening at `open_idx`, and the close index.

    Bracket-generic and string-aware. Both matter here: the label list is `[...]` while a
    macro's arguments are `(...)`, and a help string such as
    `"drain deadline exceeded (expected 0)"` contains a parenthesis pair that belongs to prose,
    while one such as `"buckets, upper bound )"` would otherwise close the argument list early
    and make every later argument unreadable.
    """
    depth, i = 0, open_idx
    while i < len(text):
        c = text[i]
        if c == "\"":
            j = i + 1
            while j < len(text):
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == "\"":
                    j += 1
                    break
                j += 1
            i = j
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
    """Split a macro argument list on top-level commas, keeping nested brackets intact."""
    parts: list[str] = []
    depth, cur = 0, []
    i, n = 0, len(args)
    while i < n:
        c = args[i]
        if c == "\"":
            j = i + 1
            while j < n:
                if args[j] == "\\":
                    j += 2
                    continue
                if args[j] == "\"":
                    j += 1
                    break
                j += 1
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


def count_elements(text: str, open_idx: int) -> tuple[int, int]:
    """Count the comma-separated entries of the `[...]` opening at `open_idx`.

    A trailing comma is ordinary Rust formatting and not an extra element: most label arrays
    in `handle_tests.rs` are written one entry per line with a trailing comma, so a counter
    that counts commas would read `&[A, B,]` as three values and report every one of them as
    an abort. The first draft of this scanner did exactly that, and `--print` on the real tree
    counted its damage: 70 problems where the fixed counter finds none.
    """
    i = open_idx
    parts: list[str] = [""]
    depth = 0
    while i < len(text):
        c = text[i]
        if c in "[{(":
            depth += 1
        elif c in "]})":
            depth -= 1
            if depth == 0:
                break
        if c == "\"" and depth >= 1:
            j = i + 1
            while j < len(text):
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == "\"":
                    j += 1
                    break
                j += 1
            parts[-1] += text[i:j]
            i = j
            continue
        if c == "," and depth == 1:
            parts.append("")
        elif depth >= 1:
            parts[-1] += c
        i += 1
    entries = [p.strip() for p in parts]
    if entries and entries[-1] == "":
        entries.pop()
    return len(entries), i


def string_literal(arg: str) -> str | None:
    m = re.match(r'^\s*(?:r#*)?"((?:[^"\\]|\\.)*)"', arg)
    return m.group(1) if m else None


def label_names(args: str, parts: list[str]) -> list[str] | None:
    """The label list of a `*_vec` registration: the first `&[...]` argument.

    Picking it out of the split arguments rather than searching the whole argument list is
    what keeps a help string that happens to contain `&[` from being read as labels. A
    `register_histogram_vec!` also carries a `vec![...]` of buckets after the labels, which
    is why this takes the first such argument and not the last bracket in the list.
    """
    for arg in parts:
        if not arg.startswith("&"):
            continue
        m = re.match(r"&\s*\[", arg)
        if not m:
            continue
        _, close = balanced(arg, arg.index("["))
        return [string_literal(p.strip()) or "?"
                for p in split_args(arg[m.end():close]) if p.strip()]
    return None


class Registration:
    __slots__ = ("metric", "labels", "file", "line", "ident", "is_vec", "via")

    def __init__(self, metric, labels, file, line, ident, is_vec, via):
        self.metric = metric
        self.labels = labels
        self.file = file
        self.line = line
        self.ident = ident
        self.is_vec = is_vec
        # "macro" for register_*! against the default registry, "scoped" for the
        # *_with_registry variants and "constructor" for a hand-built family: the first goes
        # to the one process-wide registry, the other two go wherever the code says, so only
        # the first is claimed to be globally unique.
        self.via = via

    def key(self) -> str:
        return f"{self.file}:{self.line}"


def bound_identifier(text: str, macro_start: int) -> str | None:
    """The identifier this registration is stored in, or None if it is stored in neither."""
    best = None
    for m in BINDING.finditer(text, 0, macro_start):
        tail = text[m.end():macro_start]
        if ";" in tail:
            continue
        best = m.group(1) or m.group(2)
    return best


def family_name(parts: list[str]) -> str:
    """The metric name a registration declares: the first argument, one level down if needed.

    Only the first argument is consulted. Reading further would turn the help text of a
    registration whose name is built at runtime into the metric name, which is a wrong answer
    reported as a right one.
    """
    if not parts:
        return "?"
    first = parts[0]
    lit = string_literal(first)
    if lit is not None:
        return lit
    inner = re.search(r'\b\w+(?:::\w+)*::(?:new|with_opts)\s*\(\s*"((?:[^"\\]|\\.)*)"', first)
    return inner.group(1) if inner else "?"


def scan_file(path: Path, rel: str) -> tuple[list[Registration], list[dict], str]:
    raw = path.read_text(encoding="utf-8", errors="replace")
    text = blank_comments(raw)
    regs: list[Registration] = []
    calls: list[dict] = []

    def add(start: int, end: int, macro: str, via: str) -> None:
        args, _ = balanced(text, end - 1)
        parts = split_args(args)
        is_vec = "_vec" in macro or via == "constructor"
        if is_vec:
            # `?` is the marker for "a vec whose label list this gate could not read", which
            # is reported below rather than counted as zero labels.
            labels = label_names(args, parts)
            if labels is None:
                labels = ["?"]
        else:
            labels = []
        regs.append(Registration(
            family_name(parts), labels, rel, text[:start].count("\n") + 1,
            bound_identifier(text, start), is_vec, via,
        ))

    for m in MACRO_START.finditer(text):
        macro = m.group(1)
        via = "scoped" if macro.endswith("_with_registry") else "macro"
        add(m.start(), m.end(), macro, via)

    for m in CONSTRUCTOR_VEC.finditer(text):
        add(m.start(), m.end(), "constructor_vec", "constructor")

    let_arrays: dict[str, list[tuple[int, int]]] = {}
    for m in LET_ARRAY.finditer(text):
        # `count_elements` takes the index of the opening bracket, and this regex ends on it.
        n, _ = count_elements(text, m.end() - 1)
        let_arrays.setdefault(m.group(1), []).append((m.start(), n))

    for m in CALL.finditer(text):
        receiver, method = m.group(1), m.group(2)
        line = text[:m.start()].count("\n") + 1
        rest = text[m.end():]
        head = rest.lstrip()
        offset = m.end() + (len(rest) - len(head))
        count: int | None = None
        shape = "dynamic"
        if head.startswith("[") or head.startswith("vec!["):
            idx = head.index("[")
            count, _ = count_elements(text, offset + idx)
            shape = "array"
        else:
            name = re.match(r"([A-Za-z_][A-Za-z0-9_]*)", head)
            if name:
                for pos, n in let_arrays.get(name.group(1), ()):
                    if pos < m.start():
                        count, shape = n, "local-array"
        calls.append({
            "receiver": receiver,
            "name": receiver.split("::")[-1],
            "method": method,
            "file": rel,
            "line": line,
            "count": count,
            "shape": shape,
        })
    return regs, calls, text


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


def check(root: Path, allowlist: Path, show: bool, quiet: bool) -> int:
    files = rust_files(root)
    if not files:
        print(f"no Rust sources under {root}", file=sys.stderr)
        return 2

    regs: list[Registration] = []
    calls: list[dict] = []
    for path in files:
        try:
            r, c, _ = scan_file(path, str(path.relative_to(root)))
        except OSError as exc:
            print(f"cannot read {path}: {exc}", file=sys.stderr)
            return 2
        regs.extend(r)
        calls.extend(c)

    by_name: dict[str, list[Registration]] = {}
    for reg in regs:
        if reg.metric != "?":
            by_name.setdefault(reg.metric, []).append(reg)

    # What identifier each metric is reachable under, and the label counts that identifier
    # can mean. An identifier used for two metrics with different label counts is not
    # resolved by guessing the type.
    ident_labels: dict[str, set[int]] = {}
    for reg in regs:
        if reg.ident and "?" not in reg.labels:
            ident_labels.setdefault(reg.ident, set()).add(len(reg.labels))

    problems: list[str] = []
    findings: dict[tuple[str, str], str] = {}

    for reg in sorted(regs, key=lambda r: (r.file, r.line)):
        if reg.metric == "?":
            problems.append(
                f"{reg.key()}: unreadable-name: the metric name of this registration is not a "
                f"string literal, so neither this gate nor a reader can tell which family it "
                f"declares; write the name as a literal"
            )
        elif not METRIC_NAME.match(reg.metric):
            problems.append(
                f"{reg.key()}: bad-name: {reg.metric!r} is not a Prometheus metric name; "
                f"register_*! returns Err for it and the unwrap in this registration aborts"
            )
        seen: dict[str, int] = {}
        for pos, label in enumerate(reg.labels):
            if label == "?":
                problems.append(
                    f"{reg.key()}: unreadable-labels: {reg.metric}: the label list of this "
                    f"registration is not an array of string literals, so neither this gate "
                    f"nor a reader can tell how many label values a call has to pass"
                )
            elif not LABEL_NAME.match(label):
                problems.append(
                    f"{reg.key()}: bad-label: {reg.metric}: label {label!r} is not a valid "
                    f"Prometheus label name; register_*! returns Err for it"
                )
            if label in seen:
                problems.append(
                    f"{reg.key()}: duplicate-label: {reg.metric}: label {label!r} is declared "
                    f"twice, at positions {seen[label]} and {pos}; register_*! returns Err"
                )
            seen.setdefault(label, pos)

    for metric, group in sorted(by_name.items()):
        # Only `register_*!` without an explicit registry goes to the one process-wide
        # registry, and only those can collide with each other at runtime. A family handed to
        # a `Registry` of its own, which is what every test here does, is not a duplicate of a
        # production family of the same name, so it is not reported as one.
        group = [r for r in group if r.via == "macro"]
        if len(group) > 1:
            where = ", ".join(r.key() for r in group)
            problems.append(
                f"{group[0].key()}: duplicate-name: {metric!r} is registered {len(group)} times "
                f"({where}); the second registration returns AlreadyRegistered and its unwrap "
                f"aborts the first thread that touches that metric"
            )

    for call in sorted(calls, key=lambda c: (c["file"], c["line"])):
        where = f"{call['file']}:{call['line']}"
        note = LABEL_VALUE_METHODS[call["method"]]
        counts = ident_labels.get(call["name"])
        if call["shape"] == "dynamic":
            findings[(call["name"], "dynamic-labels")] = where
            continue
        if not counts:
            findings[(call["name"], "unresolved-receiver")] = where
            continue
        if len(counts) > 1:
            findings[(call["name"], "ambiguous-receiver")] = where
            continue
        want = next(iter(counts))
        if call["count"] != want:
            problems.append(
                f"{where}: arity: {call['receiver']}.{call['method']} passes "
                f"{call['count']} label value(s), the metric declares {want}; {note}"
            )

    rows, allowlist_errors = read_allowlist(allowlist)
    if allowlist_errors:
        for err in allowlist_errors:
            print(err, file=sys.stderr)
        return 2

    live: set[tuple[str, str]] = set()
    for key, where in sorted(findings.items()):
        if key in rows:
            live.add(key)
        else:
            kind, ident = key[1], key[0]
            problems.append(
                f"{where}: {kind}: {ident} is used with label values this gate cannot count "
                f"or resolve; record it in {allowlist.name} with category {kind!r} and a reason"
            )
    for (key, category), _ in sorted(rows.items()):
        if (key, category) not in live:
            problems.append(
                f"stale: {allowlist.name} excuses {key!r} as {category!r} but no such call is "
                f"left; delete the row"
            )

    if show:
        print(f"{len(regs)} metric registrations, {len(calls)} label-value call sites "
              f"in {len(files)} Rust files")
        for reg in sorted(regs, key=lambda r: r.metric):
            labels = ",".join(reg.labels) or "-"
            print(f"  {reg.metric:58} [{labels}] {reg.key()}")
        if findings:
            print(f"{len(findings)} call site(s) excused by {allowlist.name}:")
            for (key, category), where in sorted(findings.items()):
                print(f"  {key} {category} {where}")
        print()

    if problems:
        if not quiet:
            for p in problems:
                print(p)
        print(f"\n{len(problems)} problem(s): {len(regs)} registrations, {len(calls)} calls checked")
        return 1
    if not quiet:
        print(f"metric labels hold: {len(regs)} registrations, {len(calls)} label-value "
              f"call sites, {len(rows)} recorded")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", default=".", type=Path)
    ap.add_argument("--allowlist", type=Path,
                    default=Path(__file__).with_name("metric-labels-allowlist.tsv"))
    ap.add_argument("--print", action="store_true", dest="show",
                    help="print every metric and its labels")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()
    root = args.root.resolve()
    if not root.is_dir():
        print(f"{root}: not a directory", file=sys.stderr)
        return 2
    return check(root, args.allowlist, args.show, args.quiet)


if __name__ == "__main__":
    sys.exit(main())
