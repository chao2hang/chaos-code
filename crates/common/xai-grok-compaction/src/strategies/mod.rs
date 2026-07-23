//! 自动上下文压缩策略。
//!
//! 两种策略在模型驱动的 `compress` 工具之前运行：
//! - [`deduplication`] — 重复的工具调用（相同名称与参数）只保留最新结果。
//! - [`purge_errors`] — 已过期的错误工具调用结果将被移除。
//!
//! 每个策略返回 `Vec<CompressionRange>`，可批量提交给
//! `SelectiveState::compress`。策略通过 [`StrategyEntry`] 抽象会话条目，
//! 不依赖具体的会话类型。

pub mod deduplication;
pub mod purge_errors;

pub use deduplication::deduplication_strategy;
pub use purge_errors::purge_errors_strategy;

/// 策略扫描所需的会话条目描述。宿主将自身会话条目转换为此类型后传入策略。
#[derive(Debug, Clone)]
pub struct StrategyEntry {
    /// 在规范历史中的零基索引。
    pub index: usize,
    /// 条目分类。
    pub kind: StrategyEntryKind,
}

/// 策略关注的条目分类。
#[derive(Debug, Clone)]
pub enum StrategyEntryKind {
    /// 助手发起的工具调用。仅当助手条目仅包含此一个工具调用时才标记为
    /// `ToolCall`，以保证压缩区间不会拆开同一助手条目中的多个调用。
    ToolCall {
        /// 工具调用 ID。
        id: String,
        /// 工具名称。
        name: String,
        /// JSON 编码的参数。
        arguments: String,
    },
    /// 工具执行结果。
    ToolResult {
        /// 对应的工具调用 ID。
        call_id: String,
        /// 结果是否为错误。
        is_error: bool,
    },
    /// 其他条目（用户消息、系统消息、纯文本助手回复等）。
    Other,
}
