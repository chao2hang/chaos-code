//! 错误清除策略：工具调用产生错误结果且已超过指定轮次后，移除该调用及其结果。
//!
//! 策略扫描 [`StrategyEntry`] 序列，对每个错误结果查找对应的工具调用。
//! 若从该结果到会话末尾的轮次间隔超过 `min_turns_after`，则生成一个
//! [`CompressionRange`] 覆盖调用与结果。仅处理助手条目只含单一工具调用的情形。

use crate::selective::CompressionRange;

use super::{StrategyEntry, StrategyEntryKind};

/// 对给定条目序列执行错误清除策略，返回可批量提交的压缩区间。
///
/// `min_turns_after` 指定错误结果之后至少需要经过多少个用户轮次才可清除。
/// `topic` 用作所有生成区间的主题标签。区间按 `start` 升序排列且互不重叠。
/// `tokens_before` / `tokens_after` 置零，由宿主在提交前填充实际估值。
pub fn purge_errors_strategy(
    entries: &[StrategyEntry],
    min_turns_after: usize,
    topic: &str,
) -> Vec<CompressionRange> {
    if min_turns_after == 0 {
        return Vec::new();
    }

    let mut call_index_by_id: HashMap<String, usize> = HashMap::new();
    for entry in entries {
        if let StrategyEntryKind::ToolCall { id, .. } = &entry.kind {
            call_index_by_id.insert(id.clone(), entry.index);
        }
    }

    let mut ranges = Vec::new();
    for entry in entries {
        let StrategyEntryKind::ToolResult {
            call_id,
            is_error: true,
        } = &entry.kind
        else {
            continue;
        };
        let result_index = entry.index;
        let Some(&call_index) = call_index_by_id.get(call_id) else {
            continue;
        };
        if call_index >= result_index {
            continue;
        }
        // 统计结果之后的用户轮次（Other 条目中由宿主标记为用户消息的）。
        // 宿主通过在 entries 中将用户消息标记为 Other 来参与计数；
        // 此处用 result_index 之后的条目数作为近似轮次间隔。
        let turns_after = entries.iter().filter(|e| e.index > result_index).count();
        if turns_after < min_turns_after {
            continue;
        }
        ranges.push(CompressionRange {
            start: call_index,
            end: result_index,
            topic: topic.to_owned(),
            summary: format!(
                "错误清除：工具调用 {call_id} 产生错误结果且已超过 {min_turns_after} 轮，予以移除。"
            ),
            tokens_before: 0,
            tokens_after: 0,
        });
    }

    ranges.sort_by_key(|r| r.start);
    merge_overlapping(ranges)
}

use std::collections::HashMap;

/// 合并相邻或重叠的区间。
fn merge_overlapping(mut ranges: Vec<CompressionRange>) -> Vec<CompressionRange> {
    if ranges.is_empty() {
        return ranges;
    }
    ranges.sort_by_key(|r| r.start);
    let mut merged: Vec<CompressionRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(last) = merged.last_mut() {
            if range.start <= last.end {
                last.end = last.end.max(range.end);
                continue;
            }
        }
        merged.push(range);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::{StrategyEntry, StrategyEntryKind};

    fn tool_call(index: usize, id: &str, name: &str) -> StrategyEntry {
        StrategyEntry {
            index,
            kind: StrategyEntryKind::ToolCall {
                id: id.to_owned(),
                name: name.to_owned(),
                arguments: "{}".to_owned(),
            },
        }
    }

    fn tool_result(index: usize, call_id: &str, is_error: bool) -> StrategyEntry {
        StrategyEntry {
            index,
            kind: StrategyEntryKind::ToolResult {
                call_id: call_id.to_owned(),
                is_error,
            },
        }
    }

    fn other(index: usize) -> StrategyEntry {
        StrategyEntry {
            index,
            kind: StrategyEntryKind::Other,
        }
    }

    #[test]
    fn no_errors_returns_empty() {
        let entries = vec![
            tool_call(0, "a", "read"),
            tool_result(1, "a", false),
            other(2),
            other(3),
        ];
        let ranges = purge_errors_strategy(&entries, 1, "错误清除");
        assert!(ranges.is_empty());
    }

    #[test]
    fn recent_error_not_purged() {
        let entries = vec![tool_call(0, "a", "write"), tool_result(1, "a", true)];
        let ranges = purge_errors_strategy(&entries, 5, "错误清除");
        assert!(ranges.is_empty());
    }

    #[test]
    fn old_error_is_purged() {
        let entries = vec![
            tool_call(0, "a", "write"),
            tool_result(1, "a", true),
            other(2),
            other(3),
            other(4),
            other(5),
        ];
        let ranges = purge_errors_strategy(&entries, 3, "错误清除");
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].end, 1);
    }

    #[test]
    fn zero_min_turns_returns_empty() {
        let entries = vec![tool_call(0, "a", "write"), tool_result(1, "a", true)];
        let ranges = purge_errors_strategy(&entries, 0, "错误清除");
        assert!(ranges.is_empty());
    }
}
