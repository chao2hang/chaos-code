//! Model-driven, request-only context compression.

use super::*;
use serde::Deserialize;
use std::collections::BTreeSet;
use xai_grok_compaction::selective::CompressionRange;
use xai_grok_compaction::strategies::{StrategyEntry, StrategyEntryKind};

pub(super) const COMPRESS_TOOL_NAME: &str = "compress";
const DCP_NUDGE_MARKER: &str = "[chaos-dcp]";

const NUDGE_EMERGENCY: &str = "⚠️ 上下文即将耗尽，请立即使用 compress 工具压缩对话历史。";
const NUDGE_REMINDER: &str = "💡 上下文使用率较高，建议使用 compress 工具压缩不再需要的内容。";
const NUDGE_ITERATION: &str = "📝 已进行多轮对话，考虑使用 compress 工具压缩历史以保持性能。";

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

/// 错误结果的保守判定：仅当内容以常见错误标记开头才算错误。
/// `contains("error")` 会把任何提及 error 的正常结果（grep 输出、
/// 文件内容）误判为错误，随后被自动清除策略以"错误清除"名义移除。
fn tool_result_looks_like_error(content: &str) -> bool {
    let head = content.trim_start();
    [
        "error", "Error", "ERROR", "错误", "failed", "Failed", "FAILED",
    ]
    .iter()
    .any(|marker| head.starts_with(marker))
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
                    is_error: tool_result_looks_like_error(&result.content),
                },
            },
            _ => StrategyEntry {
                index,
                kind: StrategyEntryKind::Other,
            },
        })
        .collect()
}

/// 最近 `turn_protection` 轮的起始索引。轮次边界按所有 user 条目
/// （真实输入与合成注入）计：目标循环等自主会话可能只有一条真实
/// 用户消息，若只按真实消息计，整个历史都会落入保护窗口，DCP 在
/// 最需要它的长自主会话中反而完全失效。
fn recent_turns_start(conversation: &[ConversationItem], turn_protection: usize) -> usize {
    let user_indices: Vec<usize> = conversation
        .iter()
        .enumerate()
        .filter_map(|(index, item)| matches!(item, ConversationItem::User(_)).then_some(index))
        .collect();
    user_indices
        .get(user_indices.len().saturating_sub(turn_protection))
        .copied()
        .unwrap_or(conversation.len())
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

    // 受保护工具的 call id 一次性收集，避免对每个结果重扫整个会话。
    let protected_call_ids: std::collections::HashSet<&str> = conversation
        .iter()
        .filter_map(|item| match item {
            ConversationItem::Assistant(assistant) => Some(assistant),
            _ => None,
        })
        .flat_map(|assistant| assistant.tool_calls.iter())
        .filter(|call| config.protected_tools.contains(&call.name))
        .map(|call| &*call.id)
        .collect();

    for (index, item) in conversation.iter().enumerate() {
        if let ConversationItem::Assistant(assistant) = item
            && assistant
                .tool_calls
                .iter()
                .any(|call| config.protected_tools.contains(&call.name))
        {
            protected.insert(index);
        }
        if let ConversationItem::ToolResult(result) = item
            && protected_call_ids.contains(result.tool_call_id.as_str())
        {
            protected.insert(index);
        }
        if !config.protected_tags.is_empty()
            && let ConversationItem::User(user) = item
        {
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

    let recent_start = recent_turns_start(conversation, config.turn_protection);
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

/// 依据上下文使用率与轮次计数决定提醒层级。所有层级都受
/// `turns_since_nudge` 限流（`nudge_force` 可绕过），注入后计数归零，
/// 避免同一提醒逐轮刷屏。计数在调用前已自增。
fn determine_nudge_tier(
    percent: f64,
    config: &super::super::dcp_config::DcpConfig,
    runtime: &super::super::dcp_config::DcpRuntimeState,
) -> Option<NudgeTier> {
    let turns_since_nudge = runtime
        .turns_since_nudge
        .load(std::sync::atomic::Ordering::Relaxed);
    if percent >= config.max_context_limit {
        // 紧急层：高频但不逐轮重发（注入归零 + 每轮自增，`>= 2` 即隔轮重发）。
        if turns_since_nudge >= 2 || config.nudge_force {
            return Some(NudgeTier::Emergency);
        }
        return None;
    }
    let rate_ok = turns_since_nudge >= config.nudge_frequency || config.nudge_force;
    if percent >= config.min_context_limit && rate_ok {
        return Some(NudgeTier::Reminder);
    }
    let turns_since_user = runtime
        .turns_since_user
        .load(std::sync::atomic::Ordering::Relaxed);
    if turns_since_user >= config.nudge_frequency * 2 && rate_ok {
        return Some(NudgeTier::Iteration);
    }
    None
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

        if dcp_config.strategies_enabled {
            self.run_automatic_strategies(&conversation, dcp_config)
                .await;
        }

        // 以最近一个 user 条目是否为真实输入判断本轮是否由用户驱动：
        // 真实输入归零计数；合成注入（目标循环、系统提醒）则累加。
        // 注意不能扫描整个会话——只要用户输入过一次就会永远归零。
        let last_user_is_real = conversation
            .iter()
            .rev()
            .find_map(|item| match item {
                ConversationItem::User(user) => Some(user.synthetic_reason.is_none()),
                _ => None,
            })
            .unwrap_or(false);
        if last_user_is_real {
            dcp_runtime
                .turns_since_user
                .store(0, std::sync::atomic::Ordering::Relaxed);
        } else {
            dcp_runtime
                .turns_since_user
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        dcp_runtime
            .turns_since_nudge
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let tier = determine_nudge_tier(percent, dcp_config, dcp_runtime);
        if let Some(tier) = tier {
            self.inject_nudge(tier, &conversation, dcp_config).await;
        }
    }

    async fn inject_nudge(
        &self,
        tier: NudgeTier,
        conversation: &[ConversationItem],
        config: &super::super::dcp_config::DcpConfig,
    ) {
        let message = match tier {
            NudgeTier::Emergency => NUDGE_EMERGENCY,
            NudgeTier::Reminder => NUDGE_REMINDER,
            NudgeTier::Iteration => NUDGE_ITERATION,
        };

        let recent_start = recent_turns_start(conversation, config.protected.turn_protection);

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

        self.compaction
            .dcp_runtime
            .turns_since_nudge
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    async fn run_automatic_strategies(
        &self,
        conversation: &[ConversationItem],
        config: &super::super::dcp_config::DcpConfig,
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

        // 策略读取的是未投影的原始会话，已压缩的区间每轮都会被原样
        // 重新生成；直接重提会让新块反复"消费"旧块，摘要继承链逐轮
        // 膨胀直至 NoTokenSavings 拒绝整批。先过滤已覆盖与受保护区间。
        let state = self.chat_state_handle.get_selective_compaction().await;
        let protected = compute_protected_indices(conversation, &config.protected);
        ranges.retain(|range| {
            let covered = state
                .active_blocks()
                .any(|block| block.start <= range.start && range.end <= block.end);
            let touches_protected = protected.range(range.start..=range.end).next().is_some();
            !covered && !touches_protected
        });

        // 逐条提交：批量提交是原子的，一条被拒（例如触及 chat-state
        // 侧追加的保护项）会连带丢弃其余合法区间。
        for range in ranges {
            let (start, end) = (range.start, range.end);
            if let Err(error) = self
                .chat_state_handle
                .apply_selective_compression(vec![range], protected.clone())
                .await
            {
                tracing::debug!(start, end, %error, "DCP 自动策略压缩区间被拒绝");
            }
        }
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
                        format!("compress 区间起始边界无法解析：{:?}", range.start),
                    )
                    .await;
            };
            let Some(end) = range.end.resolve(&id_to_index) else {
                return self
                    .handle_tool_not_executed(
                        &call.id,
                        &tool_call_id,
                        format!("compress 区间结束边界无法解析：{:?}", range.end),
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

        let protected = compute_protected_indices(&conversation, &self.compaction.dcp.protected);
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

#[cfg(test)]
mod dcp_helper_tests {
    use super::*;

    #[test]
    fn recent_turns_start_counts_synthetic_user_items_as_turns() {
        // 自主会话：一条真实输入，随后每轮由合成注入驱动。
        let conversation = vec![
            ConversationItem::user("vraie demande"),
            ConversationItem::tool_result("a", "r1"),
            ConversationItem::system_reminder("tour 2"),
            ConversationItem::tool_result("b", "r2"),
            ConversationItem::system_reminder("tour 3"),
            ConversationItem::tool_result("c", "r3"),
            ConversationItem::system_reminder("tour 4"),
            ConversationItem::tool_result("d", "r4"),
        ];
        // 保护最近 3 轮 → 从倒数第 3 个 user 条目（索引 2）开始；
        // 若只按真实用户消息计会得到 0，导致整个历史都被保护。
        assert_eq!(recent_turns_start(&conversation, 3), 2);
    }

    #[test]
    fn recent_turns_start_without_user_items_protects_nothing() {
        let conversation = vec![ConversationItem::tool_result("a", "r1")];
        assert_eq!(recent_turns_start(&conversation, 3), 1);
    }

    #[test]
    fn error_detection_requires_leading_marker() {
        assert!(tool_result_looks_like_error("Error: file not found"));
        assert!(tool_result_looks_like_error("  错误：无法读取文件"));
        assert!(tool_result_looks_like_error("failed to compile"));
        assert!(!tool_result_looks_like_error(
            "fn main() { /* handle error gracefully */ }"
        ));
        assert!(!tool_result_looks_like_error(
            "grep found 3 matches for 'error' in src/lib.rs"
        ));
    }
}

#[cfg(test)]
mod nudge_tier_tests {
    use super::*;
    use crate::session::dcp_config::{DcpConfig, DcpRuntimeState};
    use std::sync::atomic::Ordering;

    fn runtime(since_nudge: usize, since_user: usize) -> DcpRuntimeState {
        let state = DcpRuntimeState::default();
        state
            .turns_since_nudge
            .store(since_nudge, Ordering::Relaxed);
        state.turns_since_user.store(since_user, Ordering::Relaxed);
        state
    }

    #[test]
    fn reminder_fires_once_counter_reaches_frequency() {
        let config = DcpConfig::default();
        assert!(matches!(
            determine_nudge_tier(0.5, &config, &runtime(config.nudge_frequency, 0)),
            Some(NudgeTier::Reminder)
        ));
        assert!(
            determine_nudge_tier(0.5, &config, &runtime(config.nudge_frequency - 1, 0)).is_none()
        );
    }

    #[test]
    fn emergency_skips_the_turn_right_after_an_injection() {
        let config = DcpConfig::default();
        assert!(determine_nudge_tier(0.95, &config, &runtime(1, 0)).is_none());
        assert!(matches!(
            determine_nudge_tier(0.95, &config, &runtime(2, 0)),
            Some(NudgeTier::Emergency)
        ));
    }

    #[test]
    fn iteration_fires_for_long_autonomous_stretch() {
        let config = DcpConfig::default();
        let since_user = config.nudge_frequency * 2;
        assert!(matches!(
            determine_nudge_tier(0.1, &config, &runtime(config.nudge_frequency, since_user)),
            Some(NudgeTier::Iteration)
        ));
        assert!(determine_nudge_tier(0.1, &config, &runtime(0, since_user)).is_none());
    }

    #[test]
    fn nudge_force_bypasses_rate_limit() {
        let config = DcpConfig {
            nudge_force: true,
            ..DcpConfig::default()
        };
        assert!(matches!(
            determine_nudge_tier(0.5, &config, &runtime(0, 0)),
            Some(NudgeTier::Reminder)
        ));
        assert!(matches!(
            determine_nudge_tier(0.95, &config, &runtime(0, 0)),
            Some(NudgeTier::Emergency)
        ));
    }

    #[test]
    fn quiet_below_all_thresholds() {
        let config = DcpConfig::default();
        assert!(determine_nudge_tier(0.1, &config, &runtime(100, 0)).is_none());
    }
}
