//! 不可变的持久化分割面板树。
//!
//! 使用 `im` crate 实现树结构的结构性共享,
//! 并使用普通 `Vec` 来维护有序的面板列表。

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 终端面板的唯一标识符。
pub type PaneId = Uuid;

/// 分割方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    /// 左右面板。
    Horizontal,
    /// 上下面板。
    Vertical,
}

impl SplitDirection {
    pub fn opposite(&self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

/// 分割树中的一个节点。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SplitNode {
    /// 持有单个终端面板的叶子节点。
    Leaf { pane_id: PaneId },
    /// 内部分割节点。
    Split {
        direction: SplitDirection,
        /// 分隔符从左/上边缘的位置 (0.0–1.0)。
        ratio: f32,
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
}

/// 不可变的持久化分割面板树。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitTree {
    root: Option<SplitNode>,
    /// 所有面板 ID 的有序列表。
    panes: Vec<PaneId>,
}

impl SplitTree {
    /// 创建新的空树。
    pub fn new() -> Self {
        Self {
            root: None,
            panes: Vec::new(),
        }
    }

    /// 创建带有单个面板的树。
    pub fn single(pane_id: PaneId) -> Self {
        Self {
            root: Some(SplitNode::Leaf { pane_id }),
            panes: vec![pane_id],
        }
    }

    /// 在给定方向上分割现有面板。
    /// 返回新树。
    pub fn split(&self, existing: PaneId, new_pane: PaneId, direction: SplitDirection) -> Self {
        let mut new_panes = self.panes.clone();
        new_panes.push(new_pane);
        let new_root = self.root.clone().map(|node| {
            Self::split_node(node, existing, new_pane, direction)
        });
        Self {
            root: new_root,
            panes: new_panes,
        }
    }

    fn split_node(node: SplitNode, target: PaneId, new_pane: PaneId, direction: SplitDirection) -> SplitNode {
        match node {
            SplitNode::Leaf { pane_id } if pane_id == target => {
                SplitNode::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(SplitNode::Leaf { pane_id }),
                    second: Box::new(SplitNode::Leaf { pane_id: new_pane }),
                }
            }
            SplitNode::Leaf { pane_id } => SplitNode::Leaf { pane_id },
            SplitNode::Split { direction: d, ratio, first, second } => {
                if Self::contains(&first, target) {
                    let new_first = Self::split_node(*first, target, new_pane, direction);
                    SplitNode::Split {
                        direction: d,
                        ratio,
                        first: Box::new(new_first),
                        second,
                    }
                } else {
                    let new_second = Self::split_node(*second, target, new_pane, direction);
                    SplitNode::Split {
                        direction: d,
                        ratio,
                        first,
                        second: Box::new(new_second),
                    }
                }
            }
        }
    }

    /// 移除一个面板。
    pub fn remove(&self, pane_id: PaneId) -> Self {
        let mut new_panes = self.panes.clone();
        new_panes.retain(|id| *id != pane_id);
        let new_root = self.root.clone().and_then(|node| Self::remove_node(node, pane_id));
        Self {
            root: new_root,
            panes: new_panes,
        }
    }

    fn remove_node(node: SplitNode, target: PaneId) -> Option<SplitNode> {
        match node {
            SplitNode::Leaf { pane_id } if pane_id == target => None,
            SplitNode::Leaf { pane_id } => Some(SplitNode::Leaf { pane_id }),
            SplitNode::Split { direction, ratio, first, second } => {
                let new_first = Self::remove_node(*first, target);
                let new_second = Self::remove_node(*second, target);

                match (new_first, new_second) {
                    (Some(f), Some(s)) => {
                        Some(SplitNode::Split { direction, ratio, first: Box::new(f), second: Box::new(s) })
                    }
                    (Some(f), None) => Some(f),
                    (None, Some(s)) => Some(s),
                    (None, None) => None,
                }
            }
        }
    }

    /// 调整包含 `pane_id` 的分割的比率。
    pub fn resize(&self, pane_id: PaneId, delta: f32) -> Self {
        let new_root = self.root.clone().map(|node| {
            Self::resize_node(node, pane_id, delta)
        });
        Self {
            root: new_root,
            panes: self.panes.clone(),
        }
    }

    fn resize_node(node: SplitNode, target: PaneId, delta: f32) -> SplitNode {
        match node {
            SplitNode::Leaf { .. } => node,
            SplitNode::Split { direction, ratio, first, second } => {
                if Self::contains(&first, target) || Self::contains(&second, target) {
                    let new_ratio = (ratio + delta).clamp(0.1, 0.9);
                    return SplitNode::Split {
                        direction,
                        ratio: new_ratio,
                        first,
                        second,
                    };
                }
                // 递归到两个子节点
                let new_first = Self::resize_node(*first, target, delta);
                let new_second = Self::resize_node(*second, target, delta);
                SplitNode::Split {
                    direction,
                    ratio,
                    first: Box::new(new_first),
                    second: Box::new(new_second),
                }
            }
        }
    }

    /// 检查子树是否包含给定面板。
    fn contains(node: &SplitNode, pane_id: PaneId) -> bool {
        match node {
            SplitNode::Leaf { pane_id: id } => *id == pane_id,
            SplitNode::Split { first, second, .. } => {
                Self::contains(first, pane_id) || Self::contains(second, pane_id)
            }
        }
    }

    /// 按插入顺序获取所有面板 ID。
    pub fn panes(&self) -> &[PaneId] {
        &self.panes
    }

    /// 获取根节点。
    pub fn root(&self) -> Option<&SplitNode> {
        self.root.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// 查找给定方向上的相邻面板。
    pub fn adjacent(&self, pane_id: PaneId, direction: SplitDirection) -> Option<PaneId> {
        self.root.as_ref().and_then(|node| Self::find_adjacent(node, pane_id, direction))
    }

    fn find_adjacent(node: &SplitNode, target: PaneId, direction: SplitDirection) -> Option<PaneId> {
        match node {
            SplitNode::Leaf { .. } => None,
            SplitNode::Split { direction: dir, first, second, .. } => {
                if *dir == direction {
                    // 相同方向:导航到正确的子节点
                    if Self::contains(first, target) {
                        return Self::rightmost_pane(second);
                    } else if Self::contains(second, target) {
                        return Self::leftmost_pane(first);
                    }
                }
                // 垂直方向:搜索两个子节点
                Self::find_adjacent(first, target, direction)
                    .or_else(|| Self::find_adjacent(second, target, direction))
            }
        }
    }

    fn leftmost_pane(node: &SplitNode) -> Option<PaneId> {
        match node {
            SplitNode::Leaf { pane_id } => Some(*pane_id),
            SplitNode::Split { first, .. } => Self::leftmost_pane(first),
        }
    }

    fn rightmost_pane(node: &SplitNode) -> Option<PaneId> {
        match node {
            SplitNode::Leaf { pane_id } => Some(*pane_id),
            SplitNode::Split { second, .. } => Self::rightmost_pane(second),
        }
    }

    /// 根据总视口大小计算所有面板的像素矩形。
    pub fn compute_layout(&self, width: f32, height: f32) -> Vec<(PaneId, (f32, f32, f32, f32))> {
        let mut result = Vec::new();
        if let Some(ref root) = self.root {
            Self::layout_node(root, 0.0, 0.0, width, height, &mut result);
        }
        result
    }

    fn layout_node(
        node: &SplitNode,
        x: f32, y: f32, w: f32, h: f32,
        out: &mut Vec<(PaneId, (f32, f32, f32, f32))>,
    ) {
        match node {
            SplitNode::Leaf { pane_id } => {
                out.push((*pane_id, (x, y, w, h)));
            }
            SplitNode::Split { direction, ratio, first, second } => {
                match direction {
                    SplitDirection::Horizontal => {
                        let w1 = w * ratio;
                        Self::layout_node(first, x, y, w1, h, out);
                        Self::layout_node(second, x + w1, y, w - w1, h, out);
                    }
                    SplitDirection::Vertical => {
                        let h1 = h * ratio;
                        Self::layout_node(first, x, y, w, h1, out);
                        Self::layout_node(second, x, y + h1, w, h - h1, out);
                    }
                }
            }
        }
    }
}

impl Default for SplitTree {
    fn default() -> Self {
        Self::new()
    }
}
