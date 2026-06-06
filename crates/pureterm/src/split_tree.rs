//! 可变的二叉分割面板树。
//!
//! 每个内部节点存储分割方向和比例，叶节点存储 PaneId。
//! 支持递归嵌套（水平+垂直分割）。

use std::collections::HashMap;
use std::sync::Arc;

use crate::terminal_context::TerminalContext;

/// 面板的像素空间矩形。
#[derive(Debug, Clone, Copy, Default)]
pub struct PaneRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// 可见分割线宽度(像素)。
pub const DIVIDER_WIDTH: f32 = 4.0;
/// 分割线拖拽的命中测试区域宽度(更大以便于抓取)。
pub const DIVIDER_HIT_WIDTH: f32 = 8.0;
/// 比例最小值。
pub const MIN_RATIO: f32 = 0.1;
/// 比例最大值。
pub const MAX_RATIO: f32 = 0.9;

/// 终端面板的唯一标识符。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(pub u64);

impl PaneId {
    /// 全局自增计数器生成新的 PaneId。
    pub fn next() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        PaneId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

/// 面板：持有独立的终端上下文。
pub struct Pane {
    pub id: PaneId,
    pub terminal_ctx: Arc<TerminalContext>,
    pub title: String,
}

/// 分割方向。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitDirection {
    /// 左右分割（垂直分割线）。
    Horizontal,
    /// 上下分割（水平分割线）。
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

/// 分割树节点。
#[derive(Debug, Clone)]
pub enum SplitNode {
    /// 叶子节点：持有单个面板 ID。
    Leaf(PaneId),
    /// 内部分割节点。
    Split {
        direction: SplitDirection,
        /// first 占比 [MIN_RATIO, MAX_RATIO]。
        ratio: f32,
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
}

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

    /// 计算所有面板的像素矩形（含分割线扣除）。
    /// 返回 HashMap<PaneId, PaneRect>。
    pub fn compute_layout(
        &self,
        x: f32,
        y: f32,
        total_w: f32,
        total_h: f32,
    ) -> HashMap<PaneId, PaneRect> {
        let mut result = HashMap::new();
        self.layout_recursive(x, y, total_w, total_h, &mut result);
        result
    }

    fn layout_recursive(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        out: &mut HashMap<PaneId, PaneRect>,
    ) {
        match self {
            SplitNode::Leaf(id) => {
                out.insert(*id, PaneRect { x, y, w, h });
            }
            SplitNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                match direction {
                    SplitDirection::Horizontal => {
                        // 左右分割：分割线竖直，消耗宽度
                        let usable_w = w - DIVIDER_WIDTH;
                        let w1 = usable_w * ratio;
                        let w2 = usable_w - w1;
                        first.layout_recursive(x, y, w1, h, out);
                        second.layout_recursive(
                            x + w1 + DIVIDER_WIDTH,
                            y,
                            w2,
                            h,
                            out,
                        );
                    }
                    SplitDirection::Vertical => {
                        // 上下分割：分割线水平，消耗高度
                        let usable_h = h - DIVIDER_WIDTH;
                        let h1 = usable_h * ratio;
                        let h2 = usable_h - h1;
                        first.layout_recursive(x, y, w, h1, out);
                        second.layout_recursive(
                            x,
                            y + h1 + DIVIDER_WIDTH,
                            w,
                            h2,
                            out,
                        );
                    }
                }
            }
        }
    }

    /// 计算所有分割线的位置信息。
    /// 返回 Vec<DividerRect>，每个分割线对应一个。
    pub fn compute_dividers(
        &self,
        x: f32,
        y: f32,
        total_w: f32,
        total_h: f32,
    ) -> Vec<DividerRect> {
        let mut result = Vec::new();
        self.dividers_recursive(x, y, total_w, total_h, &mut result);
        result
    }

    /// 查找包含 `target_pane` 的直接父级 Split 节点对应的分割线。
    /// 返回 None 表示 target_pane 是根节点（没有分割线）。
    pub fn compute_divider_for_pane(
        &self,
        target_pane: PaneId,
        x: f32,
        y: f32,
        total_w: f32,
        total_h: f32,
    ) -> Option<DividerRect> {
        self.divider_for_pane_recursive(target_pane, x, y, total_w, total_h)
    }

    fn divider_for_pane_recursive(
        &self,
        target_pane: PaneId,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Option<DividerRect> {
        match self {
            SplitNode::Leaf(_) => None,
            SplitNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                if first.contains(target_pane) {
                    // target_pane 在 first 子树中
                    // 计算此 Split 的分割线位置
                    match direction {
                        SplitDirection::Horizontal => {
                            let usable_w = w - DIVIDER_WIDTH;
                            let w1 = usable_w * ratio;
                            let div_x = x + w1;
                            Some(DividerRect {
                                rect: PaneRect {
                                    x: div_x,
                                    y,
                                    w: DIVIDER_WIDTH,
                                    h,
                                },
                                direction: *direction,
                                usable_size: usable_w,
                            })
                        }
                        SplitDirection::Vertical => {
                            let usable_h = h - DIVIDER_WIDTH;
                            let h1 = usable_h * ratio;
                            let div_y = y + h1;
                            Some(DividerRect {
                                rect: PaneRect {
                                    x,
                                    y: div_y,
                                    w,
                                    h: DIVIDER_WIDTH,
                                },
                                direction: *direction,
                                usable_size: usable_h,
                            })
                        }
                    }
                } else if second.contains(target_pane) {
                    // target_pane 在 second 子树中，递归
                    let (sub_x, sub_y, sub_w, sub_h) = match direction {
                        SplitDirection::Horizontal => {
                            let usable_w = w - DIVIDER_WIDTH;
                            let w1 = usable_w * ratio;
                            (x + w1 + DIVIDER_WIDTH, y, usable_w - w1, h)
                        }
                        SplitDirection::Vertical => {
                            let usable_h = h - DIVIDER_WIDTH;
                            let h1 = usable_h * ratio;
                            (x, y + h1 + DIVIDER_WIDTH, w, usable_h - h1)
                        }
                    };
                    second.divider_for_pane_recursive(target_pane, sub_x, sub_y, sub_w, sub_h)
                } else {
                    None
                }
            }
        }
    }

    fn dividers_recursive(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        out: &mut Vec<DividerRect>,
    ) {
        match self {
            SplitNode::Leaf(_) => {}
            SplitNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                match direction {
                    SplitDirection::Horizontal => {
                        let usable_w = w - DIVIDER_WIDTH;
                        let w1 = usable_w * ratio;
                        let div_x = x + w1;
                        out.push(DividerRect {
                            rect: PaneRect {
                                x: div_x,
                                y,
                                w: DIVIDER_WIDTH,
                                h,
                            },
                            direction: *direction,
                            usable_size: usable_w,
                        });
                        first.dividers_recursive(x, y, w1, h, out);
                        second.dividers_recursive(
                            div_x + DIVIDER_WIDTH,
                            y,
                            usable_w - w1,
                            h,
                            out,
                        );
                    }
                    SplitDirection::Vertical => {
                        let usable_h = h - DIVIDER_WIDTH;
                        let h1 = usable_h * ratio;
                        let div_y = y + h1;
                        out.push(DividerRect {
                            rect: PaneRect {
                                x,
                                y: div_y,
                                w,
                                h: DIVIDER_WIDTH,
                            },
                            direction: *direction,
                            usable_size: usable_h,
                        });
                        first.dividers_recursive(x, y, w, h1, out);
                        second.dividers_recursive(
                            x,
                            div_y + DIVIDER_WIDTH,
                            w,
                            usable_h - h1,
                            out,
                        );
                    }
                }
            }
        }
    }

    /// 查找给定坐标下的分割线（用于拖拽）。
    pub fn divider_at_point(
        &self,
        px: f32,
        py: f32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Option<DividerRect> {
        // 先检查本层分割线
        let dividers = self.compute_dividers(x, y, w, h);
        let half_hit = (DIVIDER_HIT_WIDTH - DIVIDER_WIDTH) / 2.0;
        for div in &dividers {
            let hit_x = div.rect.x - half_hit;
            let hit_y = div.rect.y - half_hit;
            let hit_w = DIVIDER_HIT_WIDTH;
            let hit_h = if div.direction == SplitDirection::Horizontal {
                div.rect.h + half_hit * 2.0
            } else {
                DIVIDER_HIT_WIDTH
            };
            // 对于垂直分割线（Horizontal），检查 px 范围
            // 对于水平分割线（Vertical），检查 py 范围
            let hit = match div.direction {
                SplitDirection::Horizontal => {
                    px >= hit_x && px < hit_x + hit_w && py >= div.rect.y && py < div.rect.y + div.rect.h
                }
                SplitDirection::Vertical => {
                    py >= hit_y && py < hit_y + hit_w && px >= div.rect.x && px < div.rect.x + div.rect.w
                }
            };
            if hit {
                return Some(*div);
            }
        }
        None
    }

    /// 查找给定坐标下的面板。
    pub fn pane_at_point(&self, px: f32, py: f32, x: f32, y: f32, w: f32, h: f32) -> Option<PaneId> {
        let layout = self.compute_layout(x, y, w, h);
        for (id, rect) in &layout {
            if px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h {
                return Some(*id);
            }
        }
        None
    }
}

/// 分割线的位置信息。
#[derive(Debug, Clone, Copy)]
pub struct DividerRect {
    /// 可见分割线矩形。
    pub rect: PaneRect,
    /// 分割线方向。
    pub direction: SplitDirection,
    /// 分割方向上的可用像素总量（不含分割线）。
    pub usable_size: f32,
}

impl DividerRect {
    /// 根据像素偏移量计算 ratio 变化量。
    pub fn delta_to_ratio(&self, delta_px: f32) -> f32 {
        if self.usable_size > 0.0 {
            delta_px / self.usable_size
        } else {
            0.0
        }
    }
}
