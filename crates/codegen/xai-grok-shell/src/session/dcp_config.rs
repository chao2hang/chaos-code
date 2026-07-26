//! 动态上下文裁剪（DCP）配置。
//!
//! [`DcpConfig`] 控制三层提醒系统、自动策略与受保护内容。可从 TOML 配置
//! 文件反序列化，所有字段均有默认值。

use std::collections::HashSet;
use std::sync::atomic::AtomicUsize;

use serde::{Deserialize, Serialize};

/// 压缩策略选择。控制百分比阈值全量替换与 DCP 动态裁剪之间的组合。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompactionStrategy {
    /// 仅百分比阈值自动压缩（全量替换）。当前默认行为，向后兼容。
    #[default]
    Threshold,
    /// 仅 DCP 动态裁剪：模型驱动 compress 工具 + 提醒 + 自动策略，
    /// 不触发百分比阈值全量替换。
    Dynamic,
    /// 两者共存：DCP 做精细裁剪，阈值做兜底安全网。
    Both,
}

impl CompactionStrategy {
    /// 是否启用 DCP 动态裁剪子系统（compress 工具 + 提醒 + 自动策略）。
    pub fn dcp_active(self) -> bool {
        matches!(self, Self::Dynamic | Self::Both)
    }

    /// 是否保留百分比阈值自动压缩（全量替换）。
    pub fn threshold_active(self) -> bool {
        matches!(self, Self::Threshold | Self::Both)
    }
}

/// 受保护内容配置。受保护的会话条目不会被自动策略或模型驱动的
/// `compress` 工具压缩。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DcpProtectedConfig {
    /// 受保护的工具名称集合。这些工具的调用与结果不会被压缩。
    #[serde(default = "default_protected_tools")]
    pub protected_tools: HashSet<String>,
    /// 是否保护所有真实用户消息（非合成注入）。
    #[serde(default = "default_true")]
    pub protect_user_messages: bool,
    /// 最近 N 轮的条目受保护，不会被压缩。
    #[serde(default = "default_turn_protection")]
    pub turn_protection: usize,
    /// 受保护的内容标签。包含这些标签的消息不会被压缩。
    #[serde(default)]
    pub protected_tags: Vec<String>,
}

impl Default for DcpProtectedConfig {
    fn default() -> Self {
        Self {
            protected_tools: default_protected_tools(),
            protect_user_messages: default_true(),
            turn_protection: default_turn_protection(),
            protected_tags: Vec::new(),
        }
    }
}

fn default_protected_tools() -> HashSet<String> {
    ["write", "edit", "task", "skill", "todowrite"]
        .into_iter()
        .map(String::from)
        .collect()
}

fn default_true() -> bool {
    true
}

fn default_turn_protection() -> usize {
    3
}

/// DCP 主配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DcpConfig {
    /// 最低上下文使用率（0.0–1.0），超过后触发提醒层提醒。
    #[serde(default = "default_min_context_limit")]
    pub min_context_limit: f64,
    /// 最高上下文使用率（0.0–1.0），超过后触发紧急层提醒。
    #[serde(default = "default_max_context_limit")]
    pub max_context_limit: f64,
    /// 每隔多少轮触发一次提醒层提醒。
    #[serde(default = "default_nudge_frequency")]
    pub nudge_frequency: usize,
    /// 是否强制注入提醒（忽略去重检查）。
    #[serde(default)]
    pub nudge_force: bool,
    /// 是否启用自动策略（去重 + 错误清除）。
    #[serde(default = "default_true")]
    pub strategies_enabled: bool,
    /// 错误结果经过多少轮后可被清除策略移除。
    #[serde(default = "default_purge_errors_turns")]
    pub purge_errors_turns: usize,
    /// 受保护内容配置。
    #[serde(default)]
    pub protected: DcpProtectedConfig,
}

impl Default for DcpConfig {
    fn default() -> Self {
        Self {
            min_context_limit: default_min_context_limit(),
            max_context_limit: default_max_context_limit(),
            nudge_frequency: default_nudge_frequency(),
            nudge_force: false,
            strategies_enabled: default_true(),
            purge_errors_turns: default_purge_errors_turns(),
            protected: DcpProtectedConfig::default(),
        }
    }
}

fn default_min_context_limit() -> f64 {
    0.30
}

fn default_max_context_limit() -> f64 {
    0.90
}

fn default_nudge_frequency() -> usize {
    5
}

fn default_purge_errors_turns() -> usize {
    10
}

/// DCP 运行时状态（非持久化，跟踪提醒频率与用户轮次间隔）。
#[derive(Default)]
pub struct DcpRuntimeState {
    /// 自上次提醒层提醒以来的轮次数。
    pub turns_since_nudge: AtomicUsize,
    /// 自上次真实用户输入以来的轮次数。
    pub turns_since_user: AtomicUsize,
}

/// 稳定消息标识符，格式为 `m0001`（零填充至 4 位）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub String);

impl MessageId {
    pub fn from_index(index: usize) -> Self {
        Self(format!("m{:04}", index))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn to_index(&self) -> Option<usize> {
        self.0
            .strip_prefix('m')
            .and_then(|rest| rest.parse::<usize>().ok())
    }
}
