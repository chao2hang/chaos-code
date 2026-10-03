#!/usr/bin/env python3
"""Fixtures for scripts/ci/check-protocol-mirror.py.

The guard exists because six messages the engine speaks were missing from the
browser's protocol types while `check-gui-protocol.sh` stayed green -- both of
its inputs are the same hand-written text. So the fixtures here are mostly
negative: each way the mirror can drift is injected into a throwaway pair of
files and asserted to exit 1 with the drifted name in the message.

Two of them pin parser behaviour that was wrong while these were written. A type
argument (`Vec<serde_json::Value>`) was once read as a field named `serde_json`,
and the keys of an inline nested TS object type were once compared against Rust
field names. Both would have made the guard cry wolf until somebody switched it
off.

The last group runs against the real repository, because a guard that only
understands fixtures proves nothing about the files CI actually checks.
"""

from __future__ import annotations

import importlib.util
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name('check-protocol-mirror.py')
REPO = SCRIPT.parent.parent.parent
ENGINE = REPO / 'crates/codegen/chaos-engine/src/lib.rs'
MIRROR = REPO / 'crates/codegen/chaos-engine/src/protocol_schema.rs'

_spec = importlib.util.spec_from_file_location('check_protocol_mirror', SCRIPT)
assert _spec and _spec.loader
guard = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(guard)

ENGINE_SRC = '''
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Submit {
        client_msg_id: String,
        session_id: Uuid,
        prompt: String,
    },
    Cancel {
        client_msg_id: String,
    },
    HttpGetFile {
        client_msg_id: String,
        url: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Ack {
        client_msg_id: String,
    },
    Scan {
        entries: Vec<serde_json::Value>,
        catalog_loaded: bool,
    },
}
'''

MIRROR_SRC = '''pub const TYPESCRIPT: &str = r#"
export type UUID = string

export type ClientMessage =
  | { type: 'submit'; client_msg_id: string; session_id: UUID; prompt: string }
  | { type: 'cancel'; client_msg_id: string }
  | { type: 'http_get_file'; client_msg_id: string; url: string }

export type ServerMessage =
  | { type: 'ack'; client_msg_id: string }
  | { type: 'scan'; entries: unknown[]; catalog_loaded: boolean }
"#;
'''

# The wire tags the engine had been speaking with no mirror entry at all.
ATTACHMENT_TAGS = (
    'begin_attachment',
    'attachment_chunk',
    'cancel_attachment',
    'attachment_started',
    'attachment_progress',
    'attachment_cancelled',
)


def run(engine: Path, mirror: Path, *extra: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(SCRIPT), '--engine', str(engine), '--mirror', str(mirror), *extra],
        capture_output=True,
        text=True,
    )


class FixtureHarness(unittest.TestCase):
    """Writes a matching engine/mirror pair, then applies one drift to it."""

    def assert_drift(self, drift, message: str, engine: str = ENGINE_SRC) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            engine_path = root / 'lib.rs'
            mirror_path = root / 'protocol_schema.rs'
            engine_path.write_text(engine, encoding='utf-8')
            mirror_path.write_text(drift(MIRROR_SRC), encoding='utf-8')
            result = run(engine_path, mirror_path)
            self.assertEqual(result.returncode, 1, f'{message}\n{result.stdout}\n{result.stderr}')
            self.assertIn(message, result.stderr)

    def assert_clean(self, engine: str, mirror: str) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            engine_path = root / 'lib.rs'
            mirror_path = root / 'protocol_schema.rs'
            engine_path.write_text(engine, encoding='utf-8')
            mirror_path.write_text(mirror, encoding='utf-8')
            result = run(engine_path, mirror_path)
            self.assertEqual(result.returncode, 0, f'{result.stdout}\n{result.stderr}')


class ParserTests(unittest.TestCase):
    def test_serde_rename_all_snake_case(self):
        self.assertEqual(guard.snake_case('TuiSessionImport'), 'tui_session_import')
        self.assertEqual(guard.snake_case('HTTPStatus'), 'http_status')
        self.assertEqual(guard.snake_case('HttpGetFile'), 'http_get_file')
        self.assertEqual(guard.snake_case('Ack'), 'ack')
        self.assertEqual(guard.snake_case('AttachmentChunk'), 'attachment_chunk')

    def test_rust_field_names_ignore_type_arguments(self):
        variants = guard.rust_variants(guard.enum_body(ENGINE_SRC, 'ServerMessage'))
        self.assertEqual(variants['Scan'], {'entries', 'catalog_loaded'})

    def test_rust_field_names_ignore_a_wrapped_type_path(self):
        # rustfmt puts a type that does not fit on one line onto its own line, and
        # such a line begins with an identifier followed by `::`.
        body = """
            Catalog {
                entries: Vec<
                    serde_json::Value,
                >,
                catalog_loaded: bool,
            },
        """
        self.assertEqual(
            guard.rust_variants(body)['Catalog'], {'entries', 'catalog_loaded'})

    def test_ts_keys_stop_at_the_outer_brace(self):
        keys = guard.ts_object_keys(
            "  | { type: 'ack'; client_msg_id: string; meta: { seq: number } }"
        )
        self.assertEqual(keys, {'client_msg_id', 'meta'})

    def test_unions_are_read_per_enum(self):
        unions, problems = guard.mirror_unions(MIRROR_SRC)
        self.assertEqual(problems, [])
        self.assertEqual(
            sorted(unions), ['ClientMessage', 'ServerMessage'])
        self.assertEqual(
            sorted(unions['ClientMessage']), ['cancel', 'http_get_file', 'submit'])


class DriftTests(FixtureHarness):
    def test_matching_pair_passes(self):
        self.assert_clean(ENGINE_SRC, MIRROR_SRC)

    def test_unmirrored_message_is_refused(self):
        self.assert_drift(
            lambda text: text.replace(
                "  | { type: 'cancel'; client_msg_id: string }\n", ''),
            "'cancel' (Rust Cancel)",
        )

    def test_mirrored_message_the_engine_never_sends_is_refused(self):
        self.assert_drift(
            lambda text: text.replace(
                "'cancel'", "'drop_box'"),
            "'drop_box' as a ClientMessage",
        )

    def test_missing_field_is_refused(self):
        self.assert_drift(
            lambda text: text.replace('; prompt: string', ''),
            "'submit' is missing field(s) prompt",
        )

    def test_extra_field_is_refused(self):
        self.assert_drift(
            lambda text: text.replace(
                "'cancel'; client_msg_id: string",
                "'cancel'; client_msg_id: string; reason: string"),
            "'cancel' mirrors field(s) reason",
        )

    def test_optional_field_spelling_still_counts(self):
        # `workspace_id?: UUID` describes the same wire field as `Option<Uuid>`.
        self.assert_clean(
            ENGINE_SRC.replace(
                '    Cancel {\n        client_msg_id: String,\n    },',
                '    Cancel {\n        client_msg_id: String,\n        reason: Option<String>,\n    },'),
            MIRROR_SRC.replace(
                "'cancel'; client_msg_id: string",
                "'cancel'; client_msg_id: string; reason?: string"),
        )

    def test_acronym_spelling_must_match_serde(self):
        self.assert_drift(
            lambda text: text.replace("'http_get_file'", "'httpgetfile'"),
            "'httpgetfile' as a ClientMessage",
        )

    def test_duplicate_tag_is_refused(self):
        self.assert_drift(
            lambda text: text.replace(
                "  | { type: 'ack'; client_msg_id: string }\n",
                "  | { type: 'ack'; client_msg_id: string }\n"
                "  | { type: 'ack'; other: string }\n"),
            "ServerMessage lists 'ack' twice",
        )

    def test_variant_pair_colliding_under_snake_case_is_refused(self):
        engine = ENGINE_SRC.replace(
            '    Cancel {\n        client_msg_id: String,\n    },',
            '    SsEvent {\n        client_msg_id: String,\n    },\n'
            '    SSEvent {\n        client_msg_id: String,\n    },')
        self.assert_drift(
            lambda text: text,
            'collapse to the same wire tag',
            engine=engine,
        )

    def test_missing_union_is_refused(self):
        self.assert_drift(
            lambda text: text.replace('export type ClientMessage =', 'export type BrowserMessage ='),
            'no `export type ClientMessage =` union',
        )

    def test_tagged_entry_outside_a_union_is_refused(self):
        self.assert_drift(
            lambda text: text.replace(
                'export type UUID = string',
                "export type UUID = string\n  | { type: 'orphan'; a: string }"),
            'not inside a',
        )

    def test_nested_ts_object_keys_are_not_wire_fields(self):
        self.assert_clean(
            ENGINE_SRC.replace(
                '    Ack {\n        client_msg_id: String,\n    },',
                '    Ack {\n        client_msg_id: String,\n        meta: serde_json::Value,\n    },'),
            MIRROR_SRC.replace(
                "'ack'; client_msg_id: string",
                "'ack'; client_msg_id: string; meta: { seq: number }"),
        )

    def test_type_argument_is_not_a_wire_field(self):
        # `Vec<serde_json::Value>` used to contribute a phantom field `serde_json`.
        self.assert_clean(ENGINE_SRC, MIRROR_SRC.replace(
            'entries: unknown[]', 'entries: Array<{ name: string }>'))

    def test_unterminated_enum_is_an_error_not_a_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            engine_path = root / 'lib.rs'
            mirror_path = root / 'protocol_schema.rs'
            engine_path.write_text(
                ENGINE_SRC[: ENGINE_SRC.index('pub enum ServerMessage')], encoding='utf-8')
            mirror_path.write_text(MIRROR_SRC, encoding='utf-8')
            result = run(engine_path, mirror_path)
            self.assertEqual(result.returncode, 1, result.stdout)
            self.assertIn('no `pub enum ServerMessage`', result.stderr + result.stdout)

    def test_a_wrapped_type_line_does_not_become_a_wire_field(self):
        wrapped = ENGINE_SRC.replace(
            '        entries: Vec<serde_json::Value>,\n',
            '        entries: Vec<\n            serde_json::Value,\n        >,\n')
        self.assertIn('entries: Vec<\n', wrapped)
        self.assert_clean(wrapped, MIRROR_SRC)

    def test_a_commented_out_copy_of_the_enum_is_not_parsed(self):
        # Design notes quote the enum; the guard must still read the compiled one.
        decoy = (
            '// Sketch from the design notes, never compiled:\n'
            '// pub enum ClientMessage {\n'
            '//     Legacy { old_field: String },\n'
            '// }\n'
            + ENGINE_SRC)
        self.assert_clean(decoy, MIRROR_SRC)
        self.assert_drift(
            lambda text: text.replace(
                "  | { type: 'cancel'; client_msg_id: string }\n", ''),
            "'cancel' (Rust Cancel)",
            engine=decoy,
        )


class RealRepositoryTests(unittest.TestCase):
    def test_the_shipped_mirror_is_complete(self):
        result = run(ENGINE, MIRROR)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_every_attachment_message_is_mirrored(self):
        text = MIRROR.read_text(encoding='utf-8')
        for tag in ATTACHMENT_TAGS:
            self.assertIn(f"type: '{tag}'", text, f'{tag} is not in the mirror')

    def test_dropping_one_real_entry_reddens_the_guard(self):
        """The guard reads the shipped files, it does not carry a list."""
        original = MIRROR.read_text(encoding='utf-8')
        lines = original.splitlines(keepends=True)
        for tag in ATTACHMENT_TAGS:
            with self.subTest(tag=tag):
                kept = [line for line in lines if f"type: '{tag}'" not in line]
                self.assertEqual(len(kept), len(lines) - 1, f'{tag} is not on its own line')
                with tempfile.TemporaryDirectory() as directory:
                    mirror_path = Path(directory) / 'protocol_schema.rs'
                    mirror_path.write_text(''.join(kept), encoding='utf-8')
                    result = run(ENGINE, mirror_path)
                    self.assertEqual(result.returncode, 1, f'{tag} removal went unnoticed')
                    self.assertIn(f"'{tag}'", result.stderr)

    def test_dropping_one_real_field_reddens_the_guard(self):
        text = MIRROR.read_text(encoding='utf-8')
        drifted = text.replace(
            "| { type: 'attachment_progress'; upload_id: UUID; received: number }",
            "| { type: 'attachment_progress'; upload_id: UUID }")
        self.assertNotEqual(drifted, text)
        with tempfile.TemporaryDirectory() as directory:
            mirror_path = Path(directory) / 'protocol_schema.rs'
            mirror_path.write_text(drifted, encoding='utf-8')
            result = run(ENGINE, mirror_path)
            self.assertEqual(result.returncode, 1, result.stdout)
            self.assertIn("'attachment_progress' is missing field(s) received", result.stderr)

    def test_print_reports_full_coverage(self):
        result = run(ENGINE, MIRROR, '--print')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        covered = re.findall(r'(\d+) of (\d+) variants mirrored', result.stdout)
        self.assertEqual(len(covered), 2, result.stdout)
        for done, total in covered:
            self.assertEqual(done, total, result.stdout)


if __name__ == '__main__':
    unittest.main(verbosity=2)
