# ADR-003: GUI persistence boundary

- Status: accepted for M0 implementation
- Date: 2026-09-24

M0 uses the engine's in-memory session store and transitional JSON snapshot to prove protocol behavior. The
canonical persistent boundary is now `chaos-engine::SqliteSessionStore`, backed by
`xai-sqlite-journal`; it has schema versioning, newer-schema rejection and
round-trip tests. Production engine wiring, backup-before-migration, forward
migration, and read-only protection for newer schemas remain before M2 exit.
The GUI does not write provider secrets
or API keys into its session snapshot; credentials remain in the existing
configuration/secrets boundary.

A session resume request is an explicit snapshot operation. On process restart,
missing or corrupt storage returns a typed recovery error rather than silently
creating a new session. `CHAOS_WEB_SQLITE` now selects the canonical SQLite
engine entry; `CHAOS_WEB_STATE` remains a transitional JSON fallback. TUI
compatibility fixtures, multi-process/NFS coverage, and migration rollback are
required before SQLite persistence is called production-complete.
