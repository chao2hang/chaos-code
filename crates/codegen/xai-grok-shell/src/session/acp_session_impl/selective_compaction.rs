//! Model-driven, request-only context compression.

use super::*;
use serde::Deserialize;
use std::collections::BTreeSet;
use xai_grok_compaction::selective::CompressionRange;
use xai_grok_compaction::strategies::{StrategyEntry, StrategyEntryKind};

pub(super) const COMPRESS_TOOL_NAME: &str = "compress";
const DCP_NUDGE_MARKER: &str = "[chaos-dcp]";

const NUDGE_EMERGENCY: &str =
    "⚠️ 上下文即将耗尽，请立即使用 compress 工具压缩对话历史。";
const NUDGE_REMINDER: &str =
    "💡 上下文使用率较高，建议使用 compress 工具压缩不再需要的内容。";
const NUDGE_ITERATION: &str =
    "📝 已进行多轮对话，考虑使用 compress 工具压缩历史以保持性能。";

#[derive(Debug, Deserialize)]
struct CompressArgs {
    topic: String,
    ranges: Vec<CompressRangeArgs>,
}

#[derive(Debug, Deserialize)]
struct CompressRangeArgs {
    start: RangeBoundary,
    end: RangeBoundary,
    summary: String,
    #[serde(default)]
    topic: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RangeBoundary {
    Index(usize),
    MessageId(String),
}

impl RangeBoundary {
    fn resolve(&self, id_to_index: &std::collections::HashMap<String, usize>) -> Option<usize> {
        match self {
            Self::Index(i) => Some(*i),
            Self::MessageId(id) => id_to_index.get(id).copied(),
        }
    }
}

pub(super) fn compress_tool_definition() -> ToolDefinition {
    ToolDefinition::function(
        COMPRESS_TOOL_NAME,
        Some(
            "压缩较早且已完成的上下文区间。仅替换发送给模型的请求视图，不修改会话原始记录。区间使用系统提醒中给出的零基历史索引或消息 ID（m0001 格式）；必须完整覆盖工具调用及其结果，且不得包含受保护项。",
        ),
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["topic", "ranges"],
            "properties": {
                "topic": { "type": "string", "description": "本次压缩的总主题" },
                "ranges": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["start", "end", "summary"],
                        "properties": {
                            "start": {
                                "oneOf": [
                                    { "type": "integer", "minimum": 0 },
                                    { "type": "string", "pattern": "^m[0-9]{4}$" }
                                ]
                            },
                            "end": {
                                "oneOf": [
                                    { "type": "integer", "minimum": 0 },
                                    { "type": "string", "pattern": "^m[0-9]{4}$" }
                                ]
                            },
                            "topic": { "type": "string" },
                            "summary": { "type": "string", "description": "保留决策、文件、符号、错误、结果和未完成事项的自包含摘要" }
                        }
                    }
                }
            }
        }),
    )
}

fn item_outline(index: usize, item: &ConversationItem) -> String {
    let (kind, text) = match item {
        ConversationItem::System(system) => ("system", system.content.as_ref()),
        ConversationItem::User(user) => {
            let kind = if user.synthetic_reason.is_some() {
                "synthetic-user"
            } else {
                "user"
            };
            let text = user
                .content
                .iter()
                .find_map(|part| match part {
                    ContentPart::Text { text } => Some(text.as_ref()),
                    ContentPart::Image { .. } => None,
                })
                .unwrap_or("[image]");
            (kind, text)
        }
        ConversationItem::Assistant(assistant) => {
            if assistant.tool_calls.is_empty() {
                ("assistant", assistant.content.as_ref())
            } else {
                let names = assistant
                    .tool_calls
                    .iter()
                    .map(|call| call.name.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                return format!("[{index}] assistant-tool: {names}");
            }
        }
        ConversationItem::ToolResult(result) => ("tool-result", result.content.as_ref()),
        ConversationItem::BackendToolCall(_) => ("backend-tool", "[backend tool call]"),
        ConversationItem::Reasoning(_) => ("reasoning", "[reasoning]"),
    };
    let single_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let preview: String = single_line.chars().take(120).collect();
    format!("[{index}] {kind}: {preview}")
}

fn build_message_id_map(
    conversation: &[ConversationItem],
) -> std::collections::HashMap<String, usize> {
    let mut map = std::collections::HashMap::with_capacity(conversation.len());
    for (index, _) in conversation.iter().enumerate() {
        let id = super::super::dcp_config::MessageId::from_index(index);
        map.insert(id.0, index);
    }
    map
}

fn build_strategy_entries(conversation: &[ConversationItem]) -> Vec<StrategyEntry> {
    conversation
        .iter()
        .enumerate()
        .map(|(index, item)| match item {
            ConversationItem::Assistant(assistant) => {
                if assistant.tool_calls.len() == 1 {
                    let call = &assistant.tool_calls[0];
                    StrategyEntry {
                        index,
                        kind: StrategyEntryKind::ToolCall {
                            id: call.id.to_string(),
                            name: call.name.clone(),
                            arguments: call.arguments.to_string(),
                        },
                    }
                } else {
                    StrategyEntry {
                        index,
                        kind: StrategyEntryKind::Other,
                    }
                }
            }
            ConversationItem::ToolResult(result) => StrategyEntry {
                index,
                kind: StrategyEntryKind::ToolResult {
                    call_id: result.tool_call_id.clone(),
                    is_error: result.content.contains("error")
                        || result.content.contains("Error")
                        || result.content.contains("错误"),
                },
            },
            _ => StrategyEntry {
                index,
                kind: StrategyEntryKind::Other,
            },
        })
        .collect()
}

fn compute_protected_indices(
    conversation: &[ConversationItem],
    config: &super::super::dcp_config::DcpProtectedConfig,
) -> BTreeSet<usize> {
    let mut protected = BTreeSet::new();

    let real_user_indices: Vec<usize> = conversation
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            ConversationItem::User(user) if user.synthetic_reason.is_none() => Some(index),
            _ => None,
        })
        .collect();

    if config.protect_user_messages {
        for index in &real_user_indices {
            protected.insert(*index);
        }
    }

    for (index, item) in conversation.iter().enumerate() {
        if let ConversationItem::Assistant(assistant) = item {
            for call in &assistant.tool_calls {
                if config.protected_tools.contains(&call.name) {
                    protected.insert(index);
                }
            }
        }
        if let ConversationItem::ToolResult(result) = item {
            for call_idx in conversation.iter().enumerate() {
                if let ConversationItem::Assistant(assistant) = call_idx.1 {
                    if assistant.tool_calls.iter().any(|c| {
                        &*c.id == &result.tool_call_id && config.protected_tools.contains(&c.name)
                    }) {
                        protected.insert(index);
                    }
                }
            }
        }
        if !config.protected_tags.is_empty() {
            if let ConversationItem::User(user) = item {
                let has_tag = user.content.iter().any(|part| {
                    config.protected_tags.iter().any(|tag| match part {
                        ContentPart::Text { text } => text.contains(tag.as_str()),
                        _ => false,
                    })
                });
                if has_tag {
                    protected.insert(index);
                }
            }
        }
    }

    let recent_start = real_user_indices
        .get(real_user_indices.len().saturating_sub(config.turn_protection))
        .copied()
        .unwrap_or(conversation.len());
    for index in recent_start..conversation.len() {
        protected.insert(index);
    }

    protected
}

fn context_percent(estimated: u64, context_window: u64) -> f64 {
    if context_window == 0 {
        0.0
    } else {
        (estimated as f64 / context_window as f64).min(1.0)
    }
}

impl SessionActor {
    pub(super) async fn maybe_inject_selective_compaction_nudge(&self) {
        let dcp_config = &self.compaction.dcp;
        let dcp_runtime = &self.compaction.dcp_runtime;

        let Some(config) = self.chat_state_handle.get_sampling_config().await else {
            return;
        };
        let estimated = self.chat_state_handle.get_estimated_total_tokens().await;
        let context_window = config.context_window.get();
        let percent = context_percent(estimated, context_window);

        let conversation = self.chat_state_handle.get_conversation().await;
        let id_to_index = build_message_id_map(&conversation);

        if dcp_config.strategies_enabled {
            self.run_automatic_strategies(&conversation, dcp_config, &id_to_index)
                .await;
        }

        let has_real_user = conversation.iter().any(|item| {
            matches!(
                item,
                ConversationItem::User(user) if user.synthetic_reason.is_none()
            )
        });
        if has_real_user {
            dcp_runtime
                .turns_since_user
                .store(0, std::sync::atomic::Ordering::Relaxed);
        } else {
            dcp_runtime
                .turns_since_user
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        let tier = self.determine_nudge_tier(percent, dcp_config, dcp_runtime);
        if let Some(tier) = tier {
            self.inject_nudge(tier, &conversation, dcp_config, &id_to_index)
                .await;
        }
    }

    fn determine_nudge_tier(
        &self,
        percent: f64,
        config: &super::super::dcp_config::DcpConfig,
        runtime: &super::super::dcp_config::DcpRuntimeState,
    ) -> Option<NudgeTier> {
        if percent >= config.max_context_limit {
            return Some(NudgeTier::Emergency);
        }
        if percent >= config.min_context_limit {
            let turns = runtime
                .turns_since_nudge
                .load(std::sync::atomic::Ordering::Relaxed);
            if turns >= config.nudge_frequency || config.nudge_force {
                return Some(NudgeTier::Reminder);
            }
        }
        let turns_since_user = runtime
            .turns_since_user
            .load(std::sync::atomic::Ordering::Relaxed);
        if turns_since_user >= config.nudge_frequency * 2 {
            return Some(NudgeTier::Iteration);
        }
        None
    }

    async fn inject_nudge(
        &self,
        tier: NudgeTier,
        conversation: &[ConversationItem],
        config: &super::super::dcp_config::DcpConfig,
        id_to_index: &std::collections::HashMap<String, usize>,
    ) {
        let state = self.chat_state_handle.get_selective_compaction().await;
        if state.active_blocks().next().is_some() && !config.nudge_force {
            return;
        }

        let already_injected = conversation.iter().any(|item| {
            matches!(
                item,
                ConversationItem::User(user) if user.content.iter().any(|part| {
                    matches!(part, ContentPart::Text { text } if text.contains(DCP_NUDGE_MARKER))
                })
            )
        });
        if already_injected && !config.nudge_force {
            return;
        }

        let (message, update_nudge_counter) = match tier {
            NudgeTier::Emergency => (NUDGE_EMERGENCY, false),
            NudgeTier::Reminder => (NUDGE_REMINDER, true),
            NudgeTier::Iteration => (NUDGE_ITERATION, false),
        };

        let real_users: Vec<usize> = conversation
            .iter()
            .enumerate()
            .filter_map(|(index, item)| match item {
                ConversationItem::User(user) if user.synthetic_reason.is_none() => Some(index),
                _ => None,
            })
            .collect();
        let recent_start = real_users
            .get(real_users.len().saturating_sub(config.protected.turn_protection))
            .copied()
            .unwrap_or(conversation.len());

        let outline = conversation
            .iter()
            .enumerate()
            .filter(|(index, item)| {
                *index < recent_start
                    && !matches!(
                        item,
                        ConversationItem::System(_) | ConversationItem::User(_)
                    )
            })
            .take(160)
            .map(|(index, item)| {
                let id = super::super::dcp_config::MessageId::from_index(index);
                format!("{} {}", id.as_str(), item_outline(index, item))
            })
            .collect::<Vec<_>>()
            .join("\n");

        if outline.is_empty() && !matches!(tier, NudgeTier::Emergency) {
            return;
        }

        self.push_system_reminder(&format!(
            "{DCP_NUDGE_MARKER} {message}\n\n可用消息 ID 与历史概览：\n{outline}"
        ));

        if update_nudge_counter {
            self.compaction
                .dcp_runtime
                .turns_since_nudge
                .store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }

    async fn run_automatic_strategies(
        &self,
        conversation: &[ConversationItem],
        config: &super::super::dcp_config::DcpConfig,
        _id_to_index: &std::collections::HashMap<String, usize>,
    ) {
        let entries = build_strategy_entries(conversation);
        let mut ranges = Vec::new();
        ranges.extend(xai_grok_compaction::strategies::deduplication_strategy(
            &entries,
            "自动去重",
        ));
        ranges.extend(xai_grok_compaction::strategies::purge_errors_strategy(
            &entries,
            config.purge_errors_turns,
            "自动错误清除",
        ));
        if ranges.is_empty() {
            return;
        }
        let protected =
            compute_protected_indices(conversation, &config.protected);
        let _ = self
            .chat_state_handle
            .apply_selective_compression(ranges, protected)
            .await;
    }

    pub(super) async fn execute_compress_tool(
        &self,
        call: &crate::sampling::types::ToolCallResponse,
    ) -> Result<(), acp::Error> {
        let tool_call_id = acp::ToolCallId::new(Arc::from(call.id.clone()));
        let raw = match serde_json::from_str::<serde_json::Value>(&call.function.arguments) {
            Ok(raw) => raw,
            Err(error) => {
                return self
                    .handle_tool_not_executed(
                        &call.id,
                        &tool_call_id,
                        format!("compress 参数不是有效 JSON：{error}"),
                    )
                    .await;
            }
        };
        self.send_update(
            acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                tool_call_id.clone(),
                acp::ToolCallUpdateFields::new()
                    .title(Some("压缩上下文".to_string()))
                    .kind(Some(acp::ToolKind::Other))
                    .status(Some(acp::ToolCallStatus::InProgress))
                    .raw_input(Some(raw.clone())),
            )),
            None,
        )
        .await;
        let args: CompressArgs = match serde_json::from_value(raw) {
            Ok(args) => args,
            Err(error) => {
                return self
                    .handle_tool_not_executed(
                        &call.id,
                        &tool_call_id,
                        format!("compress 参数不符合工具定义：{error}"),
                    )
                    .await;
            }
        };

        let conversation = self.chat_state_handle.get_conversation().await;
        let id_to_index = build_message_id_map(&conversation);

        let mut ranges = Vec::with_capacity(args.ranges.len());
        for range in &args.ranges {
            let Some(start) = range.start.resolve(&id_to_index) else {
                return self
                    .handle_tool_not_executed(
                        &call.id,
                        &tool_call_id,
                        format!(
                            "compress 区间起始边界无法解析：{:?}",
                            range.start
                        ),
                    )
                    .await;
            };
            let Some(end) = range.end.resolve(&id_to_index) else {
                return self
                    .handle_tool_not_executed(
                        &call.id,
                        &tool_call_id,
                        format!(
                            "compress 区间结束边界无法解析：{:?}",
                            range.end
                        ),
                    )
                    .await;
            };
            ranges.push(CompressionRange {
                start,
                end,
                topic: range.topic.clone().unwrap_or_else(|| args.topic.clone()),
                summary: range.summary.clone(),
                tokens_before: 0,
                tokens_after: 0,
            });
        }

        let protected = compute_protected_indices(
            &conversation,
            &self.compaction.dcp.protected,
        );
        let result = self
            .chat_state_handle
            .apply_selective_compression(ranges, protected)
            .await;
        let message = match result {
            Ok(ids) => {
                let state = self.chat_state_handle.get_selective_compaction().await;
                format!(
                    "已创建 {} 个上下文压缩块（ID：{}），当前累计净节省约 {} Token。原始会话记录未修改。",
                    ids.len(),
                    ids.iter()
                        .map(|id| id.0.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    state.total_tokens_saved(),
                )
            }
            Err(error) => {
                return self
                    .handle_tool_not_executed(
                        &call.id,
                        &tool_call_id,
                        format!("上下文压缩被拒绝：{error}"),
                    )
                    .await;
            }
        };
        self.send_update(
            acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                tool_call_id,
                acp::ToolCallUpdateFields::new()
                    .status(Some(acp::ToolCallStatus::Completed))
                    .content(Some(vec![acp::ToolCallContent::from(
                        acp::ContentBlock::Text(acp::TextContent::new(message.clone())),
                    )])),
            )),
            None,
        )
        .await;
        self.chat_state_handle
            .push_tool_result(ConversationItem::tool_result(call.id.clone(), message));
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum NudgeTier {
    Emergency,
    Reminder,
    Iteration,
}
