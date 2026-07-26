//! 去重策略：相同工具以相同参数被多次调用时，仅保留最新结果。
//!
//! 策略扫描 [`StrategyEntry`] 序列，对每对 `(name, arguments)` 记录出现位置。
//! 若同一键出现多次，除最后一次外，较早的调用及其结果生成一个
//! [`CompressionRange`]。仅处理助手条目只含单一工具调用的情形，避免拆开
//! 同一条目中的多个调用。

use std::collections::HashMap;

use crate::selective::CompressionRange;

use super::{StrategyEntry, StrategyEntryKind};

/// 对给定条目序列执行去重策略，返回可批量提交的压缩区间。
///
/// `topic` 用作所有生成区间的主题标签。区间按 `start` 升序排列且互不重叠。
/// `tokens_before` / `tokens_after` 置零，由宿主在提交前填充实际估值。
pub fn deduplication_strategy(entries: &[StrategyEntry], topic: &str) -> Vec<CompressionRange> {
    let mut call_positions: HashMap<(String, String), Vec<usize>> = HashMap::new();
    let mut result_positions: HashMap<String, usize> = HashMap::new();

    for entry in entries {
        match &entry.kind {
            StrategyEntryKind::ToolCall {
                id,
                name,
                arguments,
            } => {
                call_positions
                    .entry((name.clone(), arguments.clone()))
                    .or_default()
                    .push(entry.index);
                // 记录 tool_call_id -> call_index，用于后续配对结果。
                // 若同一 id 出现多次（不应发生），保留最后一次。
                result_positions.remove(id);
            }
            StrategyEntryKind::ToolResult { call_id, .. } => {
                result_positions.insert(call_id.clone(), entry.index);
            }
            StrategyEntryKind::Other => {}
        }
    }

    let mut ranges = Vec::new();
    for positions in call_positions.values() {
        if positions.len() < 2 {
            continue;
        }
        // 除最后一次外，均为可压缩的重复调用。
        for &call_index in &positions[..positions.len() - 1] {
            // 查找此调用的 tool_call_id 以配对结果。
            let Some(tool_call_id) = find_tool_call_id(entries, call_index) else {
                continue;
            };
            let Some(&result_index) = result_positions.get(&tool_call_id) else {
                continue;
            };
            if result_index <= call_index {
                continue;
            }
            ranges.push(CompressionRange {
                start: call_index,
                end: result_index,
                topic: topic.to_owned(),
                summary: format!(
                    "去重：工具调用 {tool_call_id} 与后续重复调用合并，仅保留最新结果。"
                ),
                tokens_before: 0,
                tokens_after: 0,
            });
        }
    }

    ranges.sort_by_key(|r| r.start);
    dedup::merge_overlapping(ranges)
}

fn find_tool_call_id(entries: &[StrategyEntry], index: usize) -> Option<String> {
    entries
        .iter()
        .find(|e| e.index == index)
        .and_then(|e| match &e.kind {
            StrategyEntryKind::ToolCall { id, .. } => Some(id.clone()),
            _ => None,
        })
}

mod dedup {
    use crate::selective::CompressionRange;

    /// 合并相邻或重叠的区间，保证返回序列互不重叠且按 `start` 升序排列。
    pub fn merge_overlapping(mut ranges: Vec<CompressionRange>) -> Vec<CompressionRange> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::{StrategyEntry, StrategyEntryKind};

    fn tool_call(index: usize, id: &str, name: &str, args: &str) -> StrategyEntry {
        StrategyEntry {
            index,
            kind: StrategyEntryKind::ToolCall {
                id: id.to_owned(),
                name: name.to_owned(),
                arguments: args.to_owned(),
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
    fn no_duplicates_returns_empty() {
        let entries = vec![
            tool_call(0, "a", "read", "{}"),
            tool_result(1, "a", false),
            tool_call(2, "b", "read", "{\"file\":\"x\"}"),
            tool_result(3, "b", false),
        ];
        let ranges = deduplication_strategy(&entries, "去重");
        assert!(ranges.is_empty());
    }

    #[test]
    fn duplicate_call_produces_range_for_earlier_occurrence() {
        let entries = vec![
            tool_call(0, "a", "read", "{}"),
            tool_result(1, "a", false),
            other(2),
            tool_call(3, "b", "read", "{}"),
            tool_result(4, "b", false),
        ];
        let ranges = deduplication_strategy(&entries, "去重");
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].end, 1);
    }

    #[test]
    fn triple_duplicate_keeps_only_last() {
        let entries = vec![
            tool_call(0, "a", "read", "{}"),
            tool_result(1, "a", false),
            tool_call(2, "b", "read", "{}"),
            tool_result(3, "b", false),
            tool_call(4, "c", "read", "{}"),
            tool_result(5, "c", false),
        ];
        let ranges = deduplication_strategy(&entries, "去重");
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].end, 1);
        assert_eq!(ranges[1].start, 2);
        assert_eq!(ranges[1].end, 3);
    }
}
