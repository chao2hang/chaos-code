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
