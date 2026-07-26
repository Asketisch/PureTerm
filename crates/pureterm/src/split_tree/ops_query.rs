//! 只读查询与 id 重映射操作。
//!
//! 这些方法不改变树的几何,只回答「树里有什么」:
//! 叶子 id 列表、计数、包含性判断、几何相邻查找、DFS 序邻居。
//! 另含 GAP-5.6 跨重启恢复用的 `remap_pane_ids` /
//! `regenerate_split_ids`(语义上是「重写」,但只动 id 不动结构,故归此)。

use std::collections::HashMap;

use super::{next_split_id, PaneId, SplitDirection, SplitNode};

impl SplitNode {
    /// 获取第一个（最左/最上）面板 ID。
    pub fn first_pane(&self) -> PaneId {
        match self {
            SplitNode::Leaf(id) => *id,
            SplitNode::Split { first, .. } => first.first_pane(),
        }
    }

    /// 获取所有面板 ID。
    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut ids = Vec::new();
        self.collect_pane_ids(&mut ids);
        ids
    }

    fn collect_pane_ids(&self, ids: &mut Vec<PaneId>) {
        match self {
            SplitNode::Leaf(id) => ids.push(*id),
            SplitNode::Split { first, second, .. } => {
                first.collect_pane_ids(ids);
                second.collect_pane_ids(ids);
            }
        }
    }

    /// 面板数量。
    pub fn pane_count(&self) -> usize {
        match self {
            SplitNode::Leaf(_) => 1,
            SplitNode::Split { first, second, .. } => first.pane_count() + second.pane_count(),
        }
    }

    /// GAP-5.6 跨重启恢复：把叶节点的 PaneId 按映射替换为「新分配」的 id。
    /// 持久化树里的 id 来自上一个进程，必须重映射到本进程新分配的 id，
    /// 否则会与本进程的全局自增计数器冲突。映射里没有的叶节点保持原 id。
    pub fn remap_pane_ids(&mut self, mapping: &HashMap<PaneId, PaneId>) {
        match self {
            SplitNode::Leaf(id) => {
                if let Some(new_id) = mapping.get(id) {
                    *id = *new_id;
                }
            }
            SplitNode::Split { first, second, .. } => {
                first.remap_pane_ids(mapping);
                second.remap_pane_ids(mapping);
            }
        }
    }

    /// GAP-5.6 跨重启恢复：重新分配每个 Split 节点的 `split_id`。
    /// 反序列化得到的 split_id 来自上一个进程，可能与本进程的全局计数器冲突，
    /// 故全部用 `next_split_id()` 重发。
    pub fn regenerate_split_ids(&mut self) {
        match self {
            SplitNode::Leaf(_) => {}
            SplitNode::Split { split_id, first, second, .. } => {
                *split_id = next_split_id();
                first.regenerate_split_ids();
                second.regenerate_split_ids();
            }
        }
    }

    /// 检查子树是否包含给定面板。
    pub fn contains(&self, pane_id: PaneId) -> bool {
        match self {
            SplitNode::Leaf(id) => *id == pane_id,
            SplitNode::Split { first, second, .. } => {
                first.contains(pane_id) || second.contains(pane_id)
            }
        }
    }

    /// 查找给定方向上的相邻面板。
    pub fn find_adjacent(
        &self,
        pane_id: PaneId,
        direction: SplitDirection,
    ) -> Option<PaneId> {
        match self {
            SplitNode::Leaf(_) => None,
            SplitNode::Split {
                direction: dir,
                first,
                second,
                ..
            } => {
                if *dir == direction {
                    if first.contains(pane_id) {
                        return Some(second.first_pane());
                    } else if second.contains(pane_id) {
                        return Some(first.last_pane());
                    }
                }
                // 垂直方向或未命中：递归搜索
                first
                    .find_adjacent(pane_id, direction)
                    .or_else(|| second.find_adjacent(pane_id, direction))
            }
        }
    }

    /// 获取最后一个（最右/最下）面板 ID。
    fn last_pane(&self) -> PaneId {
        match self {
            SplitNode::Leaf(id) => *id,
            SplitNode::Split { second, .. } => second.last_pane(),
        }
    }

    /// Focus Next/Prev:返回 DFS 先序中相对于 pane_id 偏移 delta 的 pane(wrap)。
    /// delta = +1 → 下一个,delta = -1 → 上一个;|delta|>1 同样支持。
    /// 返回 None 表示 pane_id 不在树中,或树为空。
    pub fn neighbor_in_order(&self, pane_id: PaneId, delta: i32) -> Option<PaneId> {
        let ids = self.pane_ids();
        if ids.is_empty() {
            return None;
        }
        let pos = ids.iter().position(|&id| id == pane_id)?;
        let len = ids.len() as i32;
        // 防御性归一化:rem_euclid 处理负数 wrap
        let next = ((pos as i32 + delta).rem_euclid(len)) as usize;
        Some(ids[next])
    }
}
