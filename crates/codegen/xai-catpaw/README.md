# xai-catpaw

Native Rust client core for the CatPaw protocol. This first phase is deliberately transport-focused and contains no Axum gateway, OpenAI-compatible relay, dashboard, or Svelte application.

## Included

- CatPaw endpoint constants and desktop-client header fingerprint
- RSA-OAEP with SHA-1 and AES-128-ECB with PKCS#7 wire envelopes
- Embedded XOR-obfuscated RSA assets required by the protocol
- QR start/poll and token-refresh wire normalization
- Seeded model catalog with live payload merging
- Cumulative Chat SSE and Remote Agent event accumulation
- Remote Agent create/continue/connect types and stream decoder
- Minimal async `Client` methods for QR login, refresh, models, Chat, and Remote Agent
- AES-256-GCM encrypted SQLite `AccountStore` with atomic least-recently-used selection

## Account storage security

`AccountStore::open(db_path, key_path)` requires separate database and key files. On Unix, both files are created with mode `0600`; an existing file with group/world permission bits or a symlink is rejected. Access and refresh tokens are always stored as authenticated AES-256-GCM ciphertext with independent random nonces and column-specific associated data. There is no plaintext, environment-variable, or in-memory fallback. Platforms where this implementation cannot guarantee owner-only files currently fail closed.

The caller is responsible for backing up the key and database together. Losing the key makes stored tokens unrecoverable.

## Protocol compatibility

The wire behavior and embedded assets were adapted from the public MIT-licensed [`chao2hang/catpaw-relay`](https://github.com/chao2hang/catpaw-relay) repository at commit [`333e891deb97b0bb93fd23160ce609eddb6543df`](https://github.com/chao2hang/catpaw-relay/commit/333e891deb97b0bb93fd23160ce609eddb6543df). See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for provenance and the retained MIT terms.

The AES-128-ECB construction is implemented solely for compatibility with the upstream protocol. New local secrets use authenticated AES-256-GCM instead.

## Verification

```sh
cargo fmt --check --package xai-catpaw
cargo check --all-targets --package xai-catpaw
cargo test --package xai-catpaw
```
