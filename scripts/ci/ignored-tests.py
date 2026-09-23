#!/usr/bin/env python3
"""Report Rust #[ignore] attributes as correctly escaped CSV."""
import argparse
import csv
import io
import re
import sys
from pathlib import Path

IGNORE = re.compile(r"#\[\s*ignore\b")
FN = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)")

def strip_comments(source):
    out = list(source)
    i = 0
    while i < len(source):
        if source.startswith('//', i):
            end = source.find('\n', i)
            if end < 0:
                end = len(source)
            for j in range(i, end):
                out[j] = ' '
            i = end
        elif source.startswith('/*', i):
            depth, j = 1, i + 2
            while j < len(source) and depth:
                if source.startswith('/*', j):
                    depth += 1; j += 2
                elif source.startswith('*/', j):
                    depth -= 1; j += 2
                else:
                    j += 1
            for k in range(i, j):
                if source[k] != '\n':
                    out[k] = ' '
            i = j
        elif source[i] == '"':
            i += 1
            while i < len(source):
                if source[i] == '\\':
                    i += 2
                elif source[i] == '"':
                    i += 1; break
                else:
                    i += 1
        else:
            i += 1
    return ''.join(out)

def attr_end(clean, start):
    depth, quoted, escaped = 0, False, False
    for i in range(start, len(clean)):
        c = clean[i]
        if quoted:
            if escaped: escaped = False
            elif c == '\\': escaped = True
            elif c == '"': quoted = False
        elif c == '"': quoted = True
        elif c == '[': depth += 1
        elif c == ']':
            depth -= 1
            if depth == 0:
                return i + 1
    return len(clean)

def rust_string_value(value):
    out = []
    i = 0
    escapes = {'n': '\n', 'r': '\r', 't': '\t', '0': '\0', '\\': '\\', '"': '"', "'": "'"}
    while i < len(value):
        if value[i] != '\\':
            out.append(value[i]); i += 1; continue
        i += 1
        if i == len(value):
            out.append('\\'); break
        code = value[i]
        if code in escapes:
            out.append(escapes[code]); i += 1
        elif code == 'x' and i + 2 < len(value):
            out.append(chr(int(value[i + 1:i + 3], 16))); i += 3
        elif code == 'u' and i + 1 < len(value) and value[i + 1] == '{':
            end = value.find('}', i + 2)
            if end < 0: out.append('\\\\u'); i += 1
            else:
                digits = value[i + 2:end].replace('_', '')
                out.append(chr(int(digits, 16))); i = end + 1
        else:
            out.extend(('\\', code)); i += 1
    return ''.join(out)

def package_name(path):
    for parent in (path.parent, *path.parents):
        cargo = parent / 'Cargo.toml'
        if cargo.is_file():
            match = re.search(r'^name\s*=\s*"([^"]+)"', cargo.read_text(errors='replace'), re.M)
            if match:
                return match.group(1)
    return 'unknown'

def scan(path):
    source = path.read_text(encoding='utf-8')
    clean = strip_comments(source)
    rows = []
    for match in IGNORE.finditer(clean):
        if match.start() > 0 and clean[match.start() - 1] in ('"', "'", '#'):
            continue
        end = attr_end(clean, match.start())
        attr = clean[match.start():end]
        reason_match = re.search(r'\bignore\s*=\s*"((?:\\.|[^"\\])*)"', attr, re.S)
        reason = rust_string_value(reason_match.group(1)) if reason_match else 'NO_REASON'
        following = clean[end:]
        fn_match = FN.search(following)
        line = source.count('\n', 0, match.start()) + 1
        rows.append([package_name(path), path.as_posix(), line, reason,
                     fn_match.group(1) if fn_match else ''])
    return rows

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--csv', action='store_true')
    parser.add_argument('--stale', action='store_true')
    parser.add_argument('--root', default='crates')
    args = parser.parse_args()
    root = Path(args.root)
    rows = [row for path in sorted(root.rglob('*.rs')) for row in scan(path)] if root.exists() else []
    if args.stale:
        rows = [row for row in rows if not re.search(r'20\d{2}-\d{2}', row[3])]
    if args.csv:
        writer = csv.writer(sys.stdout, lineterminator='\n')
        writer.writerow(['crate', 'file', 'line', 'reason', 'nearest_fn'])
        writer.writerows(rows)
    else:
        print(f'Total ignored attributes: {len(rows)}')
        for row in rows:
            print(f'{row[0]}:{row[1]}:{row[2]}: {row[4]}: {row[3]}')

if __name__ == '__main__':
    main()
