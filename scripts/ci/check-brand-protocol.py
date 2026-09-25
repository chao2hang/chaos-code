#!/usr/bin/env python3
"""Guard shipped CLI brand strings without policing compatibility identifiers.

This intentionally scans only current user-facing source locations. It does not
scan tests, internal comments, historical changelogs, env vars, protocol IDs, or
compatibility readers. Those have separate versioning/compatibility policies.
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
SOURCE_PATHS = (
    Path('crates/codegen/xai-grok-pager-bin/src/main.rs'),
    Path('crates/codegen/xai-grok-pager/docs'),
)
RENDERED_UI_PATHS = (
    Path('apps/chaos-ui/src/main.tsx'),
    Path('apps/chaos-ui/index.html'),
)
SOURCES = tuple(ROOT / path for path in SOURCE_PATHS)
RENDERED_UI = tuple(ROOT / path for path in RENDERED_UI_PATHS)
OBSOLETE_PRODUCT_LABEL = re.compile(r'\bGrok[\s_-]*Build\b', re.IGNORECASE)
OLD_COMMAND = re.compile(
    r'(?<![A-Za-z0-9_./-])grok(?:\.exe)?\s+'
    r'(?:mcp|plugin|wrap|trace|update|setup|doctor|leader|workspace|worktree|export|'
    r'completions|agent|sessions|login|logout|du|import|model|voice|sandbox|hooks|memory|inspect)'
    r'(?=$|\s|[`\[({"])'
)


def candidates():
    for source in SOURCES:
        if source.is_file():
            yield source
        elif source.is_dir():
            yield from sorted(source.rglob('*.md'))


def main() -> int:
    required = (*SOURCES, *RENDERED_UI)
    missing = [source for source in required if not source.exists()]
    if missing:
        print('brand guard: required source paths are missing: ' + ', '.join(map(str, missing)), file=sys.stderr)
        return 2
    failures = []
    for path in RENDERED_UI:
        for number, line in enumerate(path.read_text(encoding='utf-8').splitlines(), 1):
            if OBSOLETE_PRODUCT_LABEL.search(line):
                failures.append(f'{path.relative_to(ROOT)}:{number}: obsolete product label in rendered UI text')
    for path in candidates():
        text = path.read_text(encoding='utf-8')
        for number, line in enumerate(text.splitlines(), 1):
            # Rust line comments and doc comments are implementation notes, not
            # strings presented by the shipped CLI.
            if path.suffix == '.rs' and line.lstrip().startswith('//'):
                continue
            for match in OLD_COMMAND.finditer(line):
                left = line[:match.start()]
                if path.suffix == '.rs' and ('tracing::' in left or 'vec![' in left):
                    continue
                failures.append(f'{path.relative_to(ROOT)}:{number}: obsolete CLI brand in shipped text')
    if failures:
        print('\n'.join(failures), file=sys.stderr)
        print('Use the shipped `chaos` command name; preserve protocol/env/compatibility identifiers.', file=sys.stderr)
        return 1
    print('brand/protocol guard: no obsolete upstream CLI command names in selected shipped text')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
