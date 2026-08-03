//! `x.ai/memory/flush`, `x.ai/memory/rewrite`, `x.ai/compact_conversation`,
//! and `x.ai/session/set_context_window` extension handlers.
//!
//! - `compact_conversation`: trigger an on-demand compaction for a session.
//! - `session/set_context_window`: dynamically resize the session context
//!   window; shrinks may compact immediately when usage is over budget.
//! - `memory/flush`: trigger an on-demand memory flush for a session.
//! - `memory/rewrite`: rewrite a raw memory note into structured markdown via
//!   a one-shot LLM call.

use agent_client_protocol as acp;
use serde::Deserialize;
use tokio::sync::oneshot;

use super::{Empty, ExtResult, parse_params, to_ext_response, to_raw_response};
use crate::agent::MvpAgent;
use crate::session::{CompactConversationRequest, CompactConversationResponse, SessionCommand};

#[tracing::instrument(skip_all, fields(method = %args.method))]
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        m if m.starts_with("x.ai/compact_conversation") => handle_compact(agent, args).await,
        "x.ai/session/set_context_window" => handle_set_context_window(agent, args).await,
        "x.ai/memory/flush" => handle_flush(agent, args).await,
        "x.ai/memory/rewrite" => handle_rewrite(agent, args).await,
        _ => Err(acp::Error::method_not_found()),
    }
}

async fn handle_set_context_window(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SetContextWindowRequest {
        session_id: String,
        /// Target context window in tokens (or with k/m suffix handled client-side).
        tokens: u64,
        /// Default true: compact when usage exceeds the new budget / threshold.
        #[serde(default = "default_true")]
        compact_if_needed: bool,
    }
    fn default_true() -> bool {
        true
    }

    let req: SetContextWindowRequest = parse_params(args)?;
    let tokens = std::num::NonZeroU64::new(req.tokens).ok_or_else(|| {
        acp::Error::invalid_params().data("tokens must be a positive integer".to_string())
    })?;
    // Soft upper bound to catch typos (e.g. missing unit → 2000000000).
    if tokens.get() > 10_000_000 {
        return Err(acp::Error::invalid_params()
            .data("tokens exceeds 10M; check the value (supports 128k, 200000, …)".to_string()));
    }

    let not_found_err = format!("session not found: {}", req.session_id);
    let session_handle = {
        let sessions = agent.sessions.borrow();
        sessions.get(&req.session_id.into()).cloned()
    };
    let Some(session) = session_handle else {
        return Err(acp::Error::invalid_params().data(not_found_err));
    };
    let (tx, rx) = oneshot::channel();
    let _ = session.cmd_tx.send(SessionCommand::SetContextWindow {
        tokens,
        compact_if_needed: req.compact_if_needed,
        respond_to: tx,
    });
    // Forward the structured ACP error unchanged: the inner handler
    // distinguishes "turn in flight" (InvalidRequest) from "internal"
    // (InternalError) and surfaces user-safe Chinese messages. Wrapping it
    // here with `internal_error().data(format!("{:?}", ...))` previously
    // flattened every failure to "Internal error: ErrorCode(InvalidRequest)
    // { code: -32600, message: ..., data: None }", which leaked Rust
    // internals into the TUI toast.
    let result = rx
        .await
        .map_err(|_| acp::Error::internal_error().data("session failed to respond"))?;
    match result {
        Ok(payload) => to_raw_response(&payload),
        Err(err) => Err(err),
    }
}

async fn handle_compact(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let req: CompactConversationRequest = parse_params(args)?;
    // send over the compact query here properly
    let session_handle = {
        let sessions = agent.sessions.borrow();
        sessions.get(&req.session_id.into()).cloned()
    };
    let (tx, rx) = oneshot::channel();
    if let Some(session) = session_handle {
        let _ = session.cmd_tx.send(SessionCommand::CompactSession {
            user_context: req.user_context,
            respond_to: tx,
        });
    }
    rx.await
        .map_err(|_| acp::Error::internal_error().data("session failed to respond"))?
        .map_err(|e| acp::Error::internal_error().data(format!("Internal error: {:?}", e)))?;
    to_raw_response(&CompactConversationResponse {})
}

async fn handle_flush(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    struct MemoryFlushRequest {
        session_id: String,
    }

    let req: MemoryFlushRequest = parse_params(args)?;
    let not_found_err = format!("session not found: {}", req.session_id);
    let session_handle = {
        let sessions = agent.sessions.borrow();
        sessions.get(&req.session_id.into()).cloned()
    };
    let Some(session) = session_handle else {
        return Err(acp::Error::invalid_params().data(not_found_err));
    };
    let (tx, rx) = oneshot::channel();
    let _ = session
        .cmd_tx
        .send(SessionCommand::FlushMemory { respond_to: tx });
    rx.await
        .map_err(|_| acp::Error::internal_error().data("session failed to respond"))?
        .map_err(|e| acp::Error::internal_error().data(format!("{:?}", e)))?;
    to_ext_response(Ok(Empty {}))
}

async fn handle_rewrite(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RewriteRequest {
        session_id: String,
        raw_text: String,
        context_summary: String,
    }

    let req: RewriteRequest = parse_params(args)?;
    let not_found_err = format!("session not found: {}", req.session_id);
    let session_handle = {
        let sessions = agent.sessions.borrow();
        sessions.get(&req.session_id.into()).cloned()
    };
    let Some(session) = session_handle else {
        return Err(acp::Error::invalid_params().data(not_found_err));
    };
    let (tx, rx) = oneshot::channel();
    let _ = session.cmd_tx.send(SessionCommand::RewriteMemoryNote {
        raw_text: req.raw_text,
        context_summary: req.context_summary,
        respond_to: tx,
    });
    let rewritten = rx
        .await
        .map_err(|_| acp::Error::internal_error().data("session failed to respond"))?
        .map_err(|e| acp::Error::internal_error().data(e))?;
    to_raw_response(&serde_json::json!({ "rewritten": rewritten }))
}
