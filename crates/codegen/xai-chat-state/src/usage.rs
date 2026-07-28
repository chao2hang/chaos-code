//! Per-prompt and per-session billing ledgers (not serialized).
//!
//! `total_tokens()` is input + output: Responses wire `total` is live context
//! length. Compaction and other side calls never call `record_main_loop_call`.
//!
//! # Completeness ownership
//!
//! Wire incomplete is the OR of these stores (each has a distinct role):
//!
//! - **`UsageLedger.incomplete`** — durable on the bill snapshot. Set by nested
//!   subagent incomplete fold, drain timeout, true apply-miss, and
//!   `mark_usage_incomplete`. Monotonic for a ledger instance.
//! - **Sticky (`subagent_usage_not_applied` on the coordinator)** — pin-scoped
//!   **report** signal (session-only attribution or apply-miss report). Not a
//!   second token sink; does not stain ledgers by itself.
//! - **Foreground live IDs** — fold may still land; freeze drains ≤120s or fails
//!   closed. Cancel skips multi-second drain (actor-loop safety).
//! - **Background live** — never waits; prompt report incomplete immediately;
//!   spend still folds into the session ledger at completion (no session-ledger
//!   incomplete).
//!
//! Freeze and cancel share one outcome policy: ledger marks only on fail-closed;
//! sticky and background_live are report-level only.
//!
//! Projection (`PromptUsage`) never invents tokens; it only ORs completeness
//! and scrubs costs when partial or incomplete.

use indexmap::IndexMap;
use xai_grok_sampling_types::TokenUsage;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_read_tokens: u64,
    pub reasoning_tokens: u64,
    pub model_calls: u64,
    pub api_duration_ms: u64,
    /// 解码时长累计 (ms)：模型总耗时减去首字延迟，等于 Σ(model_elapsed − ttft)。
    /// 用于计算「稳态解码速率」（`output_tokens × 1000 / decode_duration_ms`）；
    /// 只保存可加性的原始量，展示层现算比率，避免折叠破坏。0 表示未采样。
    pub decode_duration_ms: u64,
    /// USD ticks (1e10 per USD). Absent when no call reported cost.
    pub cost_usd_ticks: Option<i64>,
    pub cost_missing_calls: u64,
}

impl UsageTotals {
    fn from_call(
        model_id: &str,
        usage: &TokenUsage,
        api_duration_ms: Option<u64>,
        decode_duration_ms: Option<u64>,
        cost_usd_ticks: Option<i64>,
    ) -> Self {
        let server_cost = xai_grok_sampling_types::reported_cost_ticks(cost_usd_ticks);
        // 服务器未报告费用时，尝试本地定价表估算（支持 BYOK 模型）
        let cost_usd_ticks = server_cost.or_else(|| {
            xai_token_estimation::pricing::estimate_cost_usd_ticks(
                model_id,
                u64::from(usage.prompt_tokens),
                u64::from(usage.completion_tokens),
                u64::from(usage.cached_prompt_tokens),
            )
        });
        Self {
            input_tokens: u64::from(usage.prompt_tokens),
            output_tokens: u64::from(usage.completion_tokens),
            cached_read_tokens: u64::from(usage.cached_prompt_tokens),
            reasoning_tokens: u64::from(usage.reasoning_tokens),
            model_calls: 1,
            api_duration_ms: api_duration_ms.unwrap_or(0),
            decode_duration_ms: decode_duration_ms.unwrap_or(0),
            cost_usd_ticks,
            cost_missing_calls: u64::from(cost_usd_ticks.is_none()),
        }
    }

    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    /// 稳态解码速率 tok/s（剔除首字延迟）：只有当已经采样到解码时长且输出非零时才有值。
    /// 分母为 0 或输出为 0 时返回 `None`，避免展示层显示成 `0 tok/s`。
    pub fn decode_tokens_per_sec(&self) -> Option<f64> {
        if self.output_tokens == 0 || self.decode_duration_ms == 0 {
            return None;
        }
        Some(self.output_tokens as f64 * 1000.0 / self.decode_duration_ms as f64)
    }

    pub fn cost_is_partial(&self) -> bool {
        self.cost_usd_ticks.is_some() && self.cost_missing_calls > 0
    }

    fn fold_totals(&mut self, other: &UsageTotals) {
        let Self {
            input_tokens,
            output_tokens,
            cached_read_tokens,
            reasoning_tokens,
            model_calls,
            api_duration_ms,
            decode_duration_ms,
            cost_usd_ticks,
            cost_missing_calls,
        } = other;
        self.input_tokens = self.input_tokens.saturating_add(*input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(*output_tokens);
        self.cached_read_tokens = self.cached_read_tokens.saturating_add(*cached_read_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(*reasoning_tokens);
        self.model_calls = self.model_calls.saturating_add(*model_calls);
        self.api_duration_ms = self.api_duration_ms.saturating_add(*api_duration_ms);
        self.decode_duration_ms = self.decode_duration_ms.saturating_add(*decode_duration_ms);
        self.cost_missing_calls = self.cost_missing_calls.saturating_add(*cost_missing_calls);
        self.cost_usd_ticks = merge_cost_ticks(self.cost_usd_ticks, *cost_usd_ticks);
    }
}

fn merge_cost_ticks(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0).saturating_add(b.unwrap_or(0))),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageLedger {
    pub totals: UsageTotals,
    pub by_model: IndexMap<String, UsageTotals>,
    /// Main-agent loop rounds for `num_turns` (subagents excluded).
    pub main_loop_model_calls: u64,
    /// Bill may under-count (drain timeout, nested subagent incomplete, apply failure).
    pub incomplete: bool,
}

impl UsageLedger {
    /// Fold one main-agent-loop model call. This is the only writer of
    /// `main_loop_model_calls` (the wire `numTurns`); side calls such as
    /// compaction must not use it.
    pub fn record_main_loop_call(
        &mut self,
        model_id: &str,
        usage: &TokenUsage,
        api_duration_ms: Option<u64>,
        decode_duration_ms: Option<u64>,
        cost_usd_ticks: Option<i64>,
    ) {
        let call = UsageTotals::from_call(
            model_id,
            usage,
            api_duration_ms,
            decode_duration_ms,
            cost_usd_ticks,
        );
        self.main_loop_model_calls = self.main_loop_model_calls.saturating_add(1);
        self.fold_entry(model_id, &call);
    }

    /// Fold subagent usage without incrementing `main_loop_model_calls`.
    pub fn record_subagent(&mut self, by_model: &[(String, UsageTotals)], incomplete: bool) {
        for (model_id, totals) in by_model {
            self.fold_entry(model_id, totals);
        }
        if incomplete {
            self.incomplete = true;
        }
    }

    pub fn mark_incomplete(&mut self) {
        self.incomplete = true;
    }

    fn fold_entry(&mut self, model_id: &str, totals: &UsageTotals) {
        self.totals.fold_totals(totals);
        self.by_model
            .entry(model_id.to_owned())
            .or_default()
            .fold_totals(totals);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tu(prompt: u32, completion: u32) -> TokenUsage {
        TokenUsage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: 999_999,
            reasoning_tokens: 0,
            cached_prompt_tokens: 0,
        }
    }

    #[test]
    fn ledger_sums_partial_subagent_and_zero_cost() {
        let mut ledger = UsageLedger::default();
        ledger.record_main_loop_call("m", &tu(1, 1), None, None, Some(0));
        assert_eq!(ledger.totals.cost_usd_ticks, None);
        assert_eq!(ledger.totals.cost_missing_calls, 1);

        ledger.record_main_loop_call("a", &tu(100, 10), Some(100), None, None);
        ledger.record_main_loop_call("a", &tu(50, 5), Some(50), None, Some(70));
        assert_eq!(ledger.totals.cost_usd_ticks, Some(70));
        assert!(ledger.totals.cost_is_partial());
        assert_eq!(ledger.main_loop_model_calls, 3);

        ledger.record_subagent(
            &[(
                "b".into(),
                UsageTotals {
                    input_tokens: 5,
                    model_calls: 1,
                    ..Default::default()
                },
            )],
            false,
        );
        assert_eq!(ledger.by_model["b"].input_tokens, 5);
        assert_eq!(ledger.main_loop_model_calls, 3);
        assert_eq!(ledger.totals.model_calls, 4);
        assert!(!ledger.incomplete);

        ledger.record_subagent(&[], true);
        assert!(ledger.incomplete);
    }

    /// `decode_duration_ms` 是可加性原始量：多次调用需要 Σ；缺样本时保留为 0。
    /// 稳态速率的除法只在样本非零时返回 Some。
    #[test]
    fn ledger_accumulates_decode_duration_and_reports_tps() {
        let mut ledger = UsageLedger::default();
        // 第一次调用：output=200, elapsed=1200ms, ttft=200ms → decode=1000ms → 200 tok/s。
        ledger.record_main_loop_call("m", &tu(50, 200), Some(1_200), Some(1_000), None);
        assert_eq!(ledger.totals.decode_duration_ms, 1_000);
        assert_eq!(ledger.by_model["m"].decode_duration_ms, 1_000);
        assert!((ledger.totals.decode_tokens_per_sec().unwrap() - 200.0).abs() < 1e-6);

        // 第二次同模型：output=100, decode=500ms → 累计 output=300, decode=1500ms → 200 tok/s。
        ledger.record_main_loop_call("m", &tu(30, 100), Some(700), Some(500), None);
        assert_eq!(ledger.totals.decode_duration_ms, 1_500);
        assert_eq!(ledger.by_model["m"].decode_duration_ms, 1_500);
        assert_eq!(ledger.totals.output_tokens, 300);
        assert!((ledger.totals.decode_tokens_per_sec().unwrap() - 200.0).abs() < 1e-6);

        // 换模型：output=100, decode=250ms → 400 tok/s；主 ledger 汇总为 500/1750。
        ledger.record_main_loop_call("n", &tu(10, 100), Some(400), Some(250), None);
        assert_eq!(ledger.by_model["n"].decode_duration_ms, 250);
        assert_eq!(ledger.by_model["m"].decode_duration_ms, 1_500); // 未污染 m。
        assert!((ledger.by_model["n"].decode_tokens_per_sec().unwrap() - 400.0).abs() < 1e-6);
        assert_eq!(ledger.totals.decode_duration_ms, 1_750);
        assert_eq!(ledger.totals.output_tokens, 400);
    }

    /// 缺 ttft / 缺 output / 分母为 0 时不能露出 0 tok/s。
    #[test]
    fn ledger_decode_tps_returns_none_when_undefined() {
        // 情形 1：解码时长完全没被采样。
        let mut ledger = UsageLedger::default();
        ledger.record_main_loop_call("m", &tu(50, 200), Some(1_200), None, None);
        assert_eq!(ledger.totals.decode_duration_ms, 0);
        assert_eq!(ledger.totals.decode_tokens_per_sec(), None);

        // 情形 2：输出为 0（只出了推理或直接失败）。
        let mut ledger = UsageLedger::default();
        ledger.record_main_loop_call("m", &tu(50, 0), Some(1_200), Some(1_000), None);
        assert_eq!(ledger.totals.decode_tokens_per_sec(), None);

        // 情形 3：解码时长显式为 0（首字后立即结束）。
        let mut ledger = UsageLedger::default();
        ledger.record_main_loop_call("m", &tu(50, 200), Some(200), Some(0), None);
        assert_eq!(ledger.totals.decode_tokens_per_sec(), None);
    }
}
