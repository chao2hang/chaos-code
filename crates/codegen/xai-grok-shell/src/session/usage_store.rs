//! Persistent aggregate token/cost storage across sessions.
//!
//! Stores per-session, per-model usage snapshots so the UI can report total
//! token consumption since the user started using Chaos, broken down by
//! model. Data is accumulated incrementally: every time a session's usage
//! ledger is read, the latest snapshot is upserted into the store.
//!
//! The backing database lives at `<grok_home>/sessions/usage.sqlite`.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use rusqlite::{Connection, OptionalExtension, params};
use xai_sqlite_journal::JournalMode;

use crate::extensions::notification::{PromptUsage, PromptUsageModel};

/// Bump when the schema changes incompatibly.
const SCHEMA_VERSION: &str = "2";

/// Per-model usage row as stored in the database.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct StoredModelUsage {
    pub session_id: String,
    pub model: String,
    pub recorded_at_unix: i64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_read_tokens: u64,
    pub reasoning_tokens: u64,
    pub model_calls: u64,
    pub api_duration_ms: u64,
    pub cost_usd_ticks: Option<i64>,
    pub cost_is_partial: bool,
}

/// Aggregate usage across all sessions, plus bookkeeping flags.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AggregateUsage {
    pub totals: PromptUsageModel,
    pub by_model: IndexMap<String, PromptUsageModel>,
    pub num_turns: u64,
    pub usage_is_incomplete: bool,
    /// Number of distinct sessions that contributed to the aggregate.
    pub session_count: u64,
}

/// SQLite-backed usage store.
pub(crate) struct UsageStore {
    db: Connection,
}

impl UsageStore {
    /// Open (or create) the aggregate usage store at the default location
    /// under `grok_home`.
    pub(crate) fn open_default() -> Result<Self, rusqlite::Error> {
        let path = default_db_path();
        Self::open_or_create(&path)
    }

    /// Open (or create) the store at an explicit path. Tests use this.
    pub(crate) fn open_or_create(db_path: &Path) -> Result<Self, rusqlite::Error> {
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let journal_mode = JournalMode::for_db_path(db_path);
        let db = journal_mode.open(db_path)?;

        let stored_version: Option<String> = db
            .query_row(
                "SELECT value FROM meta WHERE key = 'usage_schema_version'",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap_or(None);

        let current: u64 = SCHEMA_VERSION.parse().expect("SCHEMA_VERSION is digits");
        let stored: Option<u64> = stored_version.as_deref().map(|v| v.parse().unwrap_or(0));

        if stored.is_some_and(|s| s < current) {
            db.execute_batch(
                "BEGIN;
                DROP TABLE IF EXISTS session_model_usage;
                DROP TABLE IF EXISTS meta;
                COMMIT;",
            )?;
        }

        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS session_model_usage (
                session_id TEXT NOT NULL,
                model TEXT NOT NULL,
                recorded_at INTEGER NOT NULL,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cached_read_tokens INTEGER NOT NULL DEFAULT 0,
                reasoning_tokens INTEGER NOT NULL DEFAULT 0,
                model_calls INTEGER NOT NULL DEFAULT 0,
                api_duration_ms INTEGER NOT NULL DEFAULT 0,
                cost_usd_ticks INTEGER,
                cost_is_partial INTEGER NOT NULL DEFAULT 0,
                usage_is_incomplete INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (session_id, model)
            );

            CREATE INDEX IF NOT EXISTS idx_session_model_usage_model
                ON session_model_usage(model);
            CREATE INDEX IF NOT EXISTS idx_session_model_usage_recorded_at
                ON session_model_usage(recorded_at);
            ",
        )?;

        if stored != Some(current) {
            db.execute(
                "INSERT OR REPLACE INTO meta(key, value) VALUES ('usage_schema_version', ?1)",
                params![SCHEMA_VERSION],
            )?;
        }

        Ok(Self { db })
    }

    /// Store (or replace) the per-model usage snapshot for a session.
    ///
    /// Callers should pass the latest [`PromptUsage`] returned by the
    /// session ledger. The aggregate is recomputed on read, so stale rows
    /// for this session are overwritten in place.
    pub(crate) fn record_session_usage(
        &self,
        session_id: &str,
        usage: &PromptUsage,
    ) -> Result<(), rusqlite::Error> {
        let recorded_at = chrono::Utc::now().timestamp();

        // Chaos: the ledger snapshot is authoritative (not incremental),
        // so purge any stale rows first. Otherwise, a session that first
        // logged spend under a wire sentinel like `"auto"` (Volcengine Ark)
        // and later got relabeled to the configured id (`ark-code-latest`)
        // would double-count in the aggregate view.
        self.db.execute(
            "DELETE FROM session_model_usage WHERE session_id = ?1",
            params![session_id],
        )?;

        // For the session-level per-model rows we use the breakdown carried
        // by `usage.model_usage`. When only one model is present we still
        // store that single row so the aggregate-by-model query is uniform.
        if usage.model_usage.is_empty() {
            // Edge case: a session with no per-model breakdown but non-zero
            // totals. Fall back to a single "unknown" model row so the spend
            // is not lost from the aggregate.
            self.upsert_session_model_usage(
                session_id,
                "unknown",
                recorded_at,
                usage,
                &usage.totals,
            )?;
        } else {
            for (model, m) in &usage.model_usage {
                self.upsert_session_model_usage(session_id, model, recorded_at, usage, m)?;
            }
        }

        Ok(())
    }

    fn upsert_session_model_usage(
        &self,
        session_id: &str,
        model: &str,
        recorded_at: i64,
        usage: &PromptUsage,
        m: &PromptUsageModel,
    ) -> Result<(), rusqlite::Error> {
        self.db.execute(
            "INSERT INTO session_model_usage(
                session_id, model, recorded_at, input_tokens, output_tokens,
                cached_read_tokens, reasoning_tokens, model_calls, api_duration_ms,
                cost_usd_ticks, cost_is_partial, usage_is_incomplete
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(session_id, model) DO UPDATE SET
                recorded_at = excluded.recorded_at,
                input_tokens = excluded.input_tokens,
                output_tokens = excluded.output_tokens,
                cached_read_tokens = excluded.cached_read_tokens,
                reasoning_tokens = excluded.reasoning_tokens,
                model_calls = excluded.model_calls,
                api_duration_ms = excluded.api_duration_ms,
                cost_usd_ticks = excluded.cost_usd_ticks,
                cost_is_partial = excluded.cost_is_partial,
                usage_is_incomplete = excluded.usage_is_incomplete",
            params![
                session_id,
                model,
                recorded_at,
                m.input_tokens as i64,
                m.output_tokens as i64,
                m.cached_read_tokens as i64,
                m.reasoning_tokens as i64,
                m.model_calls as i64,
                m.api_duration_ms as i64,
                m.cost_usd_ticks,
                m.cost_is_partial as i32,
                usage.usage_is_incomplete as i32,
            ],
        )?;

        Ok(())
    }

    /// Return aggregate usage across all stored sessions.
    pub(crate) fn aggregate_usage(&self) -> Result<AggregateUsage, rusqlite::Error> {
        let mut usage = AggregateUsage::default();

        // Totals.
        let row = self
            .db
            .query_row(
                "SELECT
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(cached_read_tokens), 0),
                    COALESCE(SUM(reasoning_tokens), 0),
                    COALESCE(SUM(model_calls), 0),
                    COALESCE(SUM(api_duration_ms), 0),
                    COALESCE(SUM(cost_usd_ticks), 0),
                    MAX(cost_is_partial),
                    MAX(usage_is_incomplete),
                    COUNT(DISTINCT session_id)
                 FROM session_model_usage",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? as u64,
                        row.get::<_, i64>(1)? as u64,
                        row.get::<_, i64>(2)? as u64,
                        row.get::<_, i64>(3)? as u64,
                        row.get::<_, i64>(4)? as u64,
                        row.get::<_, i64>(5)? as u64,
                        row.get::<_, Option<i64>>(6)?,
                        row.get::<_, Option<i32>>(7)?,
                        row.get::<_, Option<i32>>(8)?,
                        row.get::<_, i64>(9)? as u64,
                    ))
                },
            )
            .optional()?;

        if let Some((
            input_tokens,
            output_tokens,
            cached_read_tokens,
            reasoning_tokens,
            model_calls,
            api_duration_ms,
            cost_sum,
            cost_is_partial_flag,
            usage_is_incomplete_flag,
            session_count,
        )) = row
        {
            usage.totals = PromptUsageModel {
                input_tokens,
                output_tokens,
                total_tokens: input_tokens.saturating_add(output_tokens),
                cached_read_tokens,
                cache_creation_tokens: 0,
                reasoning_tokens,
                model_calls,
                api_duration_ms,
                cost_usd_ticks: cost_sum,
                cost_is_partial: cost_is_partial_flag.unwrap_or(0) != 0,
                cost_missing_calls: 0,
                decode_duration_ms: 0,
                decode_tokens_per_sec: None,
            };
            usage.usage_is_incomplete = usage_is_incomplete_flag.unwrap_or(0) != 0;
            usage.session_count = session_count;
        }

        // Per-model aggregates.
        let mut stmt = self.db.prepare(
            "SELECT
                model,
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(cached_read_tokens), 0),
                COALESCE(SUM(reasoning_tokens), 0),
                COALESCE(SUM(model_calls), 0),
                COALESCE(SUM(api_duration_ms), 0),
                COALESCE(SUM(cost_usd_ticks), 0),
                MAX(cost_is_partial),
                MAX(usage_is_incomplete)
             FROM session_model_usage
             GROUP BY model
             ORDER BY SUM(input_tokens + output_tokens) DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, i64>(3)? as u64,
                row.get::<_, i64>(4)? as u64,
                row.get::<_, i64>(5)? as u64,
                row.get::<_, i64>(6)? as u64,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, Option<i32>>(8)?,
                row.get::<_, Option<i32>>(9)?,
            ))
        })?;

        for row in rows {
            let (
                model,
                input_tokens,
                output_tokens,
                cached_read_tokens,
                reasoning_tokens,
                model_calls,
                api_duration_ms,
                cost_sum,
                cost_is_partial_flag,
                usage_is_incomplete_flag,
            ) = row?;
            usage.by_model.insert(
                model,
                PromptUsageModel {
                    input_tokens,
                    output_tokens,
                    total_tokens: input_tokens.saturating_add(output_tokens),
                    cached_read_tokens,
                    cache_creation_tokens: 0,
                    reasoning_tokens,
                    model_calls,
                    api_duration_ms,
                    cost_usd_ticks: cost_sum,
                    cost_is_partial: cost_is_partial_flag.unwrap_or(0) != 0
                        || usage_is_incomplete_flag.unwrap_or(0) != 0,
                    cost_missing_calls: 0,
                    decode_duration_ms: 0,
                    decode_tokens_per_sec: None,
                },
            );
        }

        Ok(usage)
    }

    /// Convert the aggregate into the public [`PromptUsage`] wire shape.
    pub(crate) fn aggregate_prompt_usage(&self) -> Result<PromptUsage, rusqlite::Error> {
        let aggregate = self.aggregate_usage()?;
        let mut usage = PromptUsage {
            totals: aggregate.totals,
            model_usage: aggregate.by_model,
            num_turns: aggregate.session_count,
            usage_is_incomplete: aggregate.usage_is_incomplete,
        };
        usage.scrub_untrustworthy_costs();
        Ok(usage)
    }
}

/// Default path for the aggregate usage database: `<grok_home>/sessions/usage.sqlite`.
///
/// `GROK_USAGE_STORE_PATH` overrides this for tests or advanced setups.
pub(crate) fn default_db_path() -> PathBuf {
    if let Ok(path) = std::env::var("GROK_USAGE_STORE_PATH") {
        return PathBuf::from(path);
    }
    crate::util::grok_home::grok_home()
        .join("sessions")
        .join("usage.sqlite")
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn model(
        input: u64,
        output: u64,
        calls: u64,
        ticks: Option<i64>,
        partial: bool,
    ) -> PromptUsageModel {
        PromptUsageModel {
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            cached_read_tokens: input / 2,
            cache_creation_tokens: 0,
            reasoning_tokens: output / 4,
            model_calls: calls,
            api_duration_ms: 1_000,
            decode_duration_ms: 0,
            decode_tokens_per_sec: None,
            cost_usd_ticks: ticks,
            cost_is_partial: partial,
            cost_missing_calls: 0,
        }
    }

    fn usage_with_models(models: &[(&str, PromptUsageModel)], incomplete: bool) -> PromptUsage {
        let mut model_usage = IndexMap::new();
        let mut totals = PromptUsageModel::default();
        for (name, m) in models {
            totals.input_tokens += m.input_tokens;
            totals.output_tokens += m.output_tokens;
            totals.total_tokens += m.total_tokens;
            totals.cached_read_tokens += m.cached_read_tokens;
            totals.reasoning_tokens += m.reasoning_tokens;
            totals.model_calls += m.model_calls;
            totals.api_duration_ms += m.api_duration_ms;
            totals.cost_usd_ticks = totals
                .cost_usd_ticks
                .map(|c| c + m.cost_usd_ticks.unwrap_or(0))
                .or(m.cost_usd_ticks);
            model_usage.insert((*name).to_string(), m.clone());
        }
        PromptUsage {
            totals,
            model_usage,
            num_turns: 1,
            usage_is_incomplete: incomplete,
        }
    }

    #[test]
    fn empty_store_returns_zero_aggregate() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UsageStore::open_or_create(&tmp.path().join("usage.sqlite")).unwrap();
        let agg = store.aggregate_prompt_usage().unwrap();
        assert_eq!(agg.totals.total_tokens, 0);
        assert!(agg.model_usage.is_empty());
        assert_eq!(agg.num_turns, 0);
    }

    #[test]
    fn store_aggregates_across_sessions_and_models() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UsageStore::open_or_create(&tmp.path().join("usage.sqlite")).unwrap();

        let s1 = usage_with_models(
            &[
                ("grok-4", model(1_000, 100, 2, Some(50), false)),
                ("grok-3", model(500, 50, 1, Some(20), false)),
            ],
            false,
        );
        let s2 = usage_with_models(
            &[
                ("grok-4", model(2_000, 200, 3, Some(100), false)),
                ("grok-4-fast", model(300, 30, 1, Some(10), false)),
            ],
            false,
        );

        store.record_session_usage("session-1", &s1).unwrap();
        store.record_session_usage("session-2", &s2).unwrap();

        let agg = store.aggregate_prompt_usage().unwrap();
        assert_eq!(agg.totals.input_tokens, 3_800);
        assert_eq!(agg.totals.output_tokens, 380);
        assert_eq!(agg.totals.model_calls, 7);
        assert_eq!(agg.num_turns, 2); // session_count
        assert_eq!(agg.model_usage.len(), 3);
        assert_eq!(agg.model_usage["grok-4"].input_tokens, 3_000);
        assert_eq!(agg.model_usage["grok-4-fast"].input_tokens, 300);
    }

    #[test]
    fn recording_overwrites_same_session_model() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UsageStore::open_or_create(&tmp.path().join("usage.sqlite")).unwrap();

        let first = usage_with_models(&[("grok-4", model(1_000, 100, 1, Some(10), false))], false);
        let second = usage_with_models(&[("grok-4", model(2_000, 200, 2, Some(20), false))], false);

        store.record_session_usage("session-a", &first).unwrap();
        store.record_session_usage("session-a", &second).unwrap();

        let agg = store.aggregate_prompt_usage().unwrap();
        assert_eq!(agg.totals.input_tokens, 2_000);
        assert_eq!(agg.totals.model_calls, 2);
    }

    #[test]
    fn incomplete_flag_scrubs_costs() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UsageStore::open_or_create(&tmp.path().join("usage.sqlite")).unwrap();

        let usage = usage_with_models(&[("grok-4", model(1_000, 100, 1, Some(10), false))], true);
        store.record_session_usage("session-x", &usage).unwrap();

        let agg = store.aggregate_prompt_usage().unwrap();
        assert!(agg.usage_is_incomplete);
        assert!(agg.totals.cost_usd_ticks.is_none());
    }

    /// Chaos: when a session first records under a wire sentinel like
    /// `"auto"` (Volcengine Ark) and later gets relabeled to the configured
    /// model (`ark-code-latest`), the second write must fully replace the
    /// first — no ghost `auto` row left behind.
    #[test]
    fn recording_replaces_prior_session_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UsageStore::open_or_create(&tmp.path().join("usage.sqlite")).unwrap();

        let sentinel = usage_with_models(&[("auto", model(1_000, 100, 1, Some(10), false))], false);
        store
            .record_session_usage("session-ark", &sentinel)
            .unwrap();

        let relabeled = usage_with_models(
            &[("ark-code-latest", model(1_000, 100, 1, Some(10), false))],
            false,
        );
        store
            .record_session_usage("session-ark", &relabeled)
            .unwrap();

        let agg = store.aggregate_prompt_usage().unwrap();
        assert_eq!(agg.model_usage.len(), 1);
        assert!(agg.model_usage.contains_key("ark-code-latest"));
        assert!(!agg.model_usage.contains_key("auto"));
        // Totals must match the second write, not the sum of both.
        assert_eq!(agg.totals.input_tokens, 1_000);
        assert_eq!(agg.totals.output_tokens, 100);
        assert_eq!(agg.totals.model_calls, 1);
    }
}
