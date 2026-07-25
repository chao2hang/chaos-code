//! Selective context projection primitives.
//!
//! Canonical history remains untouched. Compression blocks describe closed
//! ranges that are replaced only in the request sent to a model. A newer block
//! may consume older blocks completely contained in its range; their summaries
//! are inherited so nested compression cannot silently lose information.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stable identifier for a selective compression block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlockId(pub u64);

/// Requested inclusive range in canonical-history coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressionRange {
    pub start: usize,
    pub end: usize,
    pub topic: String,
    pub summary: String,
    pub tokens_before: u64,
    pub tokens_after: u64,
}

/// Persistable block metadata. `consumed_blocks` forms the compaction DAG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectiveBlock {
    pub id: BlockId,
    pub start: usize,
    pub end: usize,
    pub topic: String,
    pub summary: String,
    pub active: bool,
    pub parent: Option<BlockId>,
    pub consumed_blocks: Vec<BlockId>,
    pub tokens_before: u64,
    pub tokens_after: u64,
}

impl SelectiveBlock {
    pub fn tokens_saved(&self) -> u64 {
        self.tokens_before.saturating_sub(self.tokens_after)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SelectiveError {
    #[error("压缩区间 {start}..={end} 无效，历史长度为 {history_len}")]
    InvalidRange {
        start: usize,
        end: usize,
        history_len: usize,
    },
    #[error("压缩区间 {start}..={end} 与前一区间重叠或顺序错误")]
    OverlappingRange { start: usize, end: usize },
    #[error("压缩区间 {start}..={end} 包含受保护的历史项 {index}")]
    ProtectedItem {
        start: usize,
        end: usize,
        index: usize,
    },
    #[error("压缩区间 {start}..={end} 只覆盖了已有压缩块 {block:?} 的一部分")]
    PartialBlockOverlap {
        start: usize,
        end: usize,
        block: BlockId,
    },
    #[error("压缩主题和摘要不能为空")]
    EmptyContent,
    #[error("压缩区间 {start}..={end} 会拆开工具调用 {tool_call_id} 及其结果")]
    ToolPairSplit {
        start: usize,
        end: usize,
        tool_call_id: String,
    },
    #[error(
        "压缩区间 {start}..={end} 没有净 Token 收益（压缩前 {tokens_before}，压缩后 {tokens_after}）"
    )]
    NoTokenSavings {
        start: usize,
        end: usize,
        tokens_before: u64,
        tokens_after: u64,
    },
}

/// Selective compression state owned by the host session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectiveState {
    next_block_id: u64,
    blocks: Vec<SelectiveBlock>,
}

impl SelectiveState {
    pub fn blocks(&self) -> &[SelectiveBlock] {
        &self.blocks
    }

    pub fn active_blocks(&self) -> impl Iterator<Item = &SelectiveBlock> {
        self.blocks.iter().filter(|block| block.active)
    }

    pub fn total_tokens_saved(&self) -> u64 {
        self.active_blocks().map(SelectiveBlock::tokens_saved).sum()
    }

    /// Validate and atomically commit one or more ordered compression ranges.
    pub fn compress(
        &mut self,
        history_len: usize,
        ranges: Vec<CompressionRange>,
        protected_items: &BTreeSet<usize>,
    ) -> Result<Vec<BlockId>, SelectiveError> {
        validate_ranges(history_len, &ranges, protected_items, &self.blocks)?;

        let mut staged = self.clone();
        let mut created = Vec::with_capacity(ranges.len());
        for range in ranges {
            let id = BlockId(staged.next_block_id);
            staged.next_block_id = staged.next_block_id.saturating_add(1);

            let consumed: Vec<BlockId> = staged
                .blocks
                .iter()
                .filter(|block| {
                    block.active && range.start <= block.start && block.end <= range.end
                })
                .map(|block| block.id)
                .collect();

            let inherited = staged
                .blocks
                .iter()
                .filter(|block| consumed.contains(&block.id))
                .map(|block| format!("[继承自压缩块 {}]\n{}", block.id.0, block.summary))
                .collect::<Vec<_>>()
                .join("\n\n");
            let inherited_tokens = staged
                .blocks
                .iter()
                .filter(|block| consumed.contains(&block.id))
                .map(|block| block.tokens_after)
                .sum::<u64>();
            let summary = if inherited.is_empty() {
                range.summary
            } else {
                format!("{}\n\n{}", range.summary, inherited)
            };
            let tokens_after = range.tokens_after.saturating_add(inherited_tokens);
            if tokens_after >= range.tokens_before {
                return Err(SelectiveError::NoTokenSavings {
                    start: range.start,
                    end: range.end,
                    tokens_before: range.tokens_before,
                    tokens_after,
                });
            }

            for block in &mut staged.blocks {
                if consumed.contains(&block.id) {
                    block.active = false;
                    block.parent = Some(id);
                }
            }
            staged.blocks.push(SelectiveBlock {
                id,
                start: range.start,
                end: range.end,
                topic: range.topic,
                summary,
                active: true,
                parent: None,
                consumed_blocks: consumed,
                tokens_before: range.tokens_before,
                tokens_after,
            });
            created.push(id);
        }
        *self = staged;
        Ok(created)
    }

    /// Build a request-only projection without mutating canonical history.
    pub fn project<T: Clone>(
        &self,
        canonical: &[T],
        mut summary_item: impl FnMut(&SelectiveBlock) -> T,
    ) -> Vec<T> {
        let mut active: Vec<_> = self.active_blocks().collect();
        active.sort_by_key(|block| block.start);
        let mut output = Vec::with_capacity(canonical.len());
        let mut cursor = 0;
        for block in active {
            if block.start < cursor || block.end >= canonical.len() {
                continue;
            }
            output.extend_from_slice(&canonical[cursor..block.start]);
            output.push(summary_item(block));
            cursor = block.end + 1;
        }
        output.extend_from_slice(&canonical[cursor..]);
        output
    }

    /// Full compaction replaces canonical history, invalidating all ordinals.
    pub fn reset(&mut self) {
        self.blocks.clear();
        self.next_block_id = 0;
    }
}

fn validate_ranges(
    history_len: usize,
    ranges: &[CompressionRange],
    protected_items: &BTreeSet<usize>,
    blocks: &[SelectiveBlock],
) -> Result<(), SelectiveError> {
    let mut previous_end = None;
    for range in ranges {
        if range.start > range.end || range.end >= history_len {
            return Err(SelectiveError::InvalidRange {
                start: range.start,
                end: range.end,
                history_len,
            });
        }
        if range.topic.trim().is_empty() || range.summary.trim().is_empty() {
            return Err(SelectiveError::EmptyContent);
        }
        if previous_end.is_some_and(|end| range.start <= end) {
            return Err(SelectiveError::OverlappingRange {
                start: range.start,
                end: range.end,
            });
        }
        if let Some(index) = protected_items.range(range.start..=range.end).next() {
            return Err(SelectiveError::ProtectedItem {
                start: range.start,
                end: range.end,
                index: *index,
            });
        }
        for block in blocks.iter().filter(|block| block.active) {
            let intersects = range.start <= block.end && block.start <= range.end;
            let contains = range.start <= block.start && block.end <= range.end;
            if intersects && !contains {
                return Err(SelectiveError::PartialBlockOverlap {
                    start: range.start,
                    end: range.end,
                    block: block.id,
                });
            }
        }
        previous_end = Some(range.end);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(start: usize, end: usize, summary: &str) -> CompressionRange {
        CompressionRange {
            start,
            end,
            topic: "旧任务".to_owned(),
            summary: summary.to_owned(),
            tokens_before: 1_000,
            tokens_after: 100,
        }
    }

    #[test]
    fn projection_keeps_canonical_history_unchanged() {
        let canonical = vec![
            "a".to_owned(),
            "b".to_owned(),
            "c".to_owned(),
            "d".to_owned(),
        ];
        let mut state = SelectiveState::default();
        state
            .compress(4, vec![range(1, 2, "摘要")], &BTreeSet::new())
            .unwrap();
        let projected = state.project(&canonical, |block| block.summary.clone());
        assert_eq!(projected, vec!["a", "摘要", "d"]);
        assert_eq!(canonical, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn nested_compression_inherits_old_summary_and_builds_dag() {
        let mut state = SelectiveState::default();
        let first = state
            .compress(8, vec![range(1, 2, "第一次摘要")], &BTreeSet::new())
            .unwrap()[0];
        let parent = state
            .compress(8, vec![range(0, 4, "更大区间")], &BTreeSet::new())
            .unwrap()[0];
        let old = state
            .blocks()
            .iter()
            .find(|block| block.id == first)
            .unwrap();
        let new = state
            .blocks()
            .iter()
            .find(|block| block.id == parent)
            .unwrap();
        assert!(!old.active);
        assert_eq!(old.parent, Some(parent));
        assert_eq!(new.consumed_blocks, vec![first]);
        assert!(new.summary.contains("第一次摘要"));
    }

    #[test]
    fn invalid_batch_is_atomic() {
        let mut state = SelectiveState::default();
        let before = state.clone();
        let error = state
            .compress(
                6,
                vec![range(0, 2, "一"), range(2, 4, "二")],
                &BTreeSet::new(),
            )
            .unwrap_err();
        assert!(matches!(error, SelectiveError::OverlappingRange { .. }));
        assert_eq!(state, before);
    }

    #[test]
    fn non_beneficial_summary_is_rejected_atomically() {
        let mut state = SelectiveState::default();
        let before = state.clone();
        let mut no_savings = range(0, 1, "过长摘要");
        no_savings.tokens_after = no_savings.tokens_before;
        assert!(matches!(
            state.compress(3, vec![no_savings], &BTreeSet::new()),
            Err(SelectiveError::NoTokenSavings { .. })
        ));
        assert_eq!(state, before);
    }

    #[test]
    fn protected_items_and_partial_nested_overlap_are_rejected() {
        let mut state = SelectiveState::default();
        let protected = BTreeSet::from([2]);
        assert!(matches!(
            state.compress(6, vec![range(1, 3, "摘要")], &protected),
            Err(SelectiveError::ProtectedItem { index: 2, .. })
        ));
        state
            .compress(6, vec![range(1, 3, "摘要")], &BTreeSet::new())
            .unwrap();
        assert!(matches!(
            state.compress(6, vec![range(2, 4, "部分覆盖")], &BTreeSet::new()),
            Err(SelectiveError::PartialBlockOverlap { .. })
        ));
    }
}
