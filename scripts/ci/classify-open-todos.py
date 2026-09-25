#!/usr/bin/env python3
"""Print every open/partial TODO line grouped by nearest Markdown heading."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
path = ROOT / 'TODO.md'
section = '(document preamble)'
open_count = partial_count = 0
for number, line in enumerate(path.read_text(encoding='utf-8').splitlines(), 1):
    if line.startswith('#'):
        section = line
    if line.startswith('- [ ]'):
        open_count += 1
        print(f'{number}\tunchecked\t{section}\t{line}')
    elif line.startswith('- [~]'):
        partial_count += 1
        print(f'{number}\tpartial\t{section}\t{line}')
print(f'TOTAL\tunchecked={open_count}\tpartial={partial_count}\trows={open_count + partial_count}')
