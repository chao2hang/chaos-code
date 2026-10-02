import base64
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).parents[2]


def embedded_key(text, pattern):
    """The default signing key an installer ships with, or fail the test."""
    match = re.search(pattern, text, re.MULTILINE)
    if not match:
        raise AssertionError('no embedded CHAOS_SIGNING_PUBLIC_KEY default')
    return match.group(1)


class InstallerSignaturePolicyTests(unittest.TestCase):
    def test_unix_installer_fails_closed_without_sidecar_key_or_crypto(self):
        text = (ROOT / 'scripts/install.sh').read_text()
        self.assertIn('required signature sidecar unavailable', text)
        self.assertIn('CHAOS_SIGNING_PUBLIC_KEY is required', text)
        self.assertIn('python3 with cryptography is required', text)
        self.assertIn('CHAOS_SKIP_SIGNATURE', text)

    def test_windows_installers_have_fail_closed_guards(self):
        ps = (ROOT / 'scripts/install.ps1').read_text()
        bat = (ROOT / 'scripts/install.bat').read_text()
        for text in (ps, bat):
            self.assertIn('CHAOS_SKIP_SIGNATURE', text)
            self.assertIn('cryptography', text)
            self.assertIn('CHAOS_SIGNING_PUBLIC_KEY', text)
        self.assertIn('required signature sidecar unavailable', bat)
        self.assertIn('signature verification', ps.lower())
        self.assertIn('required', ps.lower())

    def test_every_installer_ships_the_same_public_key(self):
        # curl|bash has no key of its own, so a release whose sidecars verify under
        # key A cannot be installed by an installer holding key B. All three carry the
        # same value; scripts/verify-release-signature.sh checks it against a real
        # published release and scripts/install-sh-in-docker.sh checks it against the
        # CHAOS_SIGNING_PUBLIC_KEY repository variable.
        sh = embedded_key(
            (ROOT / 'scripts/install.sh').read_text(),
            r"^DEFAULT_SIGNING_PUBLIC_KEY='([^']+)'$",
        )
        ps = embedded_key(
            (ROOT / 'scripts/install.ps1').read_text(),
            r'^\$DefaultSigningPublicKey = "([^"]+)"$',
        )
        bat = embedded_key(
            (ROOT / 'scripts/install.bat').read_text(),
            r'CHAOS_SIGNING_PUBLIC_KEY=([^"\r\n]+)"',
        )
        self.assertEqual(sh, ps)
        self.assertEqual(sh, bat)
        # A bare ed25519 public key: 32 bytes, base64, no armor or whitespace.
        self.assertEqual(len(base64.b64decode(sh)), 32)

    def test_unix_installer_settles_the_key_before_downloading(self):
        # The artifact is 150 MB+. Prerequisite failures must precede the transfer,
        # otherwise a missing key costs the user a full download before saying so.
        text = (ROOT / 'scripts/install.sh').read_text()
        self.assertLess(
            text.index('require_signature_prerequisites\n'),
            text.index('USED_URL="$(download_github "$ORIGIN_URL"'),
            'install.sh must resolve the key and probe python before downloading',
        )

    def test_windows_installer_settles_the_key_before_downloading(self):
        text = (ROOT / 'scripts/install.ps1').read_text()
        self.assertLess(
            text.index('$signingPubKey = $DefaultSigningPublicKey'),
            text.index('Download-GitHubFile -OriginUrl $originUrl'),
            'install.ps1 must resolve the key and probe python before downloading',
        )

    def test_unix_installer_calls_both_integrity_checks(self):
        # install.sh kept both checks in a function each, and the checksum one lost
        # its call site: every SHA256SUMS error message in the file stayed reachable
        # only in theory, and a clean install printed no 'checksum OK' line. Defining
        # a check is not running it, so assert on the call sites and on their order
        # relative to the point where the artifact becomes executable.
        text = (ROOT / 'scripts/install.sh').read_text()
        calls = {}
        for name in ('verify_checksum', 'verify_signature'):
            sites = re.findall(rf'^{name}[ \t]*$', text, re.MULTILINE)
            self.assertEqual(
                len(sites), 1,
                f'{name} must be called exactly once at the top level '
                f'(found {len(sites)} call sites)',
            )
            calls[name] = text.index(f'\n{name}\n')
        self.assertLess(
            calls['verify_checksum'], calls['verify_signature'],
            'install.sh checks the published digest before the signature',
        )
        self.assertLess(
            calls['verify_signature'], text.index('chmod +x "$TMP"'),
            'install.sh must finish both checks before making the artifact executable',
        )

    def test_every_installer_reports_a_verified_digest(self):
        # The three installers verify the same release asset; each has to say so, in
        # the same words, on the path that runs (the .sh one is inside a function,
        # the other two inline).
        for name in ('install.sh', 'install.ps1', 'install.bat'):
            text = (ROOT / 'scripts' / name).read_text()
            self.assertIn('checksum OK', text, f'{name} never reports a verified digest')
            self.assertIn('CHAOS_SKIP_CHECKSUM', text,
                          f'{name} has no documented checksum escape hatch')
            self.assertIn('SHA256SUMS', text, f'{name} never consults SHA256SUMS')

if __name__ == '__main__':
    unittest.main()
