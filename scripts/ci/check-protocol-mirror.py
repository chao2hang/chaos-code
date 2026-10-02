#!/usr/bin/env python3
"""Fail when the GUI protocol mirror does not describe the protocol that ships.

`scripts/ci/check-gui-protocol.sh` compares the committed
`apps/chaos-ui/src/generated/protocol.ts` against the string constant in
`crates/codegen/chaos-engine/src/protocol_schema.rs`. Both sides are the same
hand-written text, so that check proves only that the file was regenerated. It
cannot see a message that was added to `ClientMessage`/`ServerMessage` and never
mirrored -- and six such messages existed until this guard was written
(`begin_attachment`, `attachment_chunk`, `cancel_attachment`,
`attachment_started`, `attachment_progress`, `attachment_cancelled`), all of them
spoken by the engine and covered by
`crates/codegen/chaos-engine/tests/attachment_protocol.rs`.

This guard parses the real Rust enums and compares them against the mirror in
both directions: every wire tag, and every field name under each tag.

Field *types* are still hand-mirrored and are out of scope here: `UUID` vs
`Uuid`, `number` vs `u64`, and named helper types like `WorkspaceInfo` are a
second, much larger translation problem, and a type mismatch fails at
deserialization rather than silently. Presence and names are what a browser can
get wrong without noticing.

Usage:
    check-protocol-mirror.py [--engine PATH] [--mirror PATH] [--print]
"""

from __future__ import annotations

import argparse
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEFAULT_ENGINE = os.path.join(REPO, "crates/codegen/chaos-engine/src/lib.rs")
DEFAULT_MIRROR = os.path.join(REPO, "crates/codegen/chaos-engine/src/protocol_schema.rs")

# The enums the browser talks to, and the TS union that is supposed to describe
# each one. A new enum with the same serde tagging needs its own pair here, and
# adding it is a deliberate act.
ENUMS = ("ClientMessage", "ServerMessage")


def snake_case(name: str) -> str:
    """The subset of serde's `rename_all = "snake_case"` (heck's algorithm).

    A capital starts a word; inside a run of capitals the last capital before a
    lowercase starts its own word, so `HTTPStatus` is `http_status` and
    `TuiSessionImport` is `tui_session_import`.
    """
    chars = list(name)
    words: list[list[str]] = []
    for index, ch in enumerate(chars):
        if ch.isupper():
            starts_word = index == 0 or not chars[index - 1].isupper()
            if not starts_word and index + 1 < len(chars) and chars[index + 1].islower():
                starts_word = True
            if starts_word:
                words.append([ch.lower()])
            else:
                words[-1].append(ch.lower())
        elif ch == '_':
            if words:
                words[-1].append('_')
        else:
            if words:
                words[-1].append(ch)
            else:
                words.append([ch])
    return '_'.join(''.join(word) for word in words)


def strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    text = re.sub(r"^\s*//.*$", "", text, flags=re.M)
    return text


def enum_body(source: str, enum_name: str) -> str:
    """The brace-balanced body of `pub enum <enum_name>`."""
    match = re.search(r"\bpub\s+enum\s+%s\b" % re.escape(enum_name), source)
    if match is None:
        raise SystemExit(f"check-protocol-mirror: no `pub enum {enum_name}` found")
    open_at = source.index("{", match.end())
    depth = 0
    for index in range(open_at, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[open_at + 1:index]
    raise SystemExit(f"check-protocol-mirror: unterminated `pub enum {enum_name}`")


def rust_variants(body: str) -> dict[str, set[str]]:
    """Map each variant name to its set of field names.

    Variants are read at the body's own brace depth, so the fields of one
    variant are never attributed to its neighbour. Tuple and unit variants get
    an empty field set, which is what the mirror spells as no extra keys.
    """
    variants: dict[str, set[str]] = {}
    depth = 0
    current: str | None = None
    for line in body.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith(("#", "///", "//!", "/*", "*")):
            continue
        if depth == 0:
            head = re.match(r"([A-Z][A-Za-z0-9_]*)\s*[{(;,]?", stripped)
            if head:
                current = head.group(1)
                variants.setdefault(current, set())
        elif current is not None:
            # Only a declaration at the start of the line is a field name; a
            # type like `Vec<serde_json::Value>` would otherwise contribute the
            # bogus field `serde_json`.
            field = re.match(r"(?:pub\s+)?([a-z][a-z0-9_]*)\s*:", stripped)
            if field:
                variants[current].add(field.group(1))
        depth += line.count("{") - line.count("}")
        if depth <= 0:
            depth = 0
            current = None
    return variants


def ts_object_keys(line: str) -> set[str]:
    """Keys of a TS object type, looking only at its outermost brace level.

    An inline nested type (`{ inner: T }`) describes a value's shape, not a wire
    field, so its keys must not be compared against Rust field names.
    """
    keys: set[str] = set()
    depth = 0
    for match in re.finditer(r"[{}]|([A-Za-z_][A-Za-z0-9_]*)\??\s*:", line):
        token = match.group(0)
        if token == "{":
            depth += 1
        elif token == "}":
            depth -= 1
        elif depth == 1:
            keys.add(match.group(1))
    keys.discard("type")
    return keys


def mirror_unions(mirror_source: str) -> tuple[dict[str, dict[str, set[str]]], list[str]]:
    """Split the mirror into `{enum: {tag: fields}}`, plus any problems found.

    The mirror lives inside a Rust raw string, so it is read as text rather than
    compiled. A union block is `export type <Enum> =` followed by lines of the
    form `| { type: 'x'; a: T; b?: U }`. Client and server tags are kept in
    separate namespaces because they are separate TS unions, and the same tag
    may legitimately appear in both.
    """
    unions: dict[str, dict[str, set[str]]] = {}
    problems: list[str] = []
    current: str | None = None
    for number, line in enumerate(mirror_source.splitlines(), start=1):
        union = re.match(r"\s*export\s+type\s+([A-Za-z0-9_]+)\s*=\s*$", line)
        if union:
            current = union.group(1)
            unions.setdefault(current, {})
            continue
        stripped = line.strip()
        if not stripped or stripped.startswith(("//", "/*", "*")):
            continue
        if not stripped.startswith("|"):
            current = None
            continue
        tag = re.search(r"type:\s*'([a-z0-9_]+)'", line)
        if tag is None:
            continue
        if current is None:
            problems.append(
                f"line {number} describes a tagged message that is not inside a "
                "`export type <Name> =` union, so nothing refers to it"
            )
            continue
        entries = unions[current]
        if tag.group(1) in entries:
            problems.append(
                f"{current} lists '{tag.group(1)}' twice; one wire tag must have "
                "exactly one shape"
            )
            continue
        entries[tag.group(1)] = ts_object_keys(line)
    return unions, problems


def check(engine_path: str, mirror_path: str) -> list[str]:
    with open(engine_path, encoding="utf-8") as handle:
        engine = strip_comments(handle.read())
    with open(mirror_path, encoding="utf-8") as handle:
        mirror_source = handle.read()

    unions, problems = mirror_unions(mirror_source)
    for enum_name in ENUMS:
        if enum_name not in unions:
            problems.append(
                f"the mirror has no `export type {enum_name} =` union, so the "
                "browser has no type for the messages the engine speaks"
            )

    for enum_name in ENUMS:
        variants = rust_variants(enum_body(engine, enum_name))
        tagged = {snake_case(name): name for name in variants}
        if len(tagged) != len(variants):
            collapsed = sorted(
                name for name in variants
                if sum(1 for other in variants if snake_case(other) == snake_case(name)) > 1
            )
            problems.append(
                f"{enum_name}: several variants collapse to the same wire tag under "
                f"snake_case ({', '.join(collapsed)}); serde would emit the same "
                "`type` for all of them"
            )
        mirrored = unions.get(enum_name, {})
        missing = sorted(set(tagged) - set(mirrored))
        if missing:
            problems.append(
                f"{enum_name} is spoken by the engine but absent from the mirror: "
                + ", ".join(f"'{tag}' (Rust {tagged[tag]})" for tag in missing)
            )
        for tag in sorted(set(tagged) & set(mirrored)):
            rust_fields = variants[tagged[tag]]
            ts_fields = mirrored[tag]
            absent = sorted(rust_fields - ts_fields)
            extra = sorted(ts_fields - rust_fields)
            if absent:
                problems.append(
                    f"'{tag}' is missing field(s) {', '.join(absent)} in the mirror "
                    f"(Rust {enum_name}::{tagged[tag]} sends them)"
                )
            if extra:
                problems.append(
                    f"'{tag}' mirrors field(s) {', '.join(extra)} that Rust "
                    f"{enum_name}::{tagged[tag]} does not have"
                )
        for tag in sorted(set(mirrored) - set(tagged)):
            problems.append(
                f"the mirror describes '{tag}' as a {enum_name}, which no Rust "
                f"{enum_name} variant produces; a browser could send it and be "
                "refused at deserialization"
            )
    return problems


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--engine", default=DEFAULT_ENGINE)
    parser.add_argument("--mirror", default=DEFAULT_MIRROR)
    parser.add_argument("--print", action="store_true", dest="print_table")
    args = parser.parse_args()

    if args.print_table:
        with open(args.engine, encoding="utf-8") as handle:
            engine = strip_comments(handle.read())
        with open(args.mirror, encoding="utf-8") as handle:
            unions, _ = mirror_unions(handle.read())
        for enum_name in ENUMS:
            variants = rust_variants(enum_body(engine, enum_name))
            mirrored = unions.get(enum_name, {})
            covered = sum(1 for name in variants if snake_case(name) in mirrored)
            print(f"{enum_name}: {covered} of {len(variants)} variants mirrored")
        for name in sorted(set(unions) - set(ENUMS)):
            if unions[name]:
                print(f"{name}: {len(unions[name])} tagged entries outside the "
                      "protocol unions")

    problems = check(args.engine, args.mirror)
    for problem in problems:
        print(f"check-protocol-mirror: {problem}", file=sys.stderr)
    if problems:
        print(
            "check-protocol-mirror: the TypeScript mirror in\n"
            "  crates/codegen/chaos-engine/src/protocol_schema.rs\n"
            "does not describe the protocol in\n"
            "  crates/codegen/chaos-engine/src/lib.rs\n"
            "Add the entries there, then regenerate with:\n"
            "  cargo run -p chaos-engine --bin chaos-protocol-schema "
            "> apps/chaos-ui/src/generated/protocol.ts",
            file=sys.stderr,
        )
        return 1
    print("check-protocol-mirror: the mirror covers every protocol message and field")
    return 0


if __name__ == "__main__":
    sys.exit(main())
