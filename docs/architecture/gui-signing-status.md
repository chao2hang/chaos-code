# GUI/CLI release signing status

Date: 2026-09-24.

## Locally enforceable

- Release workflow requires the Ed25519 private secret and public repository
  variable before platform builds; preflight verifies key length and pair match
  without printing key values.
- Release binaries build with `xai-grok-update/require-sig`.
- Updater fails closed when the signature sidecar is unavailable or invalid.
- `install.sh`, `install.ps1`, and `install.bat` now fail closed when a
  sidecar, public key, Python/cryptography verification dependency, or valid
  signature is missing. `CHAOS_SKIP_SIGNATURE=1` is the explicit opt-out.
- `scripts/ci/test-installer-signature-policy.py` prevents silent fallback from
  being reintroduced and runs in CI; it does not claim to cryptographically
  verify real assets.

## External release gates

- GitHub secret/variable names exist, but this session did not read their values
  or expose them. The release preflight must run on a real workflow dispatch to
  establish that stored values are valid and match.
- A signed GitHub Release with its actual `.sig` assets is required to exercise
  successful installation and mismatch rejection end to end.
- PowerShell/cmd paths need a Windows runner. The previous Windows matrix run
  was cancelled before those target jobs completed.
- Do not create a new version tag while any item above is unresolved.
