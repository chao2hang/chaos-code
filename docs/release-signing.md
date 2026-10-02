# Release signing runbook

The Ed25519 key pair behind `chaos update` and the installers: where each half
lives, what consumes it, how to rotate it, what breaks when you do, and how to
prove a published release is intact.

Related code:
[`signature.rs`](../crates/codegen/xai-grok-update/src/signature.rs) (verify
side only),
[`release.yml`](../.github/workflows/release.yml) (sign side),
[`install.sh`](../scripts/install.sh) /
[`install.ps1`](../scripts/install.ps1) /
[`install.bat`](../scripts/install.bat) (installer verify side).

Latest measured run against a real release:
[`docs/verification/release-signature-v0.4.2-2026-10-02.log`](verification/release-signature-v0.4.2-2026-10-02.log).

## What is signed, and what is not

`release.yml` signs the six release assets, each with a detached sidecar named
`<asset>.sig`:

| asset | sidecar |
| --- | --- |
| `chaos-linux-x64` | `chaos-linux-x64.sig` |
| `chaos-linux-arm64` | `chaos-linux-arm64.sig` |
| `chaos-darwin-arm64` | `chaos-darwin-arm64.sig` |
| `chaos-darwin-x64` | `chaos-darwin-x64.sig` |
| `chaos-win32-x64.exe` | `chaos-win32-x64.exe.sig` |
| `chaos-win32-arm64.exe` | `chaos-win32-arm64.exe.sig` |

The format is deliberately the smallest thing that works: raw Ed25519 over the
whole file (no pre-hash), and the sidecar is the bare base64 of the 64-byte
signature — no minisign armor, no trusted comment, no key id. The public key is
the bare base64 of the 32-byte raw key. `signature.rs` documents the choice and
only ever verifies; nothing in this repository can sign.

Not covered by the signature:

- `SHA256SUMS` is published next to the assets but is **not** signed. It comes
  from the same release as the artifact it describes, so it catches corruption
  and partial uploads, not someone who can rewrite the release.
- npm. `chaos-code` and the six `chaos-code-<platform>` packages carry a
  brotli-compressed binary inside the tarball
  ([`chaos-bootstrap.js`](../crates/codegen/xai-grok-pager/npm/chaos/bin/chaos-bootstrap.js)
  decompresses it; it never contacts GitHub and never reads a `.sig`). The trust
  anchor for `npm i -g chaos-code` is the npm registry plus TLS. The signature
  picks up on the *next* hop: once installed, `chaos update` verifies the sidecar
  before activating a build.
- The release index. The updater discovers the newest version through the GitHub
  releases API, which is protected by TLS but not signed, so a compromised index
  can withhold updates or point at a published artifact — it cannot make an
  installed binary run bytes that were never signed. This is the residual limit
  recorded on the `sha256 / 签名 / 版本索引 / feed` TODO row.

## Where each half lives

| name | kind | value | read by |
| --- | --- | --- | --- |
| `CHAOS_SIGNING_PRIVATE_KEY` | Actions **secret** | base64 of the raw 32-byte seed | `signing-preflight`, `Sign binaries` |
| `CHAOS_SIGNING_PUBLIC_KEY` | Actions **variable** | base64 of the raw 32-byte key | build step (compile-time), three installers, `verify-release-signature.sh` |

The public half is a variable, not a secret, on purpose: every installed binary
already carries it and `gh variable list` prints it in clear text.

The public key reaches a binary at **compile time** through
`option_env!("CHAOS_SIGNING_PUBLIC_KEY")`. `build.rs` carries
`cargo:rerun-if-env-changed=CHAOS_SIGNING_PUBLIC_KEY` because Cargo tracks no
environment variables by default; without it, a rebuild after changing the key
relinks nothing and silently keeps whichever key the previous build embedded.

The same value is copied into the three installers as a built-in default
(`DEFAULT_SIGNING_PUBLIC_KEY` in `install.sh`, `$DefaultSigningPublicKey` in
`install.ps1`, one line in `install.bat`), because `curl | bash` has no way to
read a repository variable and the command on the README front page has to work
without environment setup. `python3 scripts/ci/test-installer-signature-policy.py`
asserts all three carry the identical 32-byte key; the `CHAOS_SIGNING_PUBLIC_KEY`
environment variable still overrides them for anyone signing their own releases.

## Setting the pair up, or replacing it

Generate a pair (portable across `cryptography` versions — the older releases in
the wild have neither `private_bytes_raw()` nor the `no_encrypt=` keyword):

```python
import base64, os
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, NoEncryption, PublicFormat

sk = Ed25519PrivateKey.from_private_bytes(os.urandom(32))
seed = sk.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption())
pub = sk.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
print('secret  ', base64.b64encode(seed).decode())
print('variable', base64.b64encode(pub).decode())
```

Publish both, then check that they agree the way the workflow checks it:

```sh
gh secret set CHAOS_SIGNING_PRIVATE_KEY -R <owner>/<repo>       # paste the base64 seed
gh variable set CHAOS_SIGNING_PUBLIC_KEY -R <owner>/<repo> --body '<base64 pub>'
gh run view <release-run> -R <owner>/<repo> --log | grep 'signing configuration validated'
```

Set the secret from a POSIX shell. Both the preflight and the sign step decode
with `.strip().lstrip('\ufeff')` because a UTF-8 BOM, introduced when the secret
was pasted from a Windows shell, is what failed release run 31796456487 — a BOM
is not valid base64 and the derived key did not match the variable.

After changing the variable, the copies inside the repository have to follow or
they are a second, contradicting claim:

1. the three installers' built-in default (three files, one line each);
2. `python3 scripts/ci/test-installer-signature-policy.py` — must stay green;
3. every binary built before the change still carries the old key. That is the
   next section.

## Rotating is a hard cut

`signature::public_key()` returns exactly one key, fixed when the crate was
compiled. There is no key id, no key list, no acceptance window, and no
revocation. Consequences:

- A binary built with key B refuses every artifact signed with key A, and a
  binary built with key A refuses every artifact signed with key B. Since a
  release can only embed one key, rotating strands every pre-rotation build:
  `chaos update` across the rotation boundary fails with `signature
  verification failed` and keeps the running version.
- Those users are not stuck, but they move by a different door: `install.sh`,
  `install.ps1` and `install.bat` carry the key from the repository rather than
  from their own build, so a reinstall follows the rotation. Same for
  `npm i -g chaos-code`.
- There is no way to tell an already-installed binary to stop trusting key A.
  If the private half leaks, the response is to stop publishing under it and
  move users with the installers/npm; nothing revokes it in place. Adding a key
  id and a rotation window is the open gap on the signing TODO row.

So the practical rule: rotate only at a boundary you are willing to make users
cross by reinstalling, and record the date, the new public key and the reason
here.

## Who can sign

- Reading `CHAOS_SIGNING_PRIVATE_KEY` needs `actions: write` on the repository
  (admins by default). The value is masked in logs and is never checked out.
  If the repository ever adds GitHub environments, pin the secret to the release
  environment so branch protection gates it.
- Anyone who can trigger `release.yml` with `actions: write` can produce a signed
  release, because the workflow signs with the secret on the runner. The real
  trust boundary is the workflow file and write access to the repository, not the
  secret storage. Keep `release.yml` changes reviewable for this reason.
- Nothing outside a workflow run needs the private half. It is never used to sign
  a local build, and no test in this repository has ever held it: the accept side
  is exercised by fetching a published release and its public variable
  (`scripts/verify-release-signature.sh`).

## What refuses, and what a user sees

`signature_required()` = the `require-sig` Cargo feature, overridden by the
`CHAOS_REQUIRE_SIG` environment variable, which always wins
(`0`/`false`/`no`/`off` disable it; anything else enables it). `release.yml`
builds with `--features xai-grok-update/require-sig`, so shipped binaries
enforce by default.

| situation | behaviour |
| --- | --- |
| no `CHAOS_SIGNING_PUBLIC_KEY` at build time | `is_placeholder_key()` is true, verification returns `NoPublicKey`, the update is refused; `verify_release_artifact` exits 2 |
| `.sig` missing or unreadable | `SignatureUnreadable`, refused |
| signature does not match | `VerificationFailed`, refused, the running install is kept |
| signature matches | activated |

Errors never echo key material or signature bytes.

Installers fail closed the same way, and fail **before** spending a 150 MB+
transfer: `require_signature_prerequisites` resolves the key and probes for
`python3` + `cryptography` first, so a blank `CHAOS_SIGNING_PUBLIC_KEY` or a
machine without the crypto binding exits 1 with a named reason. A present-but-
foreign key downloads, refuses with `signature verification FAILED`, and leaves
the already-installed artifact byte-identical. Both controls are run against the
real release in a stock Debian container by
`scripts/install-sh-in-docker.sh`.

`CHAOS_SKIP_SIGNATURE=1` and `CHAOS_SKIP_CHECKSUM=1` are the installer escape
hatches and print a warning when used; `CHAOS_REQUIRE_SIG=0` is the updater's.
They exist to recover from a misconfigured release, not for routine use — an
install through any of them is trusting the download.

## Checking a release you (or someone else) just published

```sh
scripts/verify-release-signature.sh --tag v0.4.2 --all
```

Three claims per artifact, and the script fails if any is missing: the digest
matches `SHA256SUMS`; the sidecar verifies under the configured key **and** a
one-byte corruption of the same file is refused (without that control, a verifier
that accepts anything also prints "verified"); and a build given no key refuses
instead of accepting. Needs `curl`, `cargo`, and `gh` unless `--tag` and
`--public-key` are both passed. One artifact is a hundred-ish MB.

For a single file, with no network beyond the two paths:

```sh
CHAOS_SIGNING_PUBLIC_KEY="$(gh variable get CHAOS_SIGNING_PUBLIC_KEY -R <owner>/<repo>)" \
  cargo build --release --example verify_release_artifact -p xai-grok-update
./target/release/examples/verify_release_artifact chaos-linux-x64 chaos-linux-x64.sig --tamper
```

Exit status: 0 verified, 1 refused, 2 no public key compiled in.

## Cutting a release: the signing steps

1. `python3 scripts/ci/check-version-lockstep.py` — the version the release is
   built from has to be the version the updater will compare against
   ([CONTRIBUTING.md](../CONTRIBUTING.md), *Release versioning*).
2. `scripts/verify-release-signature.sh --tag <previous> --all` against the last
   release. This is the pre-flight on the key pair: if the current variable no
   longer verifies the last published artifacts, the secret and the variable have
   drifted and the next release will strand people who upgrade from it.
3. Dispatch `release.yml`. Watch `signing-preflight` first — it fails with
   `CHAOS_SIGNING_PRIVATE_KEY is not configured`,
   `CHAOS_SIGNING_PUBLIC_KEY is not configured`, `signing key material has an
   invalid length`, or `configured signing keys do not match`, all before any
   platform build starts.
4. After publish, run step 2 against the new tag and keep the transcript under
   `docs/verification/`.
5. `scripts/install-sh-in-docker.sh` if the installers or the key changed: it is
   the only check that installs the published artifact on a machine that has
   never seen this repository, using only the key the script ships with.
