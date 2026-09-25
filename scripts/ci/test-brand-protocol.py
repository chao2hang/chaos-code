#!/usr/bin/env python3
"""Exercise the scoped user-visible CLI brand guard with clean/violating trees."""
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / 'scripts/ci/check-brand-protocol.py'


def run_with_fixture(cli_text: str, docs_text: str, ui_text: str = 'export const shell = "Chaos";\\n') -> subprocess.CompletedProcess[str]:
    with tempfile.TemporaryDirectory() as temporary:
        fixture = Path(temporary)
        cli = fixture / 'crates/codegen/xai-grok-pager-bin/src/main.rs'
        cli.parent.mkdir(parents=True)
        cli.write_text(cli_text, encoding='utf-8')
        docs = fixture / 'crates/codegen/xai-grok-pager/docs'
        docs.mkdir(parents=True)
        (docs / 'reference.md').write_text(docs_text, encoding='utf-8')
        ui = fixture / 'apps/chaos-ui/src'
        ui.mkdir(parents=True)
        (ui / 'main.tsx').write_text(ui_text, encoding='utf-8')
        (fixture / 'apps/chaos-ui/index.html').write_text('<title>Chaos</title>\\n', encoding='utf-8')
        checker = fixture / 'check-brand-protocol.py'
        checker.write_text(CHECKER.read_text(encoding='utf-8'), encoding='utf-8')
        checker_text = checker.read_text(encoding='utf-8').replace(
            "Path(__file__).resolve().parents[2]", f"Path(r'{fixture.as_posix()}')"
        )
        checker.write_text(checker_text, encoding='utf-8')
        return subprocess.run([sys.executable, str(checker)], cwd=fixture, capture_output=True, text=True)


def main() -> int:
    clean = run_with_fixture(
        '// Upstream note: grok workspace was the previous binary name.\n'
        'fn help() { println!("Usage: chaos workspace"); }\n',
        'The compatible old config directory `~/.grok` remains readable.\n',
    )
    if clean.returncode:
        print(clean.stdout, clean.stderr, file=sys.stderr)
        return 1

    bad_cli = run_with_fixture('fn help() { println!("Usage: grok workspace"); }\n', '')
    if bad_cli.returncode != 1 or 'obsolete CLI brand' not in bad_cli.stderr:
        print('brand guard did not reject a user-visible obsolete CLI name', file=sys.stderr)
        return 1

    bad_docs = run_with_fixture('// no obsolete CLI output\n', 'Run `grok workspace list`.\n')
    if bad_docs.returncode != 1 or 'obsolete CLI brand' not in bad_docs.stderr:
        print('brand guard did not reject an obsolete command in shipped docs', file=sys.stderr)
        return 1

    bad_ui = run_with_fixture('// no obsolete CLI output\n', 'Chaos docs only.\n',
                              'export const title = "Grok Build";\\n')
    if bad_ui.returncode != 1 or 'obsolete product label' not in bad_ui.stderr:
        print('brand guard did not reject an obsolete rendered-UI product label', file=sys.stderr)
        return 1

    compatible_word = run_with_fixture('// no obsolete CLI output\n', 'Grok Build migrated to Chaos; `~/.grok` stays compatible.\\n',
                                       'export const title = "Chaos";\\n')
    if compatible_word.returncode:
        print(compatible_word.stdout, compatible_word.stderr, file=sys.stderr)
        return 1
    print('brand protocol guard: compatibility identifiers pass; CLI/doc/UI mutations fail')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
