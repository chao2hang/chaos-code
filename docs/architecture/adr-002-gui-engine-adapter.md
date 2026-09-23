# ADR-002: engine adapter boundary

- Status: accepted for M0
- Date: 2026-09-24

The walking skeleton keeps the current headless implementation in
`xai-grok-pager` while `chaos-engine` proves the GUI protocol with a deterministic
mock. The next adapter will call the existing headless lifecycle behind a trait;
it will not copy session startup or create a second agent state machine.

The reason for deferring a physical move is risk: `headless.rs` is logically
headless but currently lives in the pager crate, which also owns terminal
rendering. Moving it before the protocol and regression boundary exists would
make the TUI migration and GUI delivery one inseparable change. The adapter can
be migrated later with the existing `--headless` tests as the compatibility gate.
