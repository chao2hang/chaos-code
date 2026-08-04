# Third-party notices

## catpaw-relay

- Source: <https://github.com/chao2hang/catpaw-relay>
- Reference commit: `333e891deb97b0bb93fd23160ce609eddb6543df`
- Upstream package author metadata: LO `<lo@catpaw-relay.local>`
- Other Git author metadata present before the reference commit: `chaos <chaos@local>`
- License declared by upstream `backend/Cargo.toml` and `README.md`: MIT
- The reference tree does not contain a standalone `LICENSE` file or an explicit copyright line

Portions of `xai-catpaw` are adapted from the upstream client core at that commit, including endpoint/header constants, the CatPaw wire-crypto behavior, QR normalization, model seeding/merging, cumulative Chat handling, Remote Agent wire types/event accumulation, and response-envelope behavior. `assets/key1.b64` and `assets/key2.b64` are copied protocol assets from `backend/assets/` at the same commit. Axum gateway code and the Svelte frontend were not copied.

The migrated code was reorganized into a standalone client crate, updated for workspace dependency versions and APIs, and extended with fail-closed encrypted account storage and additional tests.

### MIT License

Copyright (c) 2026 catpaw-relay authors and contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
