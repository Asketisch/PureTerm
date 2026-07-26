//! GAP-2.2 嵌套分屏补齐:Equalize / Swap 系列。
//!
//! 这些方法改变 `Split` 节点的比例或子树顺序,但不增删叶子:
//! - `equalize_all`:所有 ratio 强制 0.5(均分)
//! - `swap_with_sibling`:最浅 Split 翻转 first/second + ratio 翻转
//!   (tmux `swap-pane` 的「与兄弟互换」语义)
//! - `swap_by_ids`:任意两叶子互换 id(树结构/ratio 不动,内容视觉交换,
//!   对应 tmux `swap-pane -s A -t B`)

use super::{PaneId, SplitNode};

impl SplitNode {
    /// Equalize:把所有 Split 节点的 ratio 强制设为 0.5。
    /// 递归遍历整棵子树,叶子节点跳过。
    pub fn equalize_all(&mut self) {
        match self {
            SplitNode::Leaf(_) => {}
            SplitNode::Split { ratio, first, second, .. } => {
                *ratio = 0.5;
                first.equalize_all();
                second.equalize_all();
            }
        }
    }

    /// Swap:找到含 pane_id 的最浅 Split,交换 first/second 并翻转
    /// ratio = 1.0 - ratio(保证拖拽手感不跳)。
    /// 返回 true 表示成功(pane_id 必须在某个 Split 下,否则 false —
    /// 这种情况只发生在 root 是 Leaf 且唯一包含该 pane 的情形)。
    pub fn swap_with_sibling(&mut self, pane_id: PaneId) -> bool {
        match self {
            SplitNode::Leaf(_) => false,
            SplitNode::Split { ratio, first, second, .. } => {
                // 命中:子树直接包含 pane(任一子树是 Leaf 或包含该 pane)
                if first.contains(pane_id) && second.contains(pane_id) {
                    // 不应发生:两个子树都含同一 pane_id;按"非"处理,继续递归
                    // (实际上 PaneId 全局唯一,这分支永远不命中)
                } else if first.contains(pane_id) || second.contains(pane_id) {
                    std::mem::swap(first, second);
                    *ratio = 1.0 - *ratio;
                    return true;
                }
                // 未命中:递归到子树
                first.swap_with_sibling(pane_id) || second.swap_with_sibling(pane_id)
            }
        }
    }

    /// GAP-6.6: swap two panes by id (tmux `swap-pane -s A -t B`). Swaps the
    /// pane ids of the two leaves in place — the tree structure + ratios are
    /// preserved, and since rendering maps `PaneId → content`, the two panes'
    /// contents visually exchange position (exactly swap-pane's semantics).
    /// Both ids must be present; returns false (tree unchanged) otherwise.
    pub fn swap_by_ids(&mut self, src: PaneId, dst: PaneId) -> bool {
        if src == dst {
            return self.contains(src);
        }
        if !self.contains(src) || !self.contains(dst) {
            return false;
        }
        self.swap_ids_inner(src, dst);
        true
    }

    fn swap_ids_inner(&mut self, src: PaneId, dst: PaneId) {
        match self {
            SplitNode::Leaf(id) => {
                if *id == src {
                    *id = dst;
                } else if *id == dst {
                    *id = src;
                }
            }
            SplitNode::Split { first, second, .. } => {
                first.swap_ids_inner(src, dst);
                second.swap_ids_inner(src, dst);
            }
        }
    }
}
