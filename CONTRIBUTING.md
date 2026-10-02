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

Three properties are deliberate and are what the tests around this code exist
to hold:

- **Loopback only.** Both `--tcp` forms refuse a routable address at parse time.
  The tunnel carries the trust; the transport does not attempt TLS, and a
  `ssh -L`-style tunnel is assumed rather than implemented here.
- **One-time credentials.** Each credential opens exactly one session. Reuse is
  refused as reuse, a credential past its TTL is refused as expired, and the
  client removes the credential it spent from the file it was given.
- **Nothing is faked.** `--capability interactive-pty`, `port-forward` and
  `detached-agent` are refused with a reason, and `exec` runs only programs
  named by `--allow`.
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
that accepts the connection and never speaks.

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
