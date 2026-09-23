# ADR-003: GUI persistence boundary

- Status: accepted for M0 implementation
- Date: 2026-09-24

M0 uses the engine's in-memory session store to prove protocol behavior. The
canonical persistent store will be the existing SQLite journal/store layer,
with schema versioning, backup-before-migration, forward migration, and
read-only protection for newer schemas. The GUI does not write provider secrets
or API keys into its session snapshot; credentials remain in the existing
configuration/secrets boundary.

A session resume request is an explicit snapshot operation. On process restart,
missing or corrupt storage returns a typed recovery error rather than silently
creating a new session. TUI compatibility fixtures will be required before the
persistent store is enabled.
