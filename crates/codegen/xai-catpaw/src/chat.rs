//! Chat wire types and cumulative SSE snapshot accumulator.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub user_model_type_code: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatDelta {
    pub content: String,
    pub done: bool,
    pub usage: ChatUsage,
}

#[derive(Debug, Default)]
pub struct ChatAccumulator {
    full_content: String,
    previous_snapshot: String,
    usage: ChatUsage,
    seen_events: u64,
    metadata: HashMap<String, Value>,
}

impl ChatAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ingest(&mut self, event: &Value) -> ChatDelta {
        self.seen_events = self.seen_events.saturating_add(1);
        let mut delta = ChatDelta {
            usage: self.usage.clone(),
            ..ChatDelta::default()
        };
        if let Some(content) = event.get("content").and_then(Value::as_str) {
            let new_content = if content.starts_with(&self.previous_snapshot)
                && content.len() >= self.previous_snapshot.len()
            {
                &content[self.previous_snapshot.len()..]
            } else {
                content
            };
            delta.content = new_content.to_string();
            self.full_content.push_str(new_content);
            self.previous_snapshot = content.to_string();
        }
        if let Some(usage) = event.get("usage") {
            if let Some(value) = usage.get("prompt_tokens").and_then(Value::as_i64) {
                self.usage.prompt_tokens = value;
            }
            if let Some(value) = usage.get("completion_tokens").and_then(Value::as_i64) {
                self.usage.completion_tokens = value;
            }
            if let Some(value) = usage.get("total_tokens").and_then(Value::as_i64) {
                self.usage.total_tokens = value;
            }
            delta.usage = self.usage.clone();
        }
        if let Some(last_one) = event.get("lastOne").and_then(Value::as_bool) {
            delta.done = last_one;
        }
        for key in ["finishReason", "finish_reason", "model"] {
            if let Some(value) = event.get(key) {
                self.metadata.insert(key.to_string(), value.clone());
            }
        }
        delta
    }

    pub fn content(&self) -> &str {
        &self.full_content
    }

    pub fn usage(&self) -> &ChatUsage {
        &self.usage
    }

    pub fn event_count(&self) -> u64 {
        self.seen_events
    }

    pub fn metadata(&self) -> &HashMap<String, Value> {
        &self.metadata
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_request_serializes_camel_case_wire_fields() {
        let request = ChatRequest {
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            user_model_type_code: 75,
            stream: Some(true),
            temperature: None,
            max_tokens: Some(4096),
            top_p: None,
            response_format: None,
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["userModelTypeCode"], 75);
        assert_eq!(value["maxTokens"], 4096);
        assert_eq!(value["stream"], true);
        assert!(value.get("user_model_type_code").is_none());
        assert!(value.get("max_tokens").is_none());
    }

    #[test]
    fn cumulative_snapshots_emit_only_suffix_and_finish() {
        let mut accumulator = ChatAccumulator::new();
        assert_eq!(
            accumulator
                .ingest(&serde_json::json!({"content": "你"}))
                .content,
            "你"
        );
        assert_eq!(
            accumulator
                .ingest(&serde_json::json!({"content": "你好"}))
                .content,
            "好"
        );
        let final_delta = accumulator.ingest(&serde_json::json!({
            "content": "你好",
            "usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5},
            "lastOne": true
        }));
        assert!(final_delta.content.is_empty());
        assert!(final_delta.done);
        assert_eq!(accumulator.content(), "你好");
        assert_eq!(accumulator.usage().total_tokens, 5);
    }
}
