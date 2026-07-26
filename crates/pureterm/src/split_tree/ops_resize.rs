//! 分割比例(ratio)调整操作。
//!
//! 这些方法不改变树的拓扑,只调整某个 `Split` 节点的 `ratio`
//! 字段(对应 tmux `resize-pane` 的拖拽/键控比例变化)。
//! 所有写入都通过 `[MIN_RATIO, MAX_RATIO]` 钳制,避免几何退化。

use super::{MAX_RATIO, MIN_RATIO, PaneId, SplitNode};

impl SplitNode {
    /// 调整包含 pane_id 的最近祖先 Split 的 ratio。
    pub fn resize_ratio(&mut self, pane_id: PaneId, delta: f32) {
        match self {
            SplitNode::Leaf(_) => {}
            SplitNode::Split {
                ratio, first, second, ..
            } => {
                if first.contains(pane_id) || second.contains(pane_id) {
                    *ratio = (*ratio + delta).clamp(MIN_RATIO, MAX_RATIO);
                } else {
                    first.resize_ratio(pane_id, delta);
                    second.resize_ratio(pane_id, delta);
                }
            }
        }
    }

    /// 通过 split_id 找到并更新对应 Split 节点的 ratio。
    /// 返回 true 表示成功找到并更新。
    pub fn update_split_ratio_by_id(&mut self, split_id: u64, new_ratio: f32) -> bool {
        match self {
            SplitNode::Leaf(_) => false,
            SplitNode::Split { split_id: id, ratio, first, second, .. } => {
                if *id == split_id {
                    *ratio = new_ratio.clamp(MIN_RATIO, MAX_RATIO);
                    true
                } else {
                    first.update_split_ratio_by_id(split_id, new_ratio)
                        || second.update_split_ratio_by_id(split_id, new_ratio)
                }
            }
        }
    }
}
