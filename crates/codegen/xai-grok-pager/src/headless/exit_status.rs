//! Typed result of a headless run. No free-text matching is used to choose an exit code.

use agent_client_protocol as acp;
use std::fmt;
use xai_grok_shell::sampling::error::{
    RATE_LIMITED_ERROR_CODE, provider_kind_from_error, run_exit_reason_from_error,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunExitReason {
    Completed,
    ProviderError,
    MaxTurns,
    PermissionDenied,
    /// Reserved for a future typed budget limit; not emitted by the current runner.
    BudgetExceeded,
    InternalError,
    /// An ACP tool call reported `Failed` (without a typed permission rejection).
    ToolError,
}

impl RunExitReason {
    pub const fn code(self) -> i32 {
        match self {
            Self::Completed => 0,
            Self::ProviderError => 2,
            Self::MaxTurns => 3,
            Self::PermissionDenied => 4,
            Self::BudgetExceeded => 5,
            Self::InternalError | Self::ToolError => 1,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::ProviderError => "provider_error",
            Self::MaxTurns => "max_turns",
            Self::PermissionDenied => "permission_denied",
            Self::BudgetExceeded => "budget_exceeded",
            Self::InternalError => "internal_error",
            Self::ToolError => "tool_error",
        }
    }

    pub fn from_prompt_error(error: &acp::Error) -> Self {
        // `InternalError` is also used by the sampler for HTTP, SSE, and local failures.
        // It cannot classify provider errors without the shell's typed data marker.
        match run_exit_reason_from_error(error) {
            Some("provider_error") => Self::ProviderError,
            Some("permission_denied") => Self::PermissionDenied,
            _ if i32::from(error.code) == RATE_LIMITED_ERROR_CODE => Self::ProviderError,
            _ => Self::InternalError,
        }
    }

    pub fn from_prompt_response(response: &acp::PromptResponse, tool_failed: bool) -> Self {
        let category = response
            .meta
            .as_ref()
            .and_then(|meta| meta.get(crate::app::CANCELLATION_CATEGORY_KEY))
            .and_then(|value| value.as_str());
        if category == Some(xai_grok_shell::session::commands::MAX_TURNS_REACHED_CATEGORY)
            || response.stop_reason == acp::StopReason::MaxTurnRequests
        {
            return Self::MaxTurns;
        }
        if matches!(
            category,
            Some(xai_grok_shell::session::commands::PERMISSION_REJECTED_CATEGORY)
                | Some(xai_grok_shell::session::commands::PERMISSION_CANCELLED_CATEGORY)
                | Some(xai_grok_shell::session::commands::HOOK_DENIED_CATEGORY)
        ) {
            return Self::PermissionDenied;
        }
        if response.stop_reason == acp::StopReason::Cancelled {
            return Self::InternalError;
        }
        if tool_failed {
            return Self::ToolError;
        }
        Self::Completed
    }
}

#[derive(Debug)]
pub struct HeadlessFailure {
    pub reason: RunExitReason,
    pub kind: &'static str,
    pub model: Option<String>,
    message: String,
}

impl HeadlessFailure {
    pub fn new(reason: RunExitReason, message: impl Into<String>, model: Option<&str>) -> Self {
        let kind = match reason {
            RunExitReason::Completed => "completed",
            RunExitReason::ProviderError => "provider_error",
            RunExitReason::MaxTurns => "max_turns",
            RunExitReason::PermissionDenied => "permission_denied",
            RunExitReason::BudgetExceeded => "budget_exceeded",
            RunExitReason::InternalError => "internal_error",
            RunExitReason::ToolError => "tool_error",
        };
        Self {
            reason,
            kind,
            model: model.map(str::to_owned),
            message: message.into(),
        }
    }

    pub fn from_prompt_error(error: &acp::Error, message: impl Into<String>, model: Option<&str>) -> Self {
        let mut failure = Self::new(RunExitReason::from_prompt_error(error), message, model);
        if failure.reason == RunExitReason::ProviderError {
            failure.kind = match provider_kind_from_error(error) {
                Some("provider_unsupported_tools") => "provider_unsupported_tools",
                _ => "provider_error",
            };
        }
        failure
    }

    /// A one-line machine-readable stderr diagnostic; escape untrusted model IDs to
    /// prevent newline/space injection into the key=value record.
    pub fn diagnostic(&self) -> String {
        let model = self.model.as_deref().unwrap_or("unknown");
        let model: String = model
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() || "-._/:@".contains(ch) { ch } else { '_' })
            .collect();
        format!("chaos: error kind={} model={model}", self.kind)
    }
}

impl fmt::Display for HeadlessFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for HeadlessFailure {}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(reason: acp::StopReason, category: Option<&str>) -> acp::PromptResponse {
        let mut response = acp::PromptResponse::new(reason);
        if let Some(category) = category {
            let mut meta = acp::Meta::new();
            meta.insert(
                crate::app::CANCELLATION_CATEGORY_KEY.to_owned(),
                serde_json::Value::String(category.to_owned()),
            );
            response.meta = Some(meta);
        }
        response
    }

    #[test]
    fn exit_codes_match_the_headless_contract() {
        assert_eq!(RunExitReason::Completed.code(), 0);
        assert_eq!(RunExitReason::ProviderError.code(), 2);
        assert_eq!(RunExitReason::MaxTurns.code(), 3);
        assert_eq!(RunExitReason::PermissionDenied.code(), 4);
        assert_eq!(RunExitReason::BudgetExceeded.code(), 5);
        assert_eq!(RunExitReason::InternalError.code(), 1);
        assert_eq!(RunExitReason::ToolError.code(), 1);
    }

    #[test]
    fn prompt_response_uses_typed_categories_not_message_text() {
        assert_eq!(
            RunExitReason::from_prompt_response(
                &response(acp::StopReason::EndTurn, Some("max_turns_reached")),
                false
            ),
            RunExitReason::MaxTurns
        );
        assert_eq!(
            RunExitReason::from_prompt_response(
                &response(acp::StopReason::Cancelled, Some("PermissionRejected")),
                false
            ),
            RunExitReason::PermissionDenied
        );
        assert_eq!(
            RunExitReason::from_prompt_response(&response(acp::StopReason::EndTurn, None), true),
            RunExitReason::ToolError
        );
        assert_eq!(
            RunExitReason::from_prompt_response(&response(acp::StopReason::EndTurn, None), false),
            RunExitReason::Completed
        );
    }

    #[test]
    fn diagnostic_escapes_untrusted_model_ids() {
        let failure = HeadlessFailure::new(
            RunExitReason::ProviderError,
            "provider failed",
            Some("model with\nspace"),
        );
        assert_eq!(
            failure.diagnostic(),
            "chaos: error kind=provider_error model=model_with_space"
        );
    }
}
