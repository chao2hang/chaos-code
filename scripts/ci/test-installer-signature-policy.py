import pathlib
import unittest

ROOT = pathlib.Path(__file__).parents[2]

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

if __name__ == '__main__':
    unittest.main()
