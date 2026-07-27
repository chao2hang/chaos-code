//! `x.ai/session/usage` — cumulative session token/cost as [`PromptUsage`].
//!
//! Projects the in-memory [`xai_chat_state::UsageLedger`] (main-loop + folded
//! subagent spend). Partial costs are scrubbed (absence ≠ free). Totals reset
//! when a session is resumed in a new agent process.
//!
//! Also provides `x.ai/usage/aggregate`, which returns the user's all-time
//! token/cost totals across every session stored locally.

use agent_client_protocol as acp;
use serde::{Deserialize, Serialize};

use super::{ExtResult, parse_params, to_raw_response};
use crate::agent::MvpAgent;
use crate::extensions::notification::PromptUsage;
use crate::session::usage_store::UsageStore;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionUsageRequest {
    session_id: String,
}

/// Wire response for `x.ai/session/usage`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsageResponse {
    pub usage: PromptUsage,
}

#[tracing::instrument(skip_all, fields(method = %args.method))]
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        "x.ai/session/usage" => handle_session_usage(agent, args).await,
        "x.ai/usage/aggregate" => handle_aggregate_usage().await,
        _ => Err(acp::Error::method_not_found()),
    }
}

async fn handle_session_usage(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let req: SessionUsageRequest = parse_params(args)?;
    let session_id = acp::SessionId::new(req.session_id.as_str());

    // Wait out in-flight session/load rather than racing reconnect to not-found.
    let Some(handle) = agent.session_handle_waiting_for_load(&session_id).await else {
        return Err(acp::Error::resource_not_found(Some(format!(
            "session not found: {}",
            req.session_id
        ))));
    };

    // Fail closed: a dead chat-state actor is an error, never a zero bill.
    let ledger = handle
        .chat_state_handle
        .try_get_session_usage()
        .await
        .map_err(|()| acp::Error::internal_error().data("failed to read session usage"))?;

    let usage = PromptUsage::from(&ledger);

    // Best-effort persistence into the local aggregate store. Failure here
    // must not break the session-usage response; the aggregate overlay will
    // simply miss this snapshot until the next successful write.
    //
    // Subagent sessions are skipped: their token spend is already folded
    // into the parent session's ledger, so persisting them separately would
    // double-count in the all-time aggregate.
    let is_subagent = session_kind_is_subagent(handle.session_kind.as_deref());
    if !is_subagent {
        if let Err(e) = persist_session_usage(&session_id.0, &usage) {
            tracing::warn!(
                session_id = %session_id.0,
                error = %e,
                "failed to persist session usage to aggregate store"
            );
        }
    }

    to_raw_response(&SessionUsageResponse { usage })
}

async fn handle_aggregate_usage() -> ExtResult {
    let store = UsageStore::open_default()
        .map_err(|e| acp::Error::internal_error().data(format!("usage store unavailable: {e}")))?;
    let usage = store.aggregate_prompt_usage().map_err(|e| {
        acp::Error::internal_error().data(format!("failed to read aggregate usage: {e}"))
    })?;
    to_raw_response(&SessionUsageResponse { usage })
}

/// Write a session usage snapshot into the local aggregate store.
fn persist_session_usage(session_id: &str, usage: &PromptUsage) -> Result<(), rusqlite::Error> {
    let store = UsageStore::open_default()?;
    store.record_session_usage(session_id, usage)
}

/// Returns `true` for session kinds whose spend is already folded into a
/// parent session's ledger (`"subagent"`, `"subagent_fork"`,
/// `"subagent_resume"`). The aggregate store skips these to avoid
/// double-counting.
fn session_kind_is_subagent(kind: Option<&str>) -> bool {
    kind.is_some_and(|k| k.starts_with("subagent"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use xai_chat_state::UsageLedger;
    use xai_grok_sampling_types::TokenUsage;

    fn usage(prompt: u32, completion: u32) -> TokenUsage {
        TokenUsage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: 0,
            reasoning_tokens: 0,
            cached_prompt_tokens: 0,
        }
    }

    #[test]
    fn response_serializes_ledger_as_prompt_usage_wire_shape() {
        let mut ledger = UsageLedger::default();
        ledger.record_main_loop_call("grok-build", &usage(100, 10), Some(50), Some(20_000_000));
        let v = serde_json::to_value(&SessionUsageResponse {
            usage: PromptUsage::from(&ledger),
        })
        .unwrap();
        assert_eq!(v["usage"]["inputTokens"], 100);
        assert_eq!(v["usage"]["outputTokens"], 10);
        assert_eq!(v["usage"]["numTurns"], 1);
        assert_eq!(v["usage"]["costUsdTicks"], 20_000_000);
        assert_eq!(v["usage"]["modelUsage"]["grok-build"]["inputTokens"], 100);
        let rt: SessionUsageResponse = serde_json::from_value(v).unwrap();
        assert_eq!(rt.usage.totals.cost_usd_ticks, Some(20_000_000));
    }

    #[test]
    fn response_scrubs_partial_costs() {
        let mut ledger = UsageLedger::default();
        ledger.record_main_loop_call("a", &usage(100, 10), None, Some(70));
        ledger.record_main_loop_call("a", &usage(50, 5), None, None);
        let v = serde_json::to_value(&SessionUsageResponse {
            usage: PromptUsage::from(&ledger),
        })
        .unwrap();
        assert_eq!(v["usage"]["costUsdTicks"], serde_json::Value::Null);
        assert_eq!(v["usage"]["costIsPartial"], true);
    }

    #[test]
    #[serial_test::serial]
    fn aggregate_usage_extension_returns_stored_totals() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("usage.sqlite");
        let _guard = xai_grok_test_support::env::EnvGuard::set("GROK_USAGE_STORE_PATH", &db_path);

        let store = crate::session::usage_store::UsageStore::open_or_create(&db_path).unwrap();
        let usage = PromptUsage {
            totals: crate::extensions::notification::PromptUsageModel {
                input_tokens: 1_000,
                output_tokens: 100,
                total_tokens: 1_100,
                model_calls: 2,
                cost_usd_ticks: Some(10_000_000_000),
                ..Default::default()
            },
            model_usage: {
                let mut m = indexmap::IndexMap::new();
                m.insert(
                    "grok-4".to_string(),
                    crate::extensions::notification::PromptUsageModel {
                        input_tokens: 1_000,
                        output_tokens: 100,
                        total_tokens: 1_100,
                        model_calls: 2,
                        cost_usd_ticks: Some(10_000_000_000),
                        ..Default::default()
                    },
                );
                m
            },
            num_turns: 1,
            usage_is_incomplete: false,
        };
        store.record_session_usage("session-1", &usage).unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let response = runtime.block_on(handle_aggregate_usage()).unwrap();
        let parsed: SessionUsageResponse = serde_json::from_str(response.0.get()).unwrap();
        assert_eq!(parsed.usage.totals.input_tokens, 1_000);
        assert_eq!(parsed.usage.totals.output_tokens, 100);
        assert_eq!(parsed.usage.model_usage.len(), 1);
        assert_eq!(parsed.usage.model_usage["grok-4"].input_tokens, 1_000);
    }

    #[test]
    fn session_kind_is_subagent_detects_subagent_variants() {
        assert!(session_kind_is_subagent(Some("subagent")));
        assert!(session_kind_is_subagent(Some("subagent_fork")));
        assert!(session_kind_is_subagent(Some("subagent_resume")));
        assert!(!session_kind_is_subagent(None));
        assert!(!session_kind_is_subagent(Some("fork")));
        assert!(!session_kind_is_subagent(Some("worktree")));
        assert!(!session_kind_is_subagent(Some("")));
    }
}
