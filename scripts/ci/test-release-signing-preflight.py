#!/usr/bin/env python3
"""Execute the release preflight with local Ed25519 fixtures, without secrets."""
import base64
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


def preflight_code():
    text = WORKFLOW.read_text(encoding='utf-8')
    step = text.split('- name: Validate signing configuration\n', 1)[1]
    match = re.search(r"- <<'PY'\n(.*?)\n          PY", step, re.DOTALL)
    if not match:
        raise AssertionError('release signing preflight Python heredoc is missing')
    return textwrap.dedent(match[1])


def public_key(seed):
    return Ed25519PrivateKey.from_private_bytes(seed).public_key().public_bytes(
        Encoding.Raw, PublicFormat.Raw)


class SigningPreflightTests(unittest.TestCase):
    def run_preflight(self, seed, *, prefix='', suffix='', public=None):
        env = os.environ.copy()
        env['CHAOS_SIGNING_PRIVATE_KEY'] = prefix + base64.b64encode(seed).decode() + suffix
        env['CHAOS_SIGNING_PUBLIC_KEY'] = base64.b64encode(
            public if public is not None else public_key(seed)).decode()
        return subprocess.run([sys.executable, '-c', preflight_code()], env=env,
                              capture_output=True, text=True)

    def test_valid_base64_prefixes_are_preserved(self):
        # These prefixes were stripped as a character set by lstrip('\\ufeff').
        for first in (0x00, 0x78, 0x7c, 0xb8):
            with self.subTest(first_byte=first):
                result = self.run_preflight(bytes([first]) + bytes(31))
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn('signing configuration validated', result.stdout)

    def test_bom_and_surrounding_whitespace_are_accepted(self):
        for prefix in (' \n\ufeff', ' \n\ufeff \n'):
            with self.subTest(prefix=repr(prefix)):
                result = self.run_preflight(bytes(32), prefix=prefix, suffix='\n ')
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_signer_accepts_the_same_seed_encodings_as_preflight(self):
        text = WORKFLOW.read_text(encoding='utf-8')
        match = re.search(r'"\$PY" -c "\n(.*?)\n          "', text, re.DOTALL)
        self.assertIsNotNone(match, 'release signing Python code is missing')
        code = textwrap.dedent(match[1])
        for first in (0x00, 0x78, 0x7c, 0xb8):
            seed = bytes([first]) + bytes(31)
            for prefix in ('', ' \n\ufeff \n'):
                with self.subTest(first_byte=first, prefix=repr(prefix)):
                    preflight = self.run_preflight(seed, prefix=prefix, suffix='\n ')
                    self.assertEqual(preflight.returncode, 0, preflight.stderr)
                    env = os.environ.copy()
                    env['CHAOS_SIGNING_PRIVATE_KEY'] = prefix + base64.b64encode(seed).decode() + '\n '
                    env['CHAOS_SIGNING_PUBLIC_KEY'] = base64.b64encode(public_key(seed)).decode()
                    with tempfile.TemporaryDirectory() as directory:
                        binary = Path(directory) / 'chaos-linux-x64'
                        binary.write_bytes(b'local signing fixture')
                        result = subprocess.run([sys.executable, '-c', code], cwd=directory,
                                                env=env, capture_output=True, text=True)
                        self.assertEqual(result.returncode, 0, result.stderr)
                        signature = base64.b64decode(binary.with_suffix('.sig').read_bytes().strip(),
                                                     validate=True)
                        Ed25519PrivateKey.from_private_bytes(seed).public_key().verify(
                            signature, binary.read_bytes())

    def test_wrong_public_key_is_rejected(self):
        result = self.run_preflight(bytes(32), public=public_key(bytes([1]) * 32))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('configured signing keys do not match', result.stderr)

    def test_invalid_key_lengths_are_rejected(self):
        for length in (0, 31, 33):
            with self.subTest(length=length):
                result = self.run_preflight(bytes(length), public=public_key(bytes(32)))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('invalid length', result.stderr)

    def test_invalid_base64_is_rejected(self):
        result = self.run_preflight(bytes(32), suffix='!')
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('signing configuration validated', result.stdout)


if __name__ == '__main__':
    unittest.main()
