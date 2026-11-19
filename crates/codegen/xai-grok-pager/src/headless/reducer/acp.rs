//! The `streaming-json` reducer: native ACP session updates, one JSON object per line.
//! It defines its own line shapes: [`AcpLine`], [`AcpUsageLine`], [`AcpEndLine`].

use serde::Serialize;
use serde_json::{Value, json};

use crate::headless::attach_result_usage;
use xai_grok_shell::extensions::notification::ResponseUsage;

use super::{
    Lifecycle, Reducer, StreamEvent, TurnEnd, attach_structured_output, to_line,
    tool_call_status_wire,
};

/// `streaming-json` per-response `usage` line (camelCase keys).
#[derive(Serialize)]
struct AcpUsageLine {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(rename = "messageId", skip_serializing_if = "Option::is_none")]
    message_id: Option<String>,
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<ResponseUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
}

/// `streaming-json` line shapes: an xAI `type`-tagged envelope derived from ACP updates.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AcpLine {
    Text {
        data: String,
    },
    Thought {
        data: String,
    },
    ToolCall {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        title: String,
        kind: Option<String>,
        status: Option<String>,
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(rename = "rawInput")]
        raw_input: Value,
        content: Value,
        locations: Value,
    },
    ToolCallUpdate {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        status: Option<String>,
        content: Value,
        #[serde(rename = "rawOutput")]
        raw_output: Value,
        locations: Value,
    },
    SubagentSpawned {
        #[serde(rename = "subagentId")]
        subagent_id: String,
        #[serde(rename = "attemptId", skip_serializing_if = "Option::is_none")]
        attempt_id: Option<String>,
        #[serde(rename = "eventSeq", skip_serializing_if = "Option::is_none")]
        event_seq: Option<u64>,
        #[serde(rename = "parentSessionId")]
        parent_session_id: String,
        #[serde(rename = "childSessionId")]
        child_session_id: String,
        #[serde(rename = "subagentType")]
        subagent_type: String,
        description: String,
        #[serde(rename = "parentToolCallId", skip_serializing_if = "Option::is_none")]
        parent_tool_call_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(rename = "workflowRunId", skip_serializing_if = "Option::is_none")]
        workflow_run_id: Option<String>,
        #[serde(rename = "agentAddress", skip_serializing_if = "Option::is_none")]
        agent_address: Option<String>,
        #[serde(rename = "runInBackground", skip_serializing_if = "Option::is_none")]
        run_in_background: Option<bool>,
        #[serde(rename = "startedAtMs", skip_serializing_if = "Option::is_none")]
        started_at_ms: Option<u64>,
        #[serde(rename = "durationMs", skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    SubagentProgress {
        #[serde(rename = "subagentId")]
        subagent_id: String,
        #[serde(rename = "attemptId", skip_serializing_if = "Option::is_none")]
        attempt_id: Option<String>,
        #[serde(rename = "eventSeq", skip_serializing_if = "Option::is_none")]
        event_seq: Option<u64>,
        #[serde(rename = "parentSessionId")]
        parent_session_id: String,
        #[serde(rename = "childSessionId")]
        child_session_id: String,
        #[serde(rename = "subagentType")]
        subagent_type: String,
        description: String,
        #[serde(rename = "parentToolCallId", skip_serializing_if = "Option::is_none")]
        parent_tool_call_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(rename = "workflowRunId", skip_serializing_if = "Option::is_none")]
        workflow_run_id: Option<String>,
        #[serde(rename = "agentAddress", skip_serializing_if = "Option::is_none")]
        agent_address: Option<String>,
        #[serde(rename = "runInBackground", skip_serializing_if = "Option::is_none")]
        run_in_background: Option<bool>,
        #[serde(rename = "startedAtMs", skip_serializing_if = "Option::is_none")]
        started_at_ms: Option<u64>,
        #[serde(rename = "durationMs", skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    SubagentFinished {
        #[serde(rename = "subagentId")]
        subagent_id: String,
        #[serde(rename = "attemptId", skip_serializing_if = "Option::is_none")]
        attempt_id: Option<String>,
        #[serde(rename = "eventSeq", skip_serializing_if = "Option::is_none")]
        event_seq: Option<u64>,
        #[serde(rename = "parentSessionId")]
        parent_session_id: String,
        #[serde(rename = "childSessionId")]
        child_session_id: String,
        #[serde(rename = "subagentType")]
        subagent_type: String,
        description: String,
        #[serde(rename = "parentToolCallId", skip_serializing_if = "Option::is_none")]
        parent_tool_call_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(rename = "workflowRunId", skip_serializing_if = "Option::is_none")]
        workflow_run_id: Option<String>,
        #[serde(rename = "agentAddress", skip_serializing_if = "Option::is_none")]
        agent_address: Option<String>,
        #[serde(rename = "runInBackground", skip_serializing_if = "Option::is_none")]
        run_in_background: Option<bool>,
        #[serde(rename = "startedAtMs", skip_serializing_if = "Option::is_none")]
        started_at_ms: Option<u64>,
        #[serde(rename = "durationMs", skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    Plan {
        entries: Value,
    },
    AvailableCommands {
        tools: Vec<String>,
        commands: Vec<String>,
    },
    MaxTurnsReached,
    Error {
        message: String,
    },
    AutoCompactStarted {
        percentage: u8,
    },
    AutoCompactCompleted,
    AutoCompactFailed {
        error: String,
    },
    AutoCompactCancelled,
    AutoContinueCompleted {
        total_tokens: u64,
    },
    ImageCompressed {
        message: String,
    },
    MemoryFlushStarted,
    MemoryFlushCompleted {
        result: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    MemoryCaptureActivity {
        activity: String,
        from_turn: u32,
        through_turn: u32,
        attempt: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

/// `streaming-json` terminal `end` line (spend fields merged in by the caller).
#[derive(Serialize)]
struct AcpEndLine<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(rename = "stopReason")]
    stop_reason: &'a str,
    #[serde(rename = "sessionId")]
    session_id: &'a str,
    #[serde(rename = "requestId")]
    request_id: &'a str,
}

/// `streaming-json`: native ACP session updates, one object per line.
pub(crate) struct AcpReducer;

impl Reducer for AcpReducer {
    fn reduce(&mut self, event: StreamEvent) -> Vec<Value> {
        let line = match event {
            StreamEvent::AgentMessage(data) => AcpLine::Text { data },
            StreamEvent::AgentThought(data) => AcpLine::Thought { data },
            StreamEvent::ToolCall(tc) => AcpLine::ToolCall {
                tool_call_id: tc.tool_call_id,
                title: tc.title,
                kind: tc.tool_kind,
                status: tc.status.and_then(tool_call_status_wire),
                tool_name: tc.tool_name,
                raw_input: tc.raw_input,
                content: tc.content,
                locations: tc.locations,
            },
            StreamEvent::ToolCallUpdate(u) => AcpLine::ToolCallUpdate {
                tool_call_id: u.tool_call_id,
                status: u.status.and_then(tool_call_status_wire),
                content: u.content,
                raw_output: u.raw_output,
                locations: u.locations,
            },
            StreamEvent::Subagent(event) => {
                let line = match event.event.as_str() {
                    "spawned" => AcpLine::SubagentSpawned {
                        subagent_id: event.subagent_id,
                        attempt_id: event.attempt_id,
                        event_seq: event.event_seq,
                        parent_session_id: event.parent_session_id,
                        child_session_id: event.child_session_id,
                        subagent_type: event.subagent_type,
                        description: event.description,
                        parent_tool_call_id: event.parent_tool_call_id,
                        status: event.status,
                        model: event.model,
                        workflow_run_id: event.workflow_run_id,
                        agent_address: event.agent_address,
                        run_in_background: event.run_in_background,
                        started_at_ms: event.started_at_ms,
                        duration_ms: event.duration_ms,
                    },
                    "progress" => AcpLine::SubagentProgress {
                        subagent_id: event.subagent_id,
                        attempt_id: event.attempt_id,
                        event_seq: event.event_seq,
                        parent_session_id: event.parent_session_id,
                        child_session_id: event.child_session_id,
                        subagent_type: event.subagent_type,
                        description: event.description,
                        parent_tool_call_id: event.parent_tool_call_id,
                        status: event.status,
                        model: event.model,
                        workflow_run_id: event.workflow_run_id,
                        agent_address: event.agent_address,
                        run_in_background: event.run_in_background,
                        started_at_ms: event.started_at_ms,
                        duration_ms: event.duration_ms,
                    },
                    _ => AcpLine::SubagentFinished {
                        subagent_id: event.subagent_id,
                        attempt_id: event.attempt_id,
                        event_seq: event.event_seq,
                        parent_session_id: event.parent_session_id,
                        child_session_id: event.child_session_id,
                        subagent_type: event.subagent_type,
                        description: event.description,
                        parent_tool_call_id: event.parent_tool_call_id,
                        status: event.status,
                        model: event.model,
                        workflow_run_id: event.workflow_run_id,
                        agent_address: event.agent_address,
                        run_in_background: event.run_in_background,
                        started_at_ms: event.started_at_ms,
                        duration_ms: event.duration_ms,
                    },
                };
                return vec![to_line(&line)];
            }
            StreamEvent::Plan(entries) => AcpLine::Plan { entries },
            StreamEvent::AvailableCommands {
                tools,
                commands,
                skills: _,
            } => AcpLine::AvailableCommands { tools, commands },
            StreamEvent::Lifecycle(l) => return vec![to_line(&acp_lifecycle_line(l))],
            // Only the Messages reducer consumes these, for its `--include-partial-messages` framing
            StreamEvent::ResponseStarted { .. } | StreamEvent::ReasoningCompleted { .. } => {
                return vec![];
            }
            StreamEvent::ResponseCompleted {
                message_id,
                stop_reason,
                usage,
                signature,
                stop_sequence: _,
            } => {
                return vec![to_line(&AcpUsageLine {
                    kind: "usage",
                    message_id,
                    stop_reason,
                    usage,
                    signature,
                })];
            }
            StreamEvent::TurnUsage { usage } => {
                return vec![to_line(&AcpUsageLine {
                    kind: "usage",
                    message_id: None,
                    stop_reason: None,
                    usage: Some(usage),
                    signature: None,
                })];
            }
        };
        vec![to_line(&line)]
    }

    fn max_turns(&mut self) -> Vec<Value> {
        vec![to_line(&AcpLine::MaxTurnsReached)]
    }

    fn finish(&mut self, end: &TurnEnd<'_>) -> Vec<Value> {
        let mut line = to_line(&AcpEndLine {
            kind: "end",
            stop_reason: end.stop_reason,
            session_id: end.session_id,
            request_id: end.request_id,
        });
        if let Some(usage) = end.usage {
            attach_result_usage(&mut line, usage);
        }
        attach_structured_output(&mut line, end.structured_output.clone());
        vec![line]
    }

    fn error(
        &mut self,
        message: &str,
        usage: Option<&Value>,
        _duration_ms: u64,
        _stop_reason: Option<&str>,
    ) -> Vec<Value> {
        let mut line = to_line(&AcpLine::Error {
            message: message.to_string(),
        });
        if let Some(usage) = usage {
            attach_result_usage(&mut line, usage);
        }
        vec![line]
    }
}

fn acp_lifecycle_line(l: Lifecycle) -> AcpLine {
    match l {
        Lifecycle::CompactStarted { percentage } => AcpLine::AutoCompactStarted { percentage },
        Lifecycle::CompactCompleted { .. } => AcpLine::AutoCompactCompleted,
        Lifecycle::CompactFailed { error } => AcpLine::AutoCompactFailed { error },
        Lifecycle::CompactCancelled => AcpLine::AutoCompactCancelled,
        Lifecycle::AutoContinue { total_tokens } => AcpLine::AutoContinueCompleted { total_tokens },
        Lifecycle::ImageCompressed { message } => AcpLine::ImageCompressed { message },
        Lifecycle::MemoryFlushStarted => AcpLine::MemoryFlushStarted,
        Lifecycle::MemoryFlushCompleted { result, path } => {
            AcpLine::MemoryFlushCompleted { result, path }
        }
        Lifecycle::MemoryCaptureActivity {
            activity,
            from_turn,
            through_turn,
            attempt,
            detail,
        } => AcpLine::MemoryCaptureActivity {
            activity,
            from_turn,
            through_turn,
            attempt,
            detail,
        },
    }
}

#[cfg(test)]
mod subagent_line_tests {
    use super::*;

    #[test]
    fn streaming_json_serializes_background_subagent_lifecycle_metadata() {
        let mut reducer = AcpReducer;
        let lines = reducer.reduce(StreamEvent::Subagent(
            crate::headless::ext_protocol::SubagentStreamEvent {
                event: "spawned".into(),
                subagent_id: "agent-1".into(),
                attempt_id: Some("attempt-1".into()),
                event_seq: Some(7),
                parent_session_id: "parent-1".into(),
                child_session_id: "child-1".into(),
                subagent_type: "explore".into(),
                description: "Inspect the repository".into(),
                parent_tool_call_id: Some("tool-1".into()),
                status: Some("running".into()),
                model: Some("grok-test".into()),
                workflow_run_id: None,
                agent_address: None,
                run_in_background: Some(true),
                started_at_ms: Some(1234),
                duration_ms: None,
            },
        ));
        assert_eq!(lines[0]["type"], "subagent_spawned");
        assert_eq!(lines[0]["childSessionId"], "child-1");
        assert_eq!(lines[0]["parentToolCallId"], "tool-1");
        assert_eq!(lines[0]["runInBackground"], true);
        assert_eq!(lines[0]["startedAtMs"], 1234);
    }
}
