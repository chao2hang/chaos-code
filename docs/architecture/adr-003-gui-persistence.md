# ADR-003: former GUI persistence boundary (superseded)

- Status: Superseded on 2026-11-14 when the Web/GUI product line was removed; retained as an implementation-history record only
- Date: 2026-09-24

This decision no longer describes a supported user-facing GUI. The Web/GUI client and
host were retired; current product scope is recorded in [ADR-007](adr-007-first-stable-product-scope.md).

M0 uses the engine's in-memory session store and transitional JSON snapshot to prove protocol behavior. The
canonical persistent boundary is now `chaos-engine::SqliteSessionStore`, backed by
`xai-sqlite-journal`; it has schema versioning, newer-schema rejection,
round-trip tests, and a forward upgrade fixture that creates a versioned backup
before updating metadata. A failed metadata update restores that backup; real
TUI import, multi-process/NFS and disk-full/interrupted-filesystem fixtures remain
before M2 persistence is production-complete.
The GUI does not write provider secrets
or API keys into its session snapshot; credentials remain in the existing
configuration/secrets boundary.

At the time this decision was active, a session resume request was an explicit
snapshot operation and missing or corrupt storage returned a typed recovery error.
`CHAOS_WEB_SQLITE` selected the canonical SQLite engine entry and `CHAOS_WEB_STATE`
was a transitional JSON fallback. The read-only `ImportTuiSession` seam validated
the real TUI `summary.json`/`updates.jsonl` shape under a host-configured root and
proved source bytes were unchanged. These GUI entry points and their open follow-up
items were retired with the Web/GUI product line; they are not current configuration
or roadmap instructions.
