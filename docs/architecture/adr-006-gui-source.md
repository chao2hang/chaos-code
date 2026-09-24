# ADR-006: GUI source and maintenance model

- Status: accepted
- Date: 2026-09-24

Chaos Web/Desktop use a clean-room React and Rust implementation. No reference
product source, assets, fonts, icons, illustrations, or test identifiers are
copied. The Rust engine protocol is the single logical contract; the UI is
maintained in this repository and follows the normal review and release process.

If future work proposes borrowing code or assets, that work must first provide
a fixed upstream revision, license/NOTICE review, asset inventory, and explicit
replacement decision. Until then, no upstream UI synchronization is assumed.

This decision is reversible by replacing an independently scoped UI asset or
module; it does not change the existing Chaos CLI/TUI identifiers, config paths,
wire IDs, or environment variables.
