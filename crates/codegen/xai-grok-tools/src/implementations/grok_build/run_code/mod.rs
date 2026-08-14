//! Code Mode — the `run_code` tool.
//!
//! `run_code` lets the model submit one Rhai script that makes several tool
//! calls, instead of N separate `tool_use`/`tool_result` round-trips. The
//! script can branch on structured tool output, filter it, and return only
//! the part that matters — the token saving that motivates Code Mode.
//!
//! ## Layering
//!
//! This crate is a leaf: tool definitions live here, but tool *dispatch*
//! lives in the session layer. So this tool does not execute anything
//! itself. It validates the script, then hands it to the session over a
//! [`RunCodeHandle`] channel (the same pattern `WorkflowTool` uses with
//! `WorkflowLaunchHandle`). The session owns the Rhai runtime and answers
//! the script's tool calls through its normal dispatch path, so
//! permission/auto-mode checks and hooks apply unchanged.
//!
//! ## Threat model
//!
//! The threat Code Mode defends against is **amplification**: a single
//! `run_code` invocation can batch up to 32 tool calls in one round-trip,
//! so a budget overrun or sandbox escape would compound faster than N
//! separate calls. The defense is layered:
//!
//! 1. **Rhai sandbox** — no `eval`, no module resolver, no ambient I/O.
//!    The script can only reach the outside world through `tools_call`.
//! 2. **Budget caps** — 5,000 ops / 30 s wall-clock / 32 tool calls per
//!    script, enforced via `engine.on_progress` + `CancellationToken`.
//! 3. **Permission inheritance** — every `tools_call` goes through the
//!    same `ToolBridge` dispatch as a normal `tool_use`, so permission
//!    rules, auto-mode checks, and hooks apply unchanged.
//! 4. **Preset visibility** — `run_code` is only exposed in presets that
//!    already trust the agent with a shell (`code` / `ask`), not in
//!    read-only presets (`explore` / `plan`).
//!
//! See `docs/code-mode-safety.md` for the full safety write-up.

use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};

/// The wire name the model calls.
pub const RUN_CODE_TOOL_NAME: &str = "run_code";

/// Upper bound on submitted script size, in bytes. Scripts are meant to be
/// short orchestration glue, not payloads.
const MAX_SCRIPT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct RunCodeToolInput {
    /// The Rhai script to execute.
    #[schemars(
        description = "A Rhai script. Call tools with `tools_call(name, #{ arg: value })` — it \
                       returns the tool's output as a value you can index into (`out.matches[0].path`). \
                       Available helpers: `json_encode(v)`, `json_decode(s)`, `complete(v)`. \
                       The script's final expression is the result returned to you; end with \
                       `complete(value)` to make that explicit. Tool errors are catchable with \
                       `try { ... } catch (e) { ... }`."
    )]
    pub script: String,

    /// Optional arguments bound to the script's `args` global.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Optional JSON object bound to the script's `args` global, e.g. \
                       `{\"dir\": \"src\"}` is readable as `args.dir`. Prefer this over \
                       string-interpolating values into the script."
    )]
    pub args: Option<serde_json::Value>,
}

impl RunCodeToolInput {
    pub fn normalize(&mut self) {
        self.script = self.script.trim().to_string();
        // An explicitly-null `args` is the same as omitting it.
        if matches!(self.args, Some(serde_json::Value::Null)) {
            self.args = None;
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.script.is_empty() {
            return Err("`script` must not be empty".into());
        }
        if self.script.len() > MAX_SCRIPT_BYTES {
            return Err(format!(
                "`script` is {} bytes, over the {MAX_SCRIPT_BYTES}-byte limit — \
                 keep scripts to orchestration glue",
                self.script.len()
            ));
        }
        if let Some(args) = &self.args
            && !args.is_object()
        {
            return Err("`args` must be a JSON object when provided".into());
        }
        Ok(())
    }
}

/// A script submitted for execution.
#[derive(Debug)]
pub struct RunCodeRequest {
    pub input: RunCodeToolInput,
}

/// The session's answer to a [`RunCodeRequest`].
#[derive(Debug)]
pub enum RunCodeAck {
    /// The script ran to completion.
    Completed {
        /// The script's result value.
        result: serde_json::Value,
        /// How many tool calls it made.
        tool_calls: u32,
    },
    /// The script failed to compile, errored, hit a budget, or was cancelled.
    Failed {
        error: String,
        /// Tool calls made before the failure.
        tool_calls: u32,
    },
    /// Code Mode is unavailable in this session, or the request was refused
    /// before execution.
    Rejected { code: &'static str, detail: String },
}

pub type RunCodeEnvelope = (RunCodeRequest, tokio::sync::oneshot::Sender<RunCodeAck>);

/// Channel to the session's Code Mode runner, published in `SharedResources`.
pub struct RunCodeHandle(pub tokio::sync::mpsc::UnboundedSender<RunCodeEnvelope>);

impl std::fmt::Debug for RunCodeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunCodeHandle").finish()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct RunCodeToolOutput {
    /// The value the script returned.
    pub result: serde_json::Value,
    /// How many tool calls the script made — the round-trips this one call
    /// replaced.
    pub tool_calls: u32,
}

impl xai_tool_runtime::ToolOutput for RunCodeToolOutput {}

#[derive(Debug, Default)]
pub struct RunCodeTool;

impl crate::types::tool_metadata::ToolMetadata for RunCodeTool {
    fn kind(&self) -> ToolKind {
        ToolKind::RunCode
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        r##"Run a Rhai script that makes several tool calls in one turn, instead of one `tool_use` per call. Use it when you know up front which calls you need and the later ones depend only on earlier *results*, not on your judgement: reading a list of files, grepping then reading each hit, checking several paths before editing. The script can index into structured output (`out.matches[0].path`), loop, branch, and return only what matters — so intermediate output never enters the conversation.

Call tools with `tools_call(name, #{ arg: value })`, using the exact tool name and arguments you would use to call it directly — the model-facing name from the tool list (e.g. `run_terminal_cmd`), not the tool's internal id. Helpers: `json_encode(v)`, `json_decode(s)`, `complete(v)`, and `args` (bound from the `args` parameter). A wrong tool name or a failed call raises a catchable error: wrap it in `try { ... } catch (e) { ... }` if you want to continue past a failure. The script's final expression is what you get back; end with `complete(value)` to say so explicitly.

Every call still goes through the normal permission checks, so a script can prompt the user or be denied mid-run. Limits per script: 32 tool calls, 5000 operations, 30 seconds. Do not use this to explore — if you need to *decide* what to do next based on what you read, make the calls directly and think between them."##
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }

    fn is_read_only(&self) -> bool {
        // A script can call write tools, so the meta-tool is not read-only.
        false
    }
}

impl xai_tool_runtime::Tool for RunCodeTool {
    type Args = RunCodeToolInput;
    type Output = RunCodeToolOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(RUN_CODE_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            RUN_CODE_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(xai_tool_protocol::ToolScope::Write),
            ..Default::default()
        }
    }

    #[tracing::instrument(name = "new_tool.run_code", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        mut input: RunCodeToolInput,
    ) -> Result<RunCodeToolOutput, xai_tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;

        input.normalize();
        if let Err(detail) = input.validate() {
            return Err(xai_tool_runtime::ToolError::custom(
                "run_code_invalid_input",
                detail,
            ));
        }

        let sender = {
            let res = resources.lock().await;
            res.get::<RunCodeHandle>().map(|h| h.0.clone())
        };
        let sender = sender.ok_or_else(|| {
            xai_tool_runtime::ToolError::custom(
                "run_code_not_available",
                "Code Mode is not available in this session (RunCodeHandle not registered)",
            )
        })?;

        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<RunCodeAck>();
        sender
            .send((RunCodeRequest { input }, ack_tx))
            .map_err(|_| {
                xai_tool_runtime::ToolError::custom(
                    "run_code_unavailable",
                    "the Code Mode runner is no longer accepting scripts",
                )
            })?;

        let ack = ack_rx.await.map_err(|_| {
            xai_tool_runtime::ToolError::custom(
                "run_code_no_response",
                "the Code Mode runner stopped without returning a result",
            )
        })?;

        match ack {
            RunCodeAck::Completed { result, tool_calls } => {
                Ok(RunCodeToolOutput { result, tool_calls })
            }
            // A script error is the model's to fix, so it comes back as a
            // tool error (with the tool-call count preserved in the message,
            // since a failed run has no output payload).
            RunCodeAck::Failed { error, tool_calls } => Err(xai_tool_runtime::ToolError::custom(
                "run_code_failed",
                format!("script failed after {tool_calls} tool call(s): {error}"),
            )),
            RunCodeAck::Rejected { code, detail } => {
                Err(xai_tool_runtime::ToolError::custom(code, detail))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(script: &str) -> RunCodeToolInput {
        RunCodeToolInput {
            script: script.to_string(),
            args: None,
        }
    }

    #[test]
    fn empty_script_is_rejected() {
        let mut i = input("   ");
        i.normalize();
        assert!(i.validate().is_err());
    }

    #[test]
    fn normalize_trims_and_drops_null_args() {
        let mut i = RunCodeToolInput {
            script: "  complete(1)  ".to_string(),
            args: Some(serde_json::Value::Null),
        };
        i.normalize();
        assert_eq!(i.script, "complete(1)");
        assert!(i.args.is_none(), "explicit null args normalizes to None");
        assert!(i.validate().is_ok());
    }

    #[test]
    fn oversized_script_is_rejected() {
        let mut i = input(&"x".repeat(MAX_SCRIPT_BYTES + 1));
        i.normalize();
        let err = i.validate().expect_err("oversized script must be refused");
        assert!(err.contains("over the"), "error was: {err}");
    }

    #[test]
    fn non_object_args_are_rejected() {
        let mut i = RunCodeToolInput {
            script: "complete(1)".to_string(),
            args: Some(serde_json::json!("not an object")),
        };
        i.normalize();
        assert!(i.validate().is_err(), "args must be an object");
    }

    #[test]
    fn object_args_are_accepted() {
        let mut i = RunCodeToolInput {
            script: "complete(args.x)".to_string(),
            args: Some(serde_json::json!({ "x": 1 })),
        };
        i.normalize();
        assert!(i.validate().is_ok());
    }

    #[test]
    fn tool_is_not_read_only_because_scripts_can_write() {
        use crate::types::tool_metadata::ToolMetadata;
        assert!(!RunCodeTool.is_read_only());
    }
}
