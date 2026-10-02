# Contributing

This repository does **not** accept external pull requests or unsolicited
patches.

SpaceXAI develops this software internally. The public tree is published for
source transparency and local builds under the terms of the Apache License,
Version 2.0 (see [`LICENSE`](LICENSE)).

## Local setup

Install the repository hooks once per clone:

```sh
./scripts/install-hooks.sh
```

This sets `core.hooksPath` to `scripts/hooks`, enabling a pre-commit scan that
refuses credential material and local-machine artifacts (environment dumps,
`.pem`/`.env` files, literal Windows temp paths). The same scan runs in CI, so
skipping the hook only moves the failure later. A commit was once pushed to the
public remote carrying live API keys; the hook exists so that cannot recur.

For an intentional fixture containing a fake key, annotate the line with a
`secret-scan:allow` comment (on the line or the one above it) rather than
disabling the scan.

New Rust tests should use `#[ignore = "reason; review YYYY-MM"]` only when an
external service or platform is genuinely required. Bare `#[ignore]` entries
are grandfathered by `scripts/ci/ignored-tests-baseline.tsv`; additions and
removals both require a reviewed baseline change. The quarterly audit still
requires a clear reason and review date.

## Low-memory builds

This repository's Rust workspace has a large build graph and can use substantial
memory, disk space, and filesystem metadata during compilation. On WSL2 or other
low-memory machines, limit Cargo parallelism rather than running several Cargo
builds at once:

```sh
CARGO_BUILD_JOBS=4 cargo check --workspace --all-targets --locked
CARGO_BUILD_JOBS=4 cargo test --workspace --locked
```

Avoid running release and check builds concurrently on a memory-constrained WSL2
checkout. The `target/` directory can grow large; inspect its size before a full
rebuild, and remove only disposable `target/debug` or incremental artifacts when
space is needed. Preserve source, caches outside `target/`, and user data.

The observed WSL2 9P shutdown incident and its environment-specific evidence are
recorded in [`docs/known-issues/wsl-p9io-crash-20260728.md`](docs/known-issues/wsl-p9io-crash-20260728.md).

## Clean-container verification

A passing local run is not proof that the documented steps work: a warm
`target/`, a globally installed `protoc`, a hand-installed toolchain or an OS
trust store that already has the right roots can each hide a step that a fresh
clone would need. `scripts/verify-in-docker.sh` builds
[`docker/verify.Dockerfile`](docker/verify.Dockerfile) and runs the same command
list as the `rust` job in `.github/workflows/ci.yml` inside it:

```sh
scripts/verify-in-docker.sh          # fmt, guard scripts, check, clippy
scripts/verify-in-docker.sh --full   # the above plus cargo test --workspace
scripts/verify-in-docker.sh --shell  # same image, interactive
```

The image installs its toolchain through rustup from `rust-toolchain.toml` and
asserts `rustc -V` against that pin at build time, so the pin stays the only
source of truth and a silent fallback to another compiler fails the build.
Cargo's registry and `target/` live in named volumes rather than the bind mount,
so a root-owned build tree cannot break the host build afterwards.

Behind a registry mirror, point the build at your own base image:

```sh
BASE_IMAGE=your-mirror.example.com/library/debian:bookworm-slim scripts/verify-in-docker.sh
```

This container covers the Linux gates only. It is not platform evidence: a Linux
container cannot run the macOS or Windows code paths, and it does not exercise
signing, installers, or a real TLS-terminating deployment.

## Platform-specific checks

Several crates select behaviour by `cfg(unix)` / `cfg(windows)` /
`cfg(target_os = ...)`: the TTY stderr-handle handling, the sandbox's seccomp and
namespace paths versus the Windows Job Object paths, PTY descendant teardown, the
updater's per-OS installer hint, and child-process spawning. A green Linux run
says nothing about those paths, so run the target machine's own entry point:

```sh
# macOS / Linux
scripts/test-platform.sh 2>&1 | tee "platform-test-$(uname -s).log"
```

```powershell
# Windows (PowerShell)
./scripts/test-platform.ps1 *>&1 | Tee-Object platform-test-windows.log
```

Both print the toolchain/OS report first and then run the same crate set the
`platform-tests` CI job uses (`macos-14` and `windows-latest`), and both accept
crate names to narrow the run. Keep the log: it is the evidence a platform row in
`TODO.md` needs, and a Linux log never substitutes for it.

When a run fails, send the whole log rather than the last few lines. The header
the scripts print (OS build, `rustc -Vv`, logical CPU count, `RUST_MIN_STACK`,
commit, and a `working tree : dirty` marker) is what tells a reader whether the
failure is the checked-in code or a local edit, so a truncated tail usually
cannot be diagnosed.

### Shell script portability

`scripts/test-platform.sh` and the other scripts under `scripts/` run on the
contributor's machine, not only on the Linux CI runner. macOS still ships
bash 3.2 and BSD userland, so `mapfile`, `readarray`, `declare -A`, `${var^^}`,
GNU `sed -i`, `nproc`, `readlink -f`, `grep -P`/`--include` and `timeout` all
fail there with a bare "command not found" or a silently half-done file.
`scripts/ci/check-script-portability.py` rejects those constructs, and it has no
allow list on purpose: if a rule fires, the script gets rewritten.
`scripts/ci/test-script-portability.py` injects one violation per rule and
asserts the check still exits 1, because a scanner that stopped matching looks
exactly like a repository that was fixed.

## Remote workspace sessions

Two binaries carry the remote-workspace transport
(`crates/codegen/chaos-engine/src/remote/`):

```sh
# On the host that owns the workspace. It sees exactly one directory.
chaos-remote-server --workspace ~/src/beeper --tcp 127.0.0.1:7788 \
  --token-file ~/.chaos/remote-tokens --capability tool-execution --allow cargo

# On the machine doing the work, through a tunnel whose local end is loopback.
chaos-remote --tcp 127.0.0.1:17788 --token-file ~/.chaos/remote-tokens list src
chaos-remote --tcp 127.0.0.1:17788 --token-file ~/.chaos/remote-tokens \
  exec --timeout 600 -- cargo test -p beeper
chaos-remote --tcp 127.0.0.1:17788 --token-file ~/.chaos/remote-tokens \
  install 0.4.0 --from target/release/chaos-remote-server

# Through a tunnel that is still coming up, with a bound on being stuck.
chaos-remote --tcp 127.0.0.1:17788 --token-file ~/.chaos/remote-tokens \
  --connect-wait 30 --reply-timeout 120 ping
```

A service on the remote host can be reached from here the way `ssh -L` reaches it.
The server decides which services are reachable at all, by address and port, and
naming one is the only way to enable forwarding:

```sh
# On the remote host: forward to the review app it can already reach.
chaos-remote-server --workspace ~/src/beeper --tcp 127.0.0.1:7788 \
  --token-file ~/.chaos/remote-tokens --allow-forward-to 127.0.0.1:3000

# On the machine doing the work: a local loopback port, gone when the grant is.
chaos-remote --tcp 127.0.0.1:17788 --token-file ~/.chaos/remote-tokens \
  forward --to 127.0.0.1:3000 --listen 0
```

Three properties are deliberate and are what the tests around this code exist
to hold:

- **Loopback only.** Both `--tcp` forms refuse a routable address at parse time.
  The tunnel carries the trust; the transport does not attempt TLS, and a
  `ssh -L`-style tunnel is assumed rather than implemented here. The local end of a
  forward is held to the same rule, because a forwarded service on a routable
  interface is a published service.
- **One-time credentials.** Each credential opens exactly one session. Reuse is
  refused as reuse, a credential past its TTL is refused as expired, and the
  client removes the credential it spent from the file it was given.
- **Nothing is faked.** `--capability interactive-pty` and `detached-agent` are
  refused with a reason, and `exec` runs only programs named by `--allow`.
- **Forwarding is the operator's call, not the session's.** A server with no
  `--allow-forward-to` has nowhere a forward may go and refuses to start when asked
  to advertise the capability anyway. A forward is authorised by a *ticket* kept in
  its own vault, bound to one `host:port`, valid for a bounded number of
  connections, and withdrawn when the session that asked for it closes; it opens no
  session and reads nothing. Remote forwarding (`ssh -R`, the server listening on
  the client's behalf) is not implemented and is refused by name.
- **Both waits are bounded, and they are different waits.** `--connect-wait` re-dials
  for as long as it is told — the delay doubles up to 2s — and repeats *only* the dial,
  so waiting for a tunnel that is still coming up can never burn a credential.
  `--reply-timeout` bounds a reply instead: the handshake's and each request's. When a
  request's reply does not arrive, the session ends rather than pauses
  (`RemoteWorkspace::state()` reports `Abandoned`), because a late reply would answer
  the wrong question and a request cannot be unsent. Nothing reconnects by itself, on
  purpose: a credential is one-time, so reconnecting means a new session and a new
  credential, which is a decision for whoever runs the command rather than a background
  retry. A credential also leaves the token file the moment it is sent — including on a
  run that dies in the handshake — so a failed run cannot poison the next one.

`scripts/remote-acceptance-in-docker.sh` is the gate for all of the above. It
builds the artifacts here, then puts the server in a stock Debian container that
has never seen this repository, reaches it through a `socat` tunnel, and checks
reading, searching, writing, `git diff`, tool execution, path-escape refusals,
credential handling, a full disk, an artifact on a `noexec` filesystem, a dropped
connection, and both clocks: a late reply, a tunnel that appears late, and a peer
that accepts the connection and never speaks. Forwarding is checked the same way: a
`python3 -m http.server` on the remote host is fetched through a forward from the
other container and the digest compared against the file on that host's disk, an
unlisted target is refused although something really is listening on it, a grant of
two connections is seen to end the forward and release the local port by itself, and
a hand-written RFC 6455 service on the remote host completes a WebSocket handshake
through the tunnel and returns a file the developer container cannot read.

```sh
scripts/remote-acceptance-in-docker.sh 2>&1 | tee "remote-acceptance-$(date +%F).log"
```

A run leaves its transcript in the log you tee it to;
[`docs/verification/remote-acceptance-linux-2026-10-02.log`](docs/verification/remote-acceptance-linux-2026-10-02.log)
is a kept example. The lab is Linux-only and there is no SSH transport in this
build, so it is not evidence about SSH host-key handling or about a macOS or
Windows remote host.

## Serving the Web host behind TLS

`chaos-web` binds `127.0.0.1` and stays that way: a TLS terminator in front of it
is the deployment, not a `--bind 0.0.0.0` flag. Because the proxy dials loopback,
it has to share the backend's network namespace (a sidecar pod, or
`docker run --network container:<web>`), and it forwards headers the backend has
never seen from a local browser:

- `Host: chaos.example.com` — refused unless you declare it, because the Host
  check is what stops DNS rebinding against a loopback server.
- `Origin: https://chaos.example.com:8443` — an `https` origin can never be
  same-origin with the `http` request the backend actually receives, so it is
  accepted only for a declared name.

`CHAOS_WEB_PUBLIC_ORIGIN=https://<name>[:<port>]` is that declaration. It must be
a bare `https` origin — no path, query, fragment or credentials — and it requires
`CHAOS_WEB_TOKEN`: a server that answers a public name is never anonymous. The
proxy must then set `X-Forwarded-Proto: https`; without that claim the `https`
origin is refused, so a plaintext hop cannot present itself as the TLS deployment.
`X-Forwarded-For` is not used for any authorization decision.

A proxy that "fixes" the 401 by rewriting `Host` to the upstream
(`proxy_set_header Host $proxy_host;`) makes plain requests work while the
WebSocket keeps failing, because a WebSocket handshake always carries an `Origin`.
Every refusal says which rule fired in its JSON body — `host_not_allowed`,
`origin_not_allowed`, `origin_requires_forwarded_proto` or `credential_required` —
so the operator edits the thing that is actually wrong.

```sh
scripts/web-deployment-in-docker.sh 2>&1 | tee "web-deployment-$(date +%F).log"
```

That lab runs the built binary behind stock nginx with a lab-issued certificate
chain and checks the handshake, the credential, the declared-name rules, a real
`wss:` session, a credential rotation, Safe Web Mode through the proxy, and that
the backend is unreachable from anywhere but the proxy. It is a deployment-shape
check on Linux; it is not a certificate-authority, CDN or multi-tenant review.

## Auto-update tests

`cargo test -p xai-grok-update` runs the whole updater against fake feeds it starts
itself, so it needs no network and no GitHub token. Two things about it surprise people:

- `tests/test_update_feed_e2e.rs` drives the shipped `run_update`, the same entry point
  `chaos update` reaches, with `CHAOS_GH_API_BASE` pointed at a wiremock server and
  `CHAOS_GH_DOWNLOAD_BASE` at a raw HTTP server that can cut a transfer mid-body
  (`tests/common/artifact_server.rs`). The artifacts it installs are `/bin/sh` scripts
  that append to a marker file, so "was this build actually executed" is a fact about a
  file rather than about a log line. Each test calls `assert_feed_is_loopback()` before
  updating: an earlier draft released its mock guard early, cleared the override, and
  installed a real release off github.com. If you touch that file, keep that assertion.
- The tests mutate process environment (`GROK_HOME`, the two feed overrides,
  `CHAOS_REQUIRE_SIG`), so they are `#[serial]`. `tests/common/mod.rs::reset_home()`
  removes `CHAOS_GH_API_BASE` and `CHAOS_GH_DOWNLOAD_BASE` on purpose — a guard dropped
  late would otherwise re-point the next test at a dead server.

`chaos update` verifies a `.sig` sidecar against a public key that is fixed at build
time, so no in-tree test can produce a signature its own build accepts. What the suite
can prove is the refusal half; the accept half has to run against a real published
release, which is what `scripts/verify-release-signature.sh` does.

## Release versioning

One number is the release version, and it lives in
`crates/codegen/xai-grok-pager/npm/chaos/package.json`. `release.yml`'s
`resolve-version` step reads it when the dispatch form left the version blank, so that
file decides the tag, the `GROK_VERSION` stamped into the binaries, and the npm versions
`scripts/ci/stamp-npm-version.mjs` writes.

Everything a user can hold has to carry that same number:

- the six `npm/chaos-<platform>/package.json` files and the meta package's
  `optionalDependencies` pins, because a pin npm cannot resolve makes
  `npm install chaos-code` succeed with no binary inside;
- `xai-grok-pager` and `xai-grok-pager-bin`, because those two crates build the binary
  whose `chaos --version` output the auto-updater compares against a release feed;
- a `## <version>` section in `CHANGELOG.md`, so a tag is never cut for a version this
  repository does not describe.

`scripts/ci/check-version-lockstep.py` checks all of that and runs in CI.

Four crates deliberately do **not** follow the release version, and the check fails if
this list and that paragraph drift apart: `xai-grok-web` and `xai-grok-desktop` are
versioned by their own bundles, `chaos-engine` is versioned by the protocol it speaks,
and `xai-grok-update` still carries the numbering it inherited from upstream. None of
them publishes an artifact of its own, so nothing a user installs reads their version.
Bumping one of them to match the release version is not a fix and the check will not ask
for it.

With `--published` the same script also asks npm registry what it serves for the seven
names. That half is opt-in because it needs network; run it before a release. It reports a
name whose published versions are merely behind the repository as a note (normal between
releases), and fails on a name npm holds as a security placeholder, which publishing does
not fix — see the npm row under `## Running the installers the way a user does`.

## Checking a published release

`scripts/verify-release-signature.sh` downloads the artifact, the `.sig` sidecar and
`SHA256SUMS` from a published release, recomputes the digest, and runs
`xai_grok_update::signature::verify_file` — the shipped verifier — over the real bytes.

```sh
scripts/verify-release-signature.sh                 # latest release, host platform
scripts/verify-release-signature.sh --tag v0.4.2    # a specific release
scripts/verify-release-signature.sh --tag v0.4.2 --all   # every artifact in SHA256SUMS
```

It needs `curl` and `gh` (the trusted key is a repository *variable*, so `gh variable
get` reads it in plain text; nothing secret is involved) and it downloads a
hundred-ish megabyte artifact into a temp directory. It also checks two things that make
the first check worth anything: a one-byte corruption of the same artifact must be
refused, and a build given no key must refuse rather than accept.

Because the key is baked in by `option_env!`, `crates/codegen/xai-grok-update/build.rs`
declares `cargo:rerun-if-env-changed=CHAOS_SIGNING_PUBLIC_KEY`. Without it, rebuilding
after changing that variable relinks nothing and silently keeps whichever key the
previous build embedded. The script's third check is what notices if that directive is
ever removed.

Key generation, the exact secret and variable names, who can sign, and what rotating the
pair does to already-installed binaries is in [docs/release-signing.md](docs/release-signing.md).

## Running the installers the way a user does

The release path has three halves, and the last two are only checkable by installing.

`scripts/install-sh-in-docker.sh` copies the working-tree `install.sh` into a stock
`debian:bookworm-slim` container whose only packages are `curl`, `python3` and
`python3-cryptography` — no cargo, no repo, no prior install — and runs it against the
real published release. It asserts the parts that are easy to fake: that the checksum and
signature checks both reported OK rather than silently skipped, that `bin/chaos` is a
*relative* symlink (so it survives a bind mount that remaps `$HOME`), that the stored
artifact uses the `linux-x86_64` name `chaos update` expects rather than the
`chaos-linux-x64` asset name, and that a second run is a no-op. Two controls keep the
signature line honest: a valid-but-foreign key must refuse and leave the installed
artifact byte-identical, and a present-but-blank key must refuse *before* the download
starts.

A feed that only ever serves correct artifacts cannot say what the checks do when the
bytes are wrong, and a refusal you cannot trigger looks exactly like a check that was
skipped. `scripts/install-integrity-in-docker.sh` is the negative half, and it needs
neither the network nor the signing key. It builds its own release -- an artifact, its
`SHA256SUMS` row, and an Ed25519 `.sig` over the artifact bytes -- and serves it through the
ghproxy-style mirror path `install.sh` already supports
(`${CHAOS_GITHUB_MIRROR}/https://github.com/...`), so the code under test is the real
download path with only its origin replaced. The container runs with `--network none`:
loopback works, DNS resolves nothing, and the run asserts that `github.com` is unreachable
before it installs anything. Scenarios: a tampered artifact; `SHA256SUMS` *recomputed* to
match the tampered bytes so that only the signature stands in the way; a missing sidecar;
a valid key that is not ours; a blank key (which must refuse with zero requests logged);
a manifest with no row for this asset; a manifest served as a 200-with-HTML error page; an
empty download. Every refusal is checked twice -- for the reason and for `bin/chaos` being
absent -- and the two documented escape hatches are measured rather than trusted: skipping
the checksum alone still leaves the signature refusing the tampered artifact, while
skipping both installs it, which is what "you are then trusting the download" means. The
fixture, the mirror and the request-log assertion live in `scripts/ci/release-integrity-{fixture,serve,request-log}.py`
so that the Windows installer below is offered the *same* release rather than a
hand-written lookalike of it.

`scripts/install-integrity-powershell.sh` runs that other installer, `install.ps1`, for
real: pwsh on Linux, no Windows and no docker, against the shared fixture. It re-execs
itself inside `unshare -rn`, so the run has no route but loopback -- a trap worth knowing
about, because a fresh namespace has loopback *down*, and binding `127.0.0.1` succeeds
while every connect is refused. It covers the download-and-verify path the same way the
shell lab does: install, then seven refusals each checked for the reason and for nothing
landing in `~/.chaos/bin`, then the two hatches. It does not cover the three things a real
Windows box adds, and says so in its own header: which asset name
`[RuntimeInformation]::OSArchitecture` would ask for, running a PE binary, and the
registry `PATH` write (every run passes `-NoPath`). So the honest remaining claim is
narrow: the Windows installer is now measured to put the *right bytes* on disk and to
refuse the wrong ones, and still unmeasured on whether Windows will execute them or put
them on `PATH`.

Building it found two things. `install.ps1` refuses any artifact under 1 MiB *before*
hashing it; `install.sh` had no such floor and would happily hash a truncated body and
report whatever the checksum said; `install.bat` used the same 1 MiB number only to decide
whether to sniff for HTML, so a short non-HTML body fell through to `certutil` and came
back looking like a checksum mismatch. All three now refuse anything under 1 MiB up front,
and a check in the shell lab reads all three files and fails if those numbers drift apart.
And both script installers had
the same reporting defect, found from opposite sides: when every candidate fails, only the
*last* candidate's reason was shown, so a mirror that answers 200 with an HTML error page
-- or a fixture that answers 404 -- got blamed on whichever public mirror's DNS failed
last. Both now print up to four distinct reasons as `why:` lines. Deleting the loop from
`install.ps1` makes exactly three of the 30 PowerShell checks fail, which is the evidence
that those checks test something.

Neither lab used to run anywhere but the machine that wrote it. The `installer integrity
labs` job in CI runs both on every push -- no release, no signing key, and no network
needed at run time -- and asserts `pwsh --version` and `unshare -rn true` in its first
step, so a runner image that stops shipping one says so immediately instead of three
minutes into the docker step. Neither lab has a skip path: a missing `pwsh`, a missing
crypto binding, or a kernel that will not create a network namespace exits 2 with a named
reason, so the job cannot go green by measuring nothing.

`scripts/ci/check-powershell-syntax.py` is the cheap predecessor of that lab and still
runs on the `platform tests` legs, where a parse error would otherwise go unnoticed: `install.ps1`
once carried a stray closing brace for a stretch of history, so the documented
`irm .../install.ps1 | iex` died before downloading anything and nothing noticed, because
no Linux job executes it and those legs only build and test the Rust workspace.
It parses every tracked `*.ps1` with a real PowerShell and the legs run it with
`--require` so it cannot pass by finding no PowerShell.

`scripts/npm-install-in-docker.sh` does the same for `npm install -g chaos-code`, in a
stock `node` image, and asserts the container's own registry is
`https://registry.npmjs.org/` — this host's npm points at a mirror, so a host-side run
would have tested the mirror. Because npm filters optional dependencies by platform,
`npm install --os=win32 --cpu=x64` asks the Windows question from Linux: the two Windows
platform packages are pinned to a version that has never been published under those names,
npm skips what it cannot resolve, and the install reports success with no binary. The
script fails on exactly that and says which pins are missing.

Both scripts download a hundred-ish megabyte artifact; `install-sh-in-docker.sh
--skip-wrong-key` trades the foreign-key control for one fewer download.
`install-integrity-in-docker.sh` downloads nothing at all -- its artifact is a shell script
-- so it is the one to run when there is no network or no access to the release feed.

### Why the installers embed the signing key

`install.sh`, `install.ps1` and `install.bat` verify the signature and fail closed when
they cannot, and they used to take the key *only* from `CHAOS_SIGNING_PUBLIC_KEY` — which
no documented install command sets, so `curl -fsSL .../install.sh | bash` could not
complete an install on any machine. Each installer now ships the public key as
`DEFAULT_SIGNING_PUBLIC_KEY` / `$DefaultSigningPublicKey`, overridable by that environment
variable for anyone signing their own releases. Setting the variable to an empty string is
still an error, which keeps the fail-closed branch reachable and testable.

`scripts/ci/test-installer-signature-policy.py` checks that all three installers embed the
same 32-byte key and that the key plus the crypto prerequisites are resolved *before* the
download, since the artifact is 150 MB+. `install-sh-in-docker.sh` additionally checks the
embedded value against the `CHAOS_SIGNING_PUBLIC_KEY` repository variable.

## Upstream reconnaissance

`scripts/upstream-recon.sh` records how far the ported `SOURCE_REV` has fallen
behind `xai-org/grok-build` into `sync/recon/`. It is read-only: it queries
`git ls-remote` and the GitHub compare API, writes nothing outside `sync/recon/`,
and exits non-zero rather than writing a record when the network is unavailable.
Recording a gap is not a port — each upstream change still needs the curated-port
review in `sync/` before it touches ported source.

## Security reports

Please report security issues through the process described in
[`SECURITY.md`](SECURITY.md). Do not open a public issue for vulnerabilities.

## Licensing of this source

By downloading or using this source, you agree that your use is governed by
the Apache License, Version 2.0. No contributor license agreement is offered
because external contributions are not accepted.
