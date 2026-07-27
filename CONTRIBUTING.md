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

## Security reports

Please report security issues through the process described in
[`SECURITY.md`](SECURITY.md). Do not open a public issue for vulnerabilities.

## Licensing of this source

By downloading or using this source, you agree that your use is governed by
the Apache License, Version 2.0. No contributor license agreement is offered
because external contributions are not accepted.
