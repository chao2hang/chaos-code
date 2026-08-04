//! CatPaw Remote Agent protocol types and stream normalization.
//!
//! The installed CatPawAI 2026.4.7 client treats Remote Agent as a separate,
//! autonomous protocol. Its tools execute upstream; callers only receive the
//! resulting assistant text. This module deliberately keeps internal tool
//! traces separate from any OpenAI `tool_calls` representation.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub enabled: bool,
    pub git_repo_url: String,
    #[serde(default = "default_branch")]
    pub git_base_branch: String,
    #[serde(default)]
    pub git_checkout_branch: String,
}

fn default_branch() -> String {
    "master".into()
}

impl AgentConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| Error::Config(format!("reading Agent config {}: {e}", path.display())))?;
        let config: Self = serde_json::from_str(&raw)
            .map_err(|e| Error::Config(format!("parsing Agent config {}: {e}", path.display())))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        validate_repo_url(&self.git_repo_url)?;
        if self.git_base_branch.trim().is_empty() {
            return Err(Error::Config(
                "agent git_base_branch must not be empty".into(),
            ));
        }
        Ok(())
    }
}

/// Per-request repository override resolved from an incoming Agent request.
///
/// Remote Agent targets a specific repository, so the relay lets callers pin
/// the project on every `/v1/agent/completions` request. Any omitted field
/// falls back to the Agent config default; the URL must stay on the internal
/// `git.sankuai.com` host. The override is recorded against the created
/// conversation so continuations reuse the same repository.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRepoOverride {
    #[serde(default, alias = "gitRepoUrl")]
    pub git_repo_url: Option<String>,
    #[serde(default, alias = "gitBaseBranch")]
    pub git_base_branch: Option<String>,
    #[serde(default, alias = "gitCheckoutBranch")]
    pub git_checkout_branch: Option<String>,
}

impl AgentRepoOverride {
    /// Validates the explicitly provided fields against the same rules as the
    /// config, so a malformed per-request repo fails before any upstream call.
    pub fn validate(&self) -> Result<()> {
        if let Some(url) = &self.git_repo_url {
            validate_repo_url(url)?;
        }
        if let Some(branch) = &self.git_base_branch
            && branch.trim().is_empty()
        {
            return Err(Error::Config(
                "agent git_base_branch must not be empty".into(),
            ));
        }
        Ok(())
    }

    /// Merges this override over `config`: the override wins where present,
    /// the config supplies every remaining field.
    pub fn merge(&self, config: &AgentConfig) -> Self {
        Self {
            git_repo_url: self
                .git_repo_url
                .clone()
                .or_else(|| Some(config.git_repo_url.clone())),
            git_base_branch: self
                .git_base_branch
                .clone()
                .or_else(|| Some(config.git_base_branch.clone())),
            git_checkout_branch: self
                .git_checkout_branch
                .clone()
                .or_else(|| Some(config.git_checkout_branch.clone())),
        }
    }

    fn normalized_repo_url(&self) -> String {
        self.git_repo_url
            .as_deref()
            .map(normalize_repo_url)
            .unwrap_or_default()
    }
}

fn validate_repo_url(url: &str) -> Result<()> {
    if !url.starts_with("ssh://git@git.sankuai.com/") {
        return Err(Error::Config(
            "agent git_repo_url must start with ssh://git@git.sankuai.com/".into(),
        ));
    }
    Ok(())
}

fn normalize_repo_url(url: &str) -> String {
    let url = url.trim_end_matches('/');
    if url.ends_with(".git") {
        url.to_string()
    } else {
        format!("{url}.git")
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCreateRequest {
    pub model_type: i32,
    pub git_repo_url: String,
    pub git_base_branch: String,
    // The 2026.4.7 primary entry sends this key even when its value is empty.
    pub git_checkout_branch: String,
    pub prompt: String,
    pub mode: &'static str,
    pub auto_deploy: bool,
    pub auto_pull_request: bool,
    pub source: &'static str,
    pub appkeys: Vec<String>,
    pub image_urls: Vec<String>,
    pub contexts: Vec<Value>,
    pub editor_context_states: Vec<Value>,
    pub mcp_servers: Vec<Value>,
}

impl AgentCreateRequest {
    pub fn new(
        config: &AgentConfig,
        repo: &AgentRepoOverride,
        model_type: i32,
        prompt: String,
    ) -> Self {
        let merged = repo.merge(config);
        Self {
            model_type,
            git_repo_url: merged.normalized_repo_url(),
            git_base_branch: merged.git_base_branch.unwrap_or_default(),
            git_checkout_branch: merged.git_checkout_branch.unwrap_or_default(),
            prompt,
            mode: "REMOTE_AGENT",
            auto_deploy: false,
            auto_pull_request: false,
            source: "CatPaw",
            appkeys: vec![],
            image_urls: vec![],
            contexts: vec![],
            editor_context_states: vec![],
            mcp_servers: vec![],
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentContinueRequest {
    pub conversation_id: String,
    pub model_type: i32,
    pub git_repo_url: String,
    pub git_base_branch: String,
    pub git_checkout_branch: String,
    pub prompt: String,
    pub source: &'static str,
    pub appkeys: Vec<String>,
    pub image_urls: Vec<String>,
    pub contexts: Vec<Value>,
    pub editor_context_states: Vec<Value>,
}

impl AgentContinueRequest {
    pub fn new(
        config: &AgentConfig,
        repo: &AgentRepoOverride,
        conversation_id: String,
        model_type: i32,
        prompt: String,
    ) -> Self {
        let merged = repo.merge(config);
        Self {
            conversation_id,
            model_type,
            git_repo_url: merged.normalized_repo_url(),
            git_base_branch: merged.git_base_branch.unwrap_or_default(),
            git_checkout_branch: merged.git_checkout_branch.unwrap_or_default(),
            prompt,
            source: "CatPaw",
            appkeys: vec![],
            image_urls: vec![],
            contexts: vec![],
            editor_context_states: vec![],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCreateResponse {
    pub conversation_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConnectRequest {
    pub timestamp: i64,
    pub conversation_id: String,
    pub message_index: i32,
}

impl AgentConnectRequest {
    pub fn new(conversation_id: impl Into<String>) -> Self {
        Self {
            timestamp: chrono::Utc::now().timestamp_millis(),
            conversation_id: conversation_id.into(),
            message_index: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AgentToolTrace {
    pub name: Option<String>,
    pub params: Value,
    pub result: Option<String>,
    pub image_urls: Vec<String>,
}

#[derive(Debug, Default)]
pub struct AgentEventDelta {
    pub content: String,
    pub done: bool,
    pub status: Option<String>,
    pub error: Option<String>,
}

/// Converts cumulative Agent message snapshots into assistant-text deltas.
///
/// Tool-use and tool-result blocks are paired by `toolCallId` for diagnostics,
/// but are never emitted as external function calls: CatPaw's Agent owns and
/// completes that loop itself.
#[derive(Debug)]
pub struct AgentEventAccumulator {
    requested_prompt: String,
    text_by_message: HashMap<String, String>,
    tool_traces: HashMap<String, AgentToolTrace>,
}

impl AgentEventAccumulator {
    pub fn new(requested_prompt: impl Into<String>) -> Self {
        Self {
            requested_prompt: requested_prompt.into(),
            text_by_message: HashMap::new(),
            tool_traces: HashMap::new(),
        }
    }

    pub fn tool_traces(&self) -> &HashMap<String, AgentToolTrace> {
        &self.tool_traces
    }

    pub fn ingest(&mut self, event: &Value) -> AgentEventDelta {
        let mut out = AgentEventDelta::default();

        if let Some(status) = event.get("statusUpdate").and_then(Value::as_str) {
            out.status = Some(status.to_string());
            if matches!(status, "completed" | "canceled") {
                out.done = true;
            }
        }

        let root = event
            .get("headlessRemoteAgentResp")
            .or_else(|| event.get("headlessBackgroundAgentResp"))
            .unwrap_or(event);

        if let Some(error) = root.get("error").or_else(|| event.get("error")) {
            out.error = Some(error_text(error));
            out.done = true;
            return out;
        }

        let is_history = root
            .get("isHistoryMessage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if is_history {
            if let Some(history) = root.get("historyMessages").and_then(Value::as_array) {
                self.ingest_history(history, &mut out);
            }
            return out;
        }

        if let Some(message) = root.get("message").or_else(|| event.get("message")) {
            self.ingest_message(message, "live", &mut out);
        }
        out
    }

    fn ingest_history(&mut self, history: &[Value], out: &mut AgentEventDelta) {
        let prompt = self.requested_prompt.trim();
        let Some(prompt_index) = history.iter().rposition(|message| {
            message.get("type").and_then(Value::as_str) == Some("user")
                && message_text(message).trim() == prompt
        }) else {
            // A continuation connects before `/conversation/continue` is sent.
            // Its initial history therefore has no current prompt and must not
            // be replayed to the caller.
            return;
        };

        for (index, message) in history.iter().enumerate().skip(prompt_index + 1) {
            self.ingest_message(message, &format!("history-{index}"), out);
        }
    }

    fn ingest_message(&mut self, message: &Value, fallback_id: &str, out: &mut AgentEventDelta) {
        let message_type = message
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("assistant");
        let blocks = message.get("content").and_then(Value::as_array);

        if let Some(blocks) = blocks {
            self.capture_tool_traces(blocks);
        }

        if message_type == "tool" || message_type == "user" {
            return;
        }

        let text = message_text(message);
        if text.is_empty() {
            return;
        }
        let message_id = message
            .get("messageId")
            .and_then(Value::as_str)
            .unwrap_or(fallback_id)
            .to_string();
        let previous = self
            .text_by_message
            .get(&message_id)
            .map(String::as_str)
            .unwrap_or("");
        if text == previous {
            return;
        }

        out.content
            .push_str(text.strip_prefix(previous).unwrap_or(&text));
        self.text_by_message.insert(message_id, text);
    }

    fn capture_tool_traces(&mut self, blocks: &[Value]) {
        for block in blocks {
            let Some(id) = block.get("toolCallId").and_then(Value::as_str) else {
                continue;
            };
            let trace = self
                .tool_traces
                .entry(id.to_string())
                .or_insert_with(|| AgentToolTrace {
                    params: json!({}),
                    ..AgentToolTrace::default()
                });

            if let Some(name) = block.get("toolName").and_then(Value::as_str) {
                trace.name = Some(name.to_string());
            }
            if let Some(params) = block.get("toolParams") {
                trace.params = parse_tool_params(params);
            }
            if let Some(result) = block.get("toolResult") {
                trace.result = Some(value_text(result));
            }
            if let Some(images) = block.get("imageUrl") {
                trace.image_urls = match images {
                    Value::Array(values) => values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect(),
                    Value::String(value) => vec![value.clone()],
                    _ => vec![],
                };
            }
        }
    }
}

fn message_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect(),
        _ => String::new(),
    }
}

fn parse_tool_params(params: &Value) -> Value {
    match params {
        Value::String(raw) if raw.trim().is_empty() => json!({}),
        Value::String(raw) => {
            serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.clone()))
        }
        value => value.clone(),
    }
}

fn value_text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn error_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    for key in ["message", "msg", "errorMessage", "detail"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return text.to_string();
        }
    }
    value.to_string()
}

/// Incremental decoder for the two stream forms present in the installed app:
/// regular SSE `data:` lines and a fallback stream of concatenated JSON roots.
#[derive(Debug, Default)]
pub struct AgentStreamDecoder {
    buffer: Vec<u8>,
}

impl AgentStreamDecoder {
    pub fn push_bytes(&mut self, bytes: &[u8]) -> Vec<Value> {
        self.buffer.extend_from_slice(bytes);
        self.decode(false)
    }

    pub fn finish(&mut self) -> Vec<Value> {
        self.decode(true)
    }

    fn decode(&mut self, flush: bool) -> Vec<Value> {
        let text = match std::str::from_utf8(&self.buffer) {
            Ok(text) => text.to_string(),
            Err(error) if error.error_len().is_none() && !flush => return vec![],
            Err(_) => String::from_utf8_lossy(&self.buffer).into_owned(),
        }
        .replace("\r\n", "\n")
        .replace('\r', "\n");

        if looks_like_sse(&text) {
            self.decode_sse(text, flush)
        } else {
            let (values, remainder) = extract_json_objects(&text, flush);
            self.buffer = remainder.into_bytes();
            values
        }
    }

    fn decode_sse(&mut self, text: String, flush: bool) -> Vec<Value> {
        let split_at = if flush {
            text.len()
        } else {
            text.rfind('\n').map_or(0, |index| index + 1)
        };
        let (complete, remainder) = text.split_at(split_at);
        let mut values = Vec::new();

        for line in complete.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            if let Some(payload) = line.strip_prefix("data:") {
                let payload = payload.trim();
                if payload.is_empty() || payload == "[DONE]" {
                    continue;
                }
                if let Ok(value) = serde_json::from_str(payload) {
                    values.push(value);
                } else {
                    values.extend(extract_json_objects(payload, true).0);
                }
            } else {
                values.extend(extract_json_objects(line, true).0);
            }
        }

        self.buffer = remainder.as_bytes().to_vec();
        values
    }
}

fn looks_like_sse(text: &str) -> bool {
    text.starts_with("data:")
        || text.starts_with(':')
        || text.contains("\ndata:")
        || text.contains("\n:")
}

fn extract_json_objects(input: &str, flush: bool) -> (Vec<Value>, String) {
    let mut values = Vec::new();
    let mut cursor = 0;
    let mut incomplete_at = None;

    while let Some(relative_start) = input[cursor..].find('{') {
        let start = cursor + relative_start;
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        let mut end = None;

        for (relative, ch) in input[start..].char_indices() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }

            match ch {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = Some(start + relative + ch.len_utf8());
                        break;
                    }
                }
                _ => {}
            }
        }

        let Some(end) = end else {
            incomplete_at = Some(start);
            break;
        };
        if let Ok(value) = serde_json::from_str(&input[start..end]) {
            values.push(value);
        }
        cursor = end;
    }

    let remainder = if flush {
        String::new()
    } else if let Some(start) = incomplete_at {
        input[start..].to_string()
    } else if cursor > 0 {
        input[cursor..].to_string()
    } else {
        // Preserve a short suffix so a split `data:` prefix can be completed by
        // the next byte chunk without allowing arbitrary noise to grow forever.
        let keep_from = input
            .char_indices()
            .rev()
            .nth(15)
            .map(|(index, _)| index)
            .unwrap_or(0);
        input[keep_from..].to_string()
    };

    (values, remainder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AgentConfig {
        AgentConfig {
            enabled: true,
            git_repo_url: "ssh://git@git.sankuai.com/org/repo".into(),
            git_base_branch: "main".into(),
            git_checkout_branch: "".into(),
        }
    }

    #[test]
    fn create_payload_matches_installed_primary_entry() {
        let value = serde_json::to_value(AgentCreateRequest::new(
            &config(),
            &AgentRepoOverride::default(),
            9,
            "fix it".into(),
        ))
        .unwrap();
        assert_eq!(value["modelType"], 9);
        assert_eq!(value["mode"], "REMOTE_AGENT");
        assert_eq!(
            value["gitRepoUrl"],
            "ssh://git@git.sankuai.com/org/repo.git"
        );
        assert_eq!(value["gitCheckoutBranch"], "");
        assert_eq!(value["mcpServers"], json!([]));
    }

    #[test]
    fn create_payload_prefers_per_request_repo_override() {
        let override_repo = AgentRepoOverride {
            git_repo_url: Some("ssh://git@git.sankuai.com/qxyfoods/qxy-mop".into()),
            git_base_branch: Some("release/2.0".into()),
            git_checkout_branch: Some("feat/x".into()),
        };
        let value = serde_json::to_value(AgentCreateRequest::new(
            &config(),
            &override_repo,
            9,
            "fix it".into(),
        ))
        .unwrap();
        assert_eq!(
            value["gitRepoUrl"],
            "ssh://git@git.sankuai.com/qxyfoods/qxy-mop.git"
        );
        assert_eq!(value["gitBaseBranch"], "release/2.0");
        assert_eq!(value["gitCheckoutBranch"], "feat/x");
    }

    #[test]
    fn override_merges_missing_fields_from_config() {
        let override_repo = AgentRepoOverride {
            git_repo_url: Some("ssh://git@git.sankuai.com/ss/mtd-react".into()),
            ..AgentRepoOverride::default()
        };
        let value = serde_json::to_value(AgentCreateRequest::new(
            &config(),
            &override_repo,
            9,
            "fix it".into(),
        ))
        .unwrap();
        assert_eq!(
            value["gitRepoUrl"],
            "ssh://git@git.sankuai.com/ss/mtd-react.git"
        );
        // base branch falls back to the config default
        assert_eq!(value["gitBaseBranch"], "main");
    }

    #[test]
    fn continue_payload_contains_only_installed_fields() {
        let value = serde_json::to_value(AgentContinueRequest::new(
            &config(),
            &AgentRepoOverride::default(),
            "conversation-1".into(),
            9,
            "next turn".into(),
        ))
        .unwrap();
        assert_eq!(value["conversationId"], "conversation-1");
        assert_eq!(value["prompt"], "next turn");
        assert_eq!(value["gitCheckoutBranch"], "");
        assert!(value.get("mode").is_none());
        assert!(value.get("mcpServers").is_none());
        assert!(value.get("tools").is_none());
        assert!(value.get("toolCallId").is_none());
    }

    #[test]
    fn finished_message_does_not_finish_the_agent_run() {
        let mut accumulator = AgentEventAccumulator::new("fix it");
        let first = accumulator.ingest(&json!({"headlessRemoteAgentResp": {"message": {
            "type": "assistant", "messageId": "m1", "finished": true,
            "content": [{"type": "text", "text": "done with one step"}]
        }}}));
        assert_eq!(first.content, "done with one step");
        assert!(!first.done);

        let completed = accumulator.ingest(&json!({"statusUpdate": "completed"}));
        assert!(completed.done);
    }

    #[test]
    fn internal_tool_use_and_result_are_paired_but_not_emitted_as_content() {
        let mut accumulator = AgentEventAccumulator::new("inspect");
        let use_delta = accumulator.ingest(&json!({"headlessRemoteAgentResp": {"message": {
            "type": "assistant", "messageId": "m1", "content": [{
                "toolCallId": "call-1", "toolName": "read_file",
                "toolParams": "{\"path\":\"a.rs\"}"
            }]
        }}}));
        let result_delta = accumulator.ingest(&json!({"headlessRemoteAgentResp": {"message": {
            "type": "tool", "content": [{
                "toolCallId": "call-1", "toolResult": "file contents"
            }]
        }}}));
        assert!(use_delta.content.is_empty());
        assert!(result_delta.content.is_empty());
        let trace = accumulator.tool_traces().get("call-1").unwrap();
        assert_eq!(trace.name.as_deref(), Some("read_file"));
        assert_eq!(trace.params, json!({"path": "a.rs"}));
        assert_eq!(trace.result.as_deref(), Some("file contents"));
    }

    #[test]
    fn history_only_emits_messages_after_the_requested_prompt() {
        let mut accumulator = AgentEventAccumulator::new("new prompt");
        let delta = accumulator.ingest(&json!({"headlessRemoteAgentResp": {
            "isHistoryMessage": true,
            "historyMessages": [
                {"type": "user", "content": [{"type": "text", "text": "old prompt"}]},
                {"type": "assistant", "messageId": "old", "content": [{"type": "text", "text": "old answer"}]},
                {"type": "user", "content": [{"type": "text", "text": "new prompt"}]},
                {"type": "assistant", "messageId": "new", "content": [{"type": "text", "text": "new answer"}]}
            ]
        }}));
        assert_eq!(delta.content, "new answer");
    }

    #[test]
    fn decoder_handles_sse_comments_split_utf8_and_raw_json_fallback() {
        let mut sse = AgentStreamDecoder::default();
        let chinese = "你".as_bytes();
        assert!(
            sse.push_bytes(b": heartbeat\n\ndata: {\"statusUpdate\":\"run")
                .is_empty()
        );
        let mut tail = b"ning\",\"text\":\"".to_vec();
        tail.extend_from_slice(&chinese[..2]);
        assert!(sse.push_bytes(&tail).is_empty());
        let mut end = chinese[2..].to_vec();
        end.extend_from_slice(b"\"}\n\n");
        let values = sse.push_bytes(&end);
        assert_eq!(values[0]["statusUpdate"], "running");
        assert_eq!(values[0]["text"], "你");

        let mut raw = AgentStreamDecoder::default();
        let values =
            raw.push_bytes(br#"noise {"statusUpdate":"running"}{"statusUpdate":"completed"}"#);
        assert_eq!(values.len(), 2);
        assert_eq!(values[1]["statusUpdate"], "completed");
    }

    #[test]
    fn logical_error_is_terminal_and_readable() {
        let mut accumulator = AgentEventAccumulator::new("inspect");
        let delta = accumulator.ingest(&json!({
            "headlessRemoteAgentResp": {"error": {"message": "workspace failed"}}
        }));
        assert!(delta.done);
        assert_eq!(delta.error.as_deref(), Some("workspace failed"));
    }

    #[test]
    fn enabled_config_rejects_non_internal_repo() {
        let config = AgentConfig {
            enabled: true,
            git_repo_url: "https://github.com/example/repo".into(),
            git_base_branch: "main".into(),
            git_checkout_branch: "".into(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn request_override_rejects_non_internal_repo_and_empty_branch() {
        let bad_url = AgentRepoOverride {
            git_repo_url: Some("https://github.com/example/repo".into()),
            ..AgentRepoOverride::default()
        };
        assert!(bad_url.validate().is_err());

        let bad_branch = AgentRepoOverride {
            git_base_branch: Some(" ".into()),
            ..AgentRepoOverride::default()
        };
        assert!(bad_branch.validate().is_err());

        let good = AgentRepoOverride {
            git_repo_url: Some("ssh://git@git.sankuai.com/qxyfoods/qxy-mop".into()),
            git_base_branch: Some("master".into()),
            git_checkout_branch: None,
        };
        assert!(good.validate().is_ok());
    }
}
