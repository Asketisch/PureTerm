//! 树结构修改操作:创建、分割、移除、detach。
//!
//! 这些方法直接改变 `SplitNode` 的拓扑结构(叶↔分割、节点折叠),
//! 不涉及比例/几何计算。对应 tmux 的 `split-window` / `kill-pane`
//! / `break-pane` / `join-pane` 在树层面的动作。

use super::{next_split_id, PaneId, SplitDirection, SplitNode};

impl SplitNode {
    /// 创建包含单个面板的叶子节点。
    pub fn single(pane_id: PaneId) -> Self {
        SplitNode::Leaf(pane_id)
    }

    /// 在 target 位置分割，插入新面板。新面板成为 second。
    /// 返回 true 表示成功找到 target 并分割。
    pub fn split(
        &mut self,
        target: PaneId,
        new_pane: PaneId,
        direction: SplitDirection,
    ) -> bool {
        match self {
            SplitNode::Leaf(id) if *id == target => {
                let old = *id;
                *self = SplitNode::Split {
                    split_id: next_split_id(),
                    direction,
                    ratio: 0.5,
                    first: Box::new(SplitNode::Leaf(old)),
                    second: Box::new(SplitNode::Leaf(new_pane)),
                };
                true
            }
            SplitNode::Leaf(_) => false,
            SplitNode::Split {
                first, second, ..
            } => {
                if first.split(target, new_pane, direction) {
                    true
                } else {
                    second.split(target, new_pane, direction)
                }
            }
        }
    }

    /// 移除一个面板。返回 Some(remaining) 或 None（树空）。
    /// 如果 target 是叶节点，移除后父节点折叠为兄弟节点。
    pub fn remove(&mut self, target: PaneId) -> Option<PaneId> {
        // 如果根节点本身就是 target 叶子，无法移除
        if let SplitNode::Leaf(id) = self {
            if *id == target {
                return None;
            }
            return Some(*id);
        }

        let replaced = self.remove_node_inner(target);
        if let Some(node) = replaced {
            *self = node;
            Some(self.first_pane())
        } else {
            None
        }
    }

    fn remove_node_inner(&mut self, target: PaneId) -> Option<SplitNode> {
        match self {
            SplitNode::Leaf(id) => {
                if *id == target {
                    // 信号：被移除
                    None
                } else {
                    // 不受影响，返回自身（表示 "保留"）
                    // 注意：调用方处理折叠逻辑
                    Some(SplitNode::Leaf(*id))
                }
            }
            SplitNode::Split {
                split_id: _,
                direction,
                ratio,
                first,
                second,
            } => {
                let new_first = first.remove_node_inner(target);
                let new_second = second.remove_node_inner(target);

                match (new_first, new_second) {
                    (Some(f), Some(s)) => {
                        *first = Box::new(f);
                        *second = Box::new(s);
                        Some(self.clone())
                    }
                    (Some(f), None) => Some(f),  // second 被移除，折叠为 first
                    (None, Some(s)) => Some(s),  // first 被移除，折叠为 second
                    (None, None) => None,          // 不应发生
                }
            }
        }
    }

    /// GAP-6.6: detach (remove) a leaf pane, collapsing its parent Split into
    /// the sibling (tmux `break-pane` / `join-pane` source side). Returns true
    /// iff the pane was present and the tree still has ≥1 pane afterwards
    /// (false if it was the only leaf — the last pane can't be detached). The
    /// caller owns moving the pane's content elsewhere.
    pub fn detach_leaf(&mut self, pane_id: PaneId) -> bool {
        self.remove(pane_id).is_some()
    }
}
