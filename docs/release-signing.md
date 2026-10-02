# Release signing runbook

The Ed25519 key pair behind `chaos update` and the installers: where each half
lives, what consumes it, how to rotate it, what breaks when you do, and how to
prove a published release is intact.

Related code:
[`xai-grok-signature`](../crates/codegen/xai-grok-signature/src/lib.rs) (verify
side only; re-exported as `xai_grok_update::signature`),
[`release.yml`](../.github/workflows/release.yml) (sign side),
[`install.sh`](../scripts/install.sh) /
[`install.ps1`](../scripts/install.ps1) /
[`install.bat`](../scripts/install.bat) (installer verify side),
[`remote/provenance.rs`](../crates/codegen/chaos-engine/src/remote/provenance.rs)
(the same check on the `chaos-remote install` path).

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
the bare base64 of the 32-byte raw key. `xai-grok-signature` documents the choice
and only ever verifies; nothing in this repository can sign. Its reader is
forgiving about the one thing a shell pipeline is not: `... | base64` wraps those
88 characters at column 76, and a wrapped sidecar is read back as the single
signature it is. `-----` armor lines and minisign's `comment:` lines are skipped
too, so a sidecar made by hand does not fail for being made by hand.

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

That run can only ever see a correct release, so `scripts/install-integrity-in-docker.sh`
supplies the other half: it builds a release of its own (artifact, `SHA256SUMS` row,
`.sig` over the artifact bytes), serves it through the mirror path the installer already
supports, and runs in a container with `--network none`. A tampered artifact, a
`SHA256SUMS` recomputed to match the tampering, a missing sidecar and a valid-but-foreign
key are each refused there, with nothing installed behind any of them -- so the accepting
line above is known to be a check rather than a message. That lab also measured the cost
of the two escape hatches: skipping the checksum alone still leaves the signature refusing
the tampered artifact; skipping both installs it.

`install.ps1` is now executed the same way, by `scripts/install-integrity-powershell.sh`:
pwsh on Linux against the *same* fixture -- the generator, mirror and request-log assertion
are shared in `scripts/ci/release-integrity-*.py`, deliberately, so the two installers
cannot drift into being tested against different releases -- inside `unshare -rn`, where
the only route is loopback and `github.com` resolves to nothing. It ends with the identical
refusal set, plus two checks that came out of writing it. A truncated transfer is refused
before hashing: `install.ps1` has always rejected an artifact under 1 MiB, `install.sh` had
no equivalent floor so a body cut off mid-transfer reached the hasher, and `install.bat`
used the same 1 MiB figure only to decide whether to sniff for HTML -- a short non-HTML body
fell through to `certutil` and was reported as a checksum mismatch. All three now refuse
under 1 MiB up front, and a check in the shell lab reads all three files and fails if those
numbers drift apart. And when every candidate fails, both script installers print up to four
distinct reasons instead of only the last one -- before, a mirror answering 200 with an HTML
error page was reported as a DNS failure at a public mirror that was never the problem.

Both labs run in CI now, in the `installer integrity labs` job, on every push. Neither has a
skip path: a missing `pwsh`, a missing `python3-cryptography`, or a kernel that will not
create a network namespace exits 2 with a named reason.

Still unmeasured about the Windows installer, and the lab says so in its own header: which
asset name `[RuntimeInformation]::OSArchitecture` asks for, whether Windows executes the
bytes it wrote, and the registry `PATH` write (every run passes `-NoPath`).
`install.bat` remains unexecuted entirely -- nothing on a Linux or macOS machine runs a
batch file -- so what is known about it comes from its source plus three static guards: the
floor check in the shell lab above, the structural assertions in
`scripts/ci/test-installer-signature-policy.py` (that it embeds the key, and that the key
and the crypto probe precede the download), and the fact that its PowerShell sibling is
parse-checked by `scripts/ci/check-powershell-syntax.py`, which is how the brace that had
made `install.ps1` unparsable since `21f5a186` was found. So "the Windows installer puts a
working binary on PATH" is still an untested claim; "the Windows installer accepts these
bytes and refuses those" is not.

`CHAOS_SKIP_SIGNATURE=1` and `CHAOS_SKIP_CHECKSUM=1` are the installer escape
hatches and print a warning when used; `CHAOS_REQUIRE_SIG=0` is the updater's.
They exist to recover from a misconfigured release, not for routine use — an
install through any of them is trusting the download.

## The same key on the `chaos-remote install` path

`chaos-remote install <version> --from FILE` puts a `chaos-remote-server` build onto
the host it is connected to and points that host's `current` at it. It checked one
thing: the sha256 the client computed over the bytes it was sending. That proves the
upload arrived whole and nothing about who produced it, so a leaked credential or a
tampered build pipeline could make its own bytes the next-starting server on a box.
Now the same ed25519 key the release assets are signed with decides that question.

The client reads a sidecar — `FILE.sig` next to the artifact, or whatever `--signature`
names — and sends its base64 body in the optional `signature_b64` field of
`InstallBegin`. A field that may be absent is not a protocol change, so an older client
still deploys to a newer host and vice versa. The host's order is
[digest → signature → platform header → first filesystem change], which is why a refusal
leaves no version directory, no pointer movement and no half-written upload behind.

| switch | effect |
| --- | --- |
| `--trust-signing-key <base64>` or `@path` | check offered signatures against this key |
| `CHAOS_SIGNING_PUBLIC_KEY` | same, from the host's environment |
| compiled-in `CHAOS_SIGNING_PUBLIC_KEY` | same, from the build; the all-zeros placeholder means "no key" |
| `--allow-unsigned-artifact` | install unsigned builds; a signature that still arrives is checked |
| `CHAOS_REMOTE_REQUIRE_SIGNATURE=0` | the same opt-out, for a unit file or a script |

The default is the fail-closed one: a signature is required even when no key is
configured, and such a host says at startup that every install will be refused. That is
deliberate — "verify, but nobody told me against what" is not a state worth starting in,
and a host that quietly accepted everything until someone remembered a key carries the
old exposure invisibly. Refusals carry a reason code: `signature_missing`,
`signature_malformed`, `signature_invalid`, `no_trusted_key`, `artifact_too_large`,
`wrong_platform`.

Why the host may take its key at runtime when the updater may not: the updater's trust
anchor is chosen by the party that signed the binary it is replacing, so a runtime
override there would let a hostile update server pick its own key. A remote host's key
is chosen by that host's operator, on the command line or in its own environment, before
any artifact is offered — the same relationship `CHAOS_REQUIRE_SIG` has for the updater,
and the reason `parse_public_key_b64` is public while `public_key()` is not.

`wrong_platform` belongs in the same paragraph because it closes the adjacent hole: an
artifact that is intact and correctly signed but built for another CPU or another
operating system. `chaos-remote-server` installs a build of itself, so the host reads the
ELF / Mach-O / PE header and compares the target the file states with the platform it is
actually running on. A file that states nothing readable — a shell wrapper, an architecture
the table has no entry for — is not refused; the check exists to catch a mismatch it can
name, not to guess.

What this does not buy, said plainly: a session granted the `tool-execution` capability
can already run the programs its server allowlisted, and a session with write access can
already change files. Provenance protects the boundary "these bytes become the program
that starts next", which is the one no capability grant implies and no digest covers.

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
   `python3 scripts/ci/test-installer-asset-names.py` is its sibling for the other half
   of an artifact's identity: the four places that ask a release for a file
   (`install.sh`, `install.ps1`, `install.bat`, `chaos update`) have to ask for names this
   workflow actually publishes. Both also run in CI, so this step is a pre-flight, not the
   only place they are checked.
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
6. `scripts/install-integrity-in-docker.sh` whenever the installer's *verification
   logic* changed. It needs no network and no release: it builds and signs its own
   artifact and then tries to make the installer accept a wrong one. Run it after
   step 5, not instead of it -- a fixture only proves the refusals, the real feed
   only proves the acceptance.
7. `scripts/install-integrity-powershell.sh` whenever `install.ps1` or the shared
   fixture changed. Same fixture as step 6, no Windows and no network needed; needs
   `pwsh` and `unshare`. It is the only thing that executes the Windows installer's
   download-and-verify path, so an `install.ps1` change checked only by step 6 is a
   change to untested code.
