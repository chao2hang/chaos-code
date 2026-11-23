#!/usr/bin/env python3
"""Run the workflow's release asset verifier and upload predicate with local fixtures."""
import base64
import hashlib
import os
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

WORKFLOW = Path(__file__).resolve().parents[2] / '.github/workflows/release.yml'
REQUIRED = ('chaos-linux-x64', 'chaos-linux-arm64', 'chaos-darwin-arm64',
            'chaos-darwin-x64', 'chaos-win32-x64.exe')
OPTIONAL = 'chaos-win32-arm64.exe'
NOTICES = 'crates/codegen/xai-grok-pager/npm/THIRD_PARTY_NOTICES.md'
SBOM = 'sbom/chaos.cdx.json'


def workflow_step(name):
    text = WORKFLOW.read_text(encoding='utf-8')
    rest = text.split(f'      - name: {name}\n', 1)[1]
    return rest.split('\n      - ', 1)[0]


class ReleaseAssetTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.bins = self.root / 'release-bins'
        self.bins.mkdir()
        self.key = Ed25519PrivateKey.from_private_bytes(bytes(32))
        self.public = self.key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
        for name in REQUIRED:
            self.add_binary(name)
        self.write_checksums()
        for name, contents in ((NOTICES, 'fixture notices\n'), (SBOM, '{"bomFormat":"CycloneDX"}')):
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(contents, encoding='utf-8')

    def add_binary(self, name):
        data = f'local signed fixture {name}\n'.encode()
        (self.bins / name).write_bytes(data)
        (self.bins / (name + '.sig')).write_bytes(base64.b64encode(self.key.sign(data)) + b'\n')

    def write_checksums(self):
        lines = []
        for name in (*REQUIRED, OPTIONAL):
            path = self.bins / name
            if path.exists():
                lines.append(f'{hashlib.sha256(path.read_bytes()).hexdigest()}  {name}\n')
        (self.bins / 'SHA256SUMS').write_text(''.join(lines), encoding='utf-8')

    def verify(self):
        step = workflow_step('Verify release assets')
        script = textwrap.dedent(step.split('        run: |\n', 1)[1])
        output = self.root / 'github-output'
        env = os.environ.copy()
        env.update(CHAOS_SIGNING_PUBLIC_KEY=base64.b64encode(self.public).decode(),
                   SIGNING_PYTHON=sys.executable, GITHUB_OUTPUT=str(output))
        result = subprocess.run(['bash', '-c', script], cwd=self.root, env=env,
                                text=True, capture_output=True)
        emitted = output.read_text() if output.exists() else ''
        return result, emitted

    def assert_rejected(self):
        result, output = self.verify()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(output, '', 'failed verification must never emit upload paths')

    def test_optional_windows_arm64_can_be_absent(self):
        result, output = self.verify()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn(OPTIONAL, output)
        expected = ['release-bins/SHA256SUMS']
        for name in REQUIRED:
            expected.extend(['release-bins/' + name, 'release-bins/' + name + '.sig'])
        expected.extend([NOTICES, SBOM])
        self.assertEqual(output.splitlines(), ['files<<CHAOS_RELEASE_FILES', *expected,
                                              'CHAOS_RELEASE_FILES'])

    def test_optional_windows_arm64_is_verified_and_uploaded_when_present(self):
        self.add_binary(OPTIONAL)
        self.write_checksums()
        result, output = self.verify()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('release-bins/' + OPTIONAL + '\n', output)
        self.assertIn('release-bins/' + OPTIONAL + '.sig\n', output)

    def test_missing_required_binary_is_rejected(self):
        (self.bins / REQUIRED[1]).unlink()
        self.write_checksums()
        self.assert_rejected()

    def test_empty_binary_is_rejected(self):
        (self.bins / REQUIRED[0]).write_bytes(b'')
        self.write_checksums()
        self.assert_rejected()

    def test_missing_signature_is_rejected(self):
        (self.bins / (REQUIRED[0] + '.sig')).unlink()
        self.assert_rejected()

    def test_empty_signature_is_rejected(self):
        (self.bins / (REQUIRED[0] + '.sig')).write_bytes(b'')
        self.assert_rejected()

    def test_invalid_signature_encoding_is_rejected(self):
        (self.bins / (REQUIRED[0] + '.sig')).write_text('!invalid!')
        self.assert_rejected()

    def test_tampered_binary_is_rejected_even_with_matching_checksum(self):
        (self.bins / REQUIRED[0]).write_bytes(b'tampered fixture')
        self.write_checksums()
        self.assert_rejected()

    def test_wrong_public_key_is_rejected(self):
        self.public = Ed25519PrivateKey.from_private_bytes(bytes([1]) * 32).public_key().public_bytes(
            Encoding.Raw, PublicFormat.Raw)
        self.assert_rejected()

    def test_optional_binary_cannot_omit_signature(self):
        self.add_binary(OPTIONAL)
        self.write_checksums()
        (self.bins / (OPTIONAL + '.sig')).unlink()
        self.assert_rejected()

    def test_missing_checksums_are_rejected(self):
        (self.bins / 'SHA256SUMS').unlink()
        self.assert_rejected()

    def test_wrong_checksum_is_rejected(self):
        path = self.bins / 'SHA256SUMS'
        path.write_text(path.read_text().replace(
            hashlib.sha256((self.bins / REQUIRED[0]).read_bytes()).hexdigest(), '0' * 64))
        self.assert_rejected()

    def test_incomplete_checksum_set_is_rejected(self):
        path = self.bins / 'SHA256SUMS'
        path.write_text(''.join(path.read_text().splitlines(keepends=True)[1:]))
        self.assert_rejected()

    def test_duplicate_checksum_entry_is_rejected(self):
        path = self.bins / 'SHA256SUMS'
        path.write_text(path.read_text() + path.read_text().splitlines(keepends=True)[0])
        self.assert_rejected()

    def test_missing_notices_are_rejected(self):
        (self.root / NOTICES).unlink()
        self.assert_rejected()

    def test_missing_sbom_is_rejected(self):
        (self.root / SBOM).unlink()
        self.assert_rejected()

    def test_invalid_sbom_is_rejected(self):
        (self.root / SBOM).write_text('not json')
        self.assert_rejected()


class ReleaseUploadGateTests(unittest.TestCase):
    def allowed(self, *, create=True, **outcomes):
        step = workflow_step('Create GitHub Release')
        expression = step.split('        if: >-\n', 1)[1].split('        uses:', 1)[0]
        expression = expression.replace('always()', 'True').replace('&&', 'and')
        expression = expression.replace('needs.resolve-version.outputs.create_github_release',
                                        repr('true' if create else 'false'))
        expression = re.sub(r'steps\.([a-z_]+)\.outcome',
                            lambda match: repr(outcomes.get(match[1], 'success')), expression)
        return eval(' '.join(expression.split()), {'__builtins__': {}})

    def test_failed_or_skipped_integrity_steps_block_upload(self):
        for step in ('layout', 'checksums', 'signing', 'npm_integrity', 'release_assets'):
            for outcome in ('failure', 'skipped', 'cancelled'):
                with self.subTest(step=step, outcome=outcome):
                    self.assertFalse(self.allowed(**{step: outcome}))

    def test_npm_failure_does_not_block_verified_release(self):
        self.assertTrue(self.allowed(npm='failure'))
        self.assertTrue(self.allowed(npm='skipped'))

    def test_release_checkbox_is_honored(self):
        self.assertFalse(self.allowed(create=False))

    def test_upload_uses_only_verified_paths_and_rejects_missing_files(self):
        step = workflow_step('Create GitHub Release')
        self.assertIn('files: ${{ steps.release_assets.outputs.files }}', step)
        self.assertIn('fail_on_unmatched_files: true', step)

    def test_verifier_runs_before_any_npm_publication(self):
        text = WORKFLOW.read_text(encoding='utf-8')
        self.assertLess(text.index('- name: Verify release assets'), text.index('- name: Publish to npmjs'))
        for name, ident in (('Layout release-bins', 'layout'), ('Generate SHA256SUMS', 'checksums'),
                            ('Sign binaries', 'signing'), ('Verify release assets', 'release_assets'),
                            ('Check npm packages against the signed artifact digests', 'npm_integrity')):
            self.assertIn(f'id: {ident}\n', workflow_step(name))


if __name__ == '__main__':
    unittest.main()
