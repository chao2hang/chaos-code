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

## Security reports

Please report security issues through the process described in
[`SECURITY.md`](SECURITY.md). Do not open a public issue for vulnerabilities.

## Licensing of this source

By downloading or using this source, you agree that your use is governed by
the Apache License, Version 2.0. No contributor license agreement is offered
because external contributions are not accepted.
