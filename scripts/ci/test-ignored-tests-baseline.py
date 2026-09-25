#!/usr/bin/env python3
"""Verify the checked-in bare #[ignore] inventory against the live Rust tree."""
import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'scripts/ci/ignored-tests.py'
spec = importlib.util.spec_from_file_location('ignored_tests', SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

rows = [
    row
    for path in sorted((ROOT / 'crates').rglob('*.rs'))
    for row in module.scan(path)
]
baseline = (ROOT / 'scripts/ci/ignored-tests-baseline.tsv').read_text(encoding='utf-8')
added = module.new_bare_ignores(rows, baseline, ROOT / 'crates')
if added:
    for crate, path, line, _, function in added:
        print(f'{path}:{line}: new bare #[ignore] in {crate}::{function or "<unknown>"}')
    raise SystemExit(1)
print(
    f'ignored-test baseline: {len(rows)} total, '
    f'{sum(row[3] == "NO_REASON" for row in rows)} approved bare, 0 new'
)
