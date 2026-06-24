//! 可变的二叉分割面板树。
//!
//! 每个内部节点存储分割方向和比例，叶节点存储 PaneId。
//! 支持递归嵌套（水平+垂直分割）。

use std::collections::HashMap;
use std::sync::Arc;

use crate::browser::BrowserContext;
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
pub const DIVIDER_WIDTH: f32 = 6.0;
/// 分割线拖拽的命中测试区域宽度(更大以便于抓取)。
pub const DIVIDER_HIT_WIDTH: f32 = 8.0;
/// 比例最小值。
pub const MIN_RATIO: f32 = 0.1;
/// 比例最大值。
pub const MAX_RATIO: f32 = 0.9;

/// 终端面板的唯一标识符。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PaneId(pub u64);

impl PaneId {
    /// 全局自增计数器生成新的 PaneId。
    pub fn next() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        PaneId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

/// Pane 的内容载体 —— 终端或内置浏览器(WebView)。
///
/// GAP-3.5:在终端之外引入 `Web` 变体,使一个 pane 能容纳 WebView。
/// 用枚举而非 trait:仅两种变体,每个 match 点更直接,且快照序列化
/// 需要判别式(`session_restore::PaneSnapshot::kind`)。
pub enum PaneContent {
    /// 终端(本地 PTY / SSH PTY)。
    Terminal(Arc<TerminalContext>),
    /// 内置浏览器(GAP-3.5 WebView)。
    Web(Arc<BrowserContext>),
}

/// 面板:持有 pane 内容(终端或浏览器)+ 在分割树中的位置。
pub struct Pane {
    pub id: PaneId,
    pub content: PaneContent,
    pub title: String,
}

impl Pane {
    /// 若是终端 pane,返回其终端上下文。
    pub fn terminal(&self) -> Option<&Arc<TerminalContext>> {
        match &self.content {
            PaneContent::Terminal(t) => Some(t),
            _ => None,
        }
    }

    /// 若是浏览器 pane,返回其浏览器上下文。
    pub fn web(&self) -> Option<&Arc<BrowserContext>> {
        match &self.content {
            PaneContent::Web(w) => Some(w),
            _ => None,
        }
    }

    /// 是否为浏览器 pane。
    pub fn is_web(&self) -> bool {
        matches!(self.content, PaneContent::Web(_))
    }
}

/// 分割方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SplitNode {
    /// 叶子节点：持有单个面板 ID。
    Leaf(PaneId),
    /// 内部分割节点。
    Split {
        /// Split 节点唯一标识符。
        split_id: u64,
        direction: SplitDirection,
        /// first 占比 [MIN_RATIO, MAX_RATIO]。
        ratio: f32,
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
}

/// 全局自增的 SplitId 计数器。
fn next_split_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
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

    /// 通过 split_id 找到对应 Split 节点的分割线位置。
    pub fn compute_divider_by_split_id(
        &self,
        target_split_id: u64,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Option<DividerRect> {
        self.divider_by_split_id_recursive(target_split_id, x, y, w, h)
    }

    fn divider_by_split_id_recursive(
        &self,
        target_split_id: u64,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Option<DividerRect> {
        match self {
            SplitNode::Leaf(_) => None,
            SplitNode::Split { split_id, direction, ratio, first, second, .. } => {
                if *split_id == target_split_id {
                    // 找到目标，返回此 Split 的分割线位置
                    match direction {
                        SplitDirection::Horizontal => {
                            let usable_w = w - DIVIDER_HIT_WIDTH;
                            let w1 = usable_w * ratio;
                            let div_x = x + w1;
                            Some(DividerRect {
                                rect: PaneRect {
                                    x: div_x,
                                    y,
                                    w: DIVIDER_HIT_WIDTH,
                                    h,
                                },
                                direction: *direction,
                                usable_size: usable_w,
                            })
                        }
                        SplitDirection::Vertical => {
                            let usable_h = h - DIVIDER_HIT_WIDTH;
                            let h1 = usable_h * ratio;
                            let div_y = y + h1;
                            Some(DividerRect {
                                rect: PaneRect {
                                    x,
                                    y: div_y,
                                    w,
                                    h: DIVIDER_HIT_WIDTH,
                                },
                                direction: *direction,
                                usable_size: usable_h,
                            })
                        }
                    }
                } else {
                    // 递归到子树
                    first.divider_by_split_id_recursive(target_split_id, x, y, w, h)
                        .or_else(|| {
                            // 计算 second 子树的 bounds
                            let (sub_x, sub_y, sub_w, sub_h) = match direction {
                                SplitDirection::Horizontal => {
                                    let usable_w = w - DIVIDER_HIT_WIDTH;
                                    let w1 = usable_w * ratio;
                                    (x + w1 + DIVIDER_HIT_WIDTH, y, usable_w - w1, h)
                                }
                                SplitDirection::Vertical => {
                                    let usable_h = h - DIVIDER_HIT_WIDTH;
                                    let h1 = usable_h * ratio;
                                    (x, y + h1 + DIVIDER_HIT_WIDTH, w, usable_h - h1)
                                }
                            };
                            second.divider_by_split_id_recursive(
                                target_split_id, sub_x, sub_y, sub_w, sub_h,
                            )
                        })
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
                split_id: _,
                direction,
                ratio,
                first,
                second,
            } => {
                match direction {
                    SplitDirection::Horizontal => {
                        // 左右分割：分割线竖直，消耗宽度
                        // 使用 DIVIDER_HIT_WIDTH 以匹配 Iced view 中 divider widget 的实际占用宽度
                        let usable_w = w - DIVIDER_HIT_WIDTH;
                        let w1 = usable_w * ratio;
                        let w2 = usable_w - w1;
                        first.layout_recursive(x, y, w1, h, out);
                        second.layout_recursive(
                            x + w1 + DIVIDER_HIT_WIDTH,
                            y,
                            w2,
                            h,
                            out,
                        );
                    }
                    SplitDirection::Vertical => {
                        // 上下分割：分割线水平，消耗高度
                        let usable_h = h - DIVIDER_HIT_WIDTH;
                        let h1 = usable_h * ratio;
                        let h2 = usable_h - h1;
                        first.layout_recursive(x, y, w, h1, out);
                        second.layout_recursive(
                            x,
                            y + h1 + DIVIDER_HIT_WIDTH,
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
                split_id: _,
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
                            let usable_w = w - DIVIDER_HIT_WIDTH;
                            let w1 = usable_w * ratio;
                            let div_x = x + w1;
                            Some(DividerRect {
                                rect: PaneRect {
                                    x: div_x,
                                    y,
                                    w: DIVIDER_HIT_WIDTH,
                                    h,
                                },
                                direction: *direction,
                                usable_size: usable_w,
                            })
                        }
                        SplitDirection::Vertical => {
                            let usable_h = h - DIVIDER_HIT_WIDTH;
                            let h1 = usable_h * ratio;
                            let div_y = y + h1;
                            Some(DividerRect {
                                rect: PaneRect {
                                    x,
                                    y: div_y,
                                    w,
                                    h: DIVIDER_HIT_WIDTH,
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
                            let usable_w = w - DIVIDER_HIT_WIDTH;
                            let w1 = usable_w * ratio;
                            (x + w1 + DIVIDER_HIT_WIDTH, y, usable_w - w1, h)
                        }
                        SplitDirection::Vertical => {
                            let usable_h = h - DIVIDER_HIT_WIDTH;
                            let h1 = usable_h * ratio;
                            (x, y + h1 + DIVIDER_HIT_WIDTH, w, usable_h - h1)
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
                split_id: _,
                direction,
                ratio,
                first,
                second,
            } => {
                match direction {
                    SplitDirection::Horizontal => {
                        let usable_w = w - DIVIDER_HIT_WIDTH;
                        let w1 = usable_w * ratio;
                        let div_x = x + w1;
                        out.push(DividerRect {
                            rect: PaneRect {
                                x: div_x,
                                y,
                                w: DIVIDER_HIT_WIDTH,
                                h,
                            },
                            direction: *direction,
                            usable_size: usable_w,
                        });
                        first.dividers_recursive(x, y, w1, h, out);
                        second.dividers_recursive(
                            div_x + DIVIDER_HIT_WIDTH,
                            y,
                            usable_w - w1,
                            h,
                            out,
                        );
                    }
                    SplitDirection::Vertical => {
                        let usable_h = h - DIVIDER_HIT_WIDTH;
                        let h1 = usable_h * ratio;
                        let div_y = y + h1;
                        out.push(DividerRect {
                            rect: PaneRect {
                                x,
                                y: div_y,
                                w,
                                h: DIVIDER_HIT_WIDTH,
                            },
                            direction: *direction,
                            usable_size: usable_h,
                        });
                        first.dividers_recursive(x, y, w, h1, out);
                        second.dividers_recursive(
                            x,
                            div_y + DIVIDER_HIT_WIDTH,
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
        for div in &dividers {
            // div.rect 现在使用 DIVIDER_HIT_WIDTH 作为宽度/高度，即完整的命中区域
            let hit = match div.direction {
                SplitDirection::Horizontal => {
                    px >= div.rect.x && px < div.rect.x + div.rect.w
                        && py >= div.rect.y && py < div.rect.y + div.rect.h
                }
                SplitDirection::Vertical => {
                    py >= div.rect.y && py < div.rect.y + div.rect.h
                        && px >= div.rect.x && px < div.rect.x + div.rect.w
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

    // ---- GAP-2.2 嵌套分屏补齐:Equalize / Swap / Focus Next-Prev ----

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

    /// GAP-6.6: detach (remove) a leaf pane, collapsing its parent Split into
    /// the sibling (tmux `break-pane` / `join-pane` source side). Returns true
    /// iff the pane was present and the tree still has ≥1 pane afterwards
    /// (false if it was the only leaf — the last pane can't be detached). The
    /// caller owns moving the pane's content elsewhere.
    pub fn detach_leaf(&mut self, pane_id: PaneId) -> bool {
        self.remove(pane_id).is_some()
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

// ---- GAP-2.2 单元测试 ----
#[cfg(test)]
mod tests {
    use super::*;

    /// 构造 3 层嵌套:H[ V[a, b], H[c, V[d, e]] ]
    /// 形成 5 个 pane(a..e),用于覆盖嵌套场景。
    fn build_nested() -> (SplitNode, [PaneId; 5]) {
        let ids = [
            PaneId(101), PaneId(102), PaneId(103), PaneId(104), PaneId(105),
        ];
        let inner_v1 = SplitNode::Split {
            split_id: 1,
            direction: SplitDirection::Vertical,
            ratio: 0.3,
            first: Box::new(SplitNode::Leaf(ids[0])),
            second: Box::new(SplitNode::Leaf(ids[1])),
        };
        let inner_v2 = SplitNode::Split {
            split_id: 2,
            direction: SplitDirection::Vertical,
            ratio: 0.4,
            first: Box::new(SplitNode::Leaf(ids[3])),
            second: Box::new(SplitNode::Leaf(ids[4])),
        };
        let inner_h = SplitNode::Split {
            split_id: 3,
            direction: SplitDirection::Horizontal,
            ratio: 0.7,
            first: Box::new(SplitNode::Leaf(ids[2])),
            second: Box::new(inner_v2),
        };
        let root = SplitNode::Split {
            split_id: 4,
            direction: SplitDirection::Horizontal,
            ratio: 0.2,
            first: Box::new(inner_v1),
            second: Box::new(inner_h),
        };
        (root, ids)
    }

    /// 收集所有 Split 节点的 ratio,用于 equalize 断言。
    fn collect_ratios(node: &SplitNode, out: &mut Vec<f32>) {
        if let SplitNode::Split { ratio, first, second, .. } = node {
            out.push(*ratio);
            collect_ratios(first, out);
            collect_ratios(second, out);
        }
    }

    #[test]
    fn equalize_all_sets_every_split_to_half() {
        let (mut tree, _) = build_nested();
        tree.equalize_all();
        let mut ratios = Vec::new();
        collect_ratios(&tree, &mut ratios);
        assert_eq!(ratios.len(), 4, "应有 4 个 Split 节点");
        for r in ratios {
            assert!((r - 0.5).abs() < 1e-6, "每个 Split.ratio 应为 0.5,实际 {}", r);
        }
    }

    #[test]
    fn equalize_all_on_leaf_is_noop() {
        let mut tree = SplitNode::Leaf(PaneId(42));
        tree.equalize_all();
        assert!(matches!(tree, SplitNode::Leaf(PaneId(42))));
    }

    // ── GAP-6.6: swap_by_ids / detach_leaf (swap-pane / join-pane) ────────

    #[test]
    fn swap_by_ids_swaps_two_leaves_anywhere() {
        let (mut tree, ids) = build_nested(); // DFS order [a, b, c, d, e]
        assert!(tree.swap_by_ids(ids[0], ids[3])); // swap a ↔ d
        assert_eq!(tree.pane_ids(), vec![ids[3], ids[1], ids[2], ids[0], ids[4]]);
    }

    #[test]
    fn swap_by_ids_twice_restores() {
        let (mut tree, ids) = build_nested();
        let before = tree.pane_ids();
        assert!(tree.swap_by_ids(ids[0], ids[4]));
        assert!(tree.swap_by_ids(ids[0], ids[4]));
        assert_eq!(tree.pane_ids(), before);
    }

    #[test]
    fn swap_by_ids_unknown_leaves_tree_unchanged() {
        let (mut tree, ids) = build_nested();
        let before = tree.pane_ids();
        assert!(!tree.swap_by_ids(ids[0], PaneId(999)));
        assert_eq!(tree.pane_ids(), before);
    }

    #[test]
    fn swap_by_ids_same_id_present_is_noop() {
        let (mut tree, ids) = build_nested();
        let before = tree.pane_ids();
        assert!(tree.swap_by_ids(ids[0], ids[0]));
        assert_eq!(tree.pane_ids(), before);
    }

    #[test]
    fn detach_leaf_removes_and_keeps_rest() {
        let (mut tree, ids) = build_nested(); // 5 panes
        assert!(tree.detach_leaf(ids[1])); // remove b
        assert_eq!(tree.pane_count(), 4);
        assert!(!tree.contains(ids[1]));
        assert!(tree.contains(ids[0]));
    }

    #[test]
    fn detach_only_leaf_fails() {
        let mut tree = SplitNode::single(PaneId(7));
        assert!(!tree.detach_leaf(PaneId(7)));
        assert!(matches!(tree, SplitNode::Leaf(PaneId(7))));
    }

    #[test]
    fn insert_existing_pane_as_sibling_via_split() {
        // join-pane target side: `split(at_leaf, existing_pane, dir)` inserts an
        // external pane as the sibling of at_leaf — split already supports this.
        let (mut tree, ids) = build_nested();
        let external = PaneId(999);
        assert!(tree.split(ids[2], external, SplitDirection::Vertical));
        assert!(tree.contains(external));
        assert_eq!(tree.pane_count(), 6);
    }

    #[test]
    fn swap_with_sibling_swaps_and_inverts_ratio() {
        let (mut tree, ids) = build_nested();
        // 找到含 ids[0] 的最浅 Split(根 H,first=V[a,b], second=H[...])
        // 期望:根 Split 的 first/second 交换,ratio 从 0.2 → 0.8
        let ok = tree.swap_with_sibling(ids[0]);
        assert!(ok);
        if let SplitNode::Split { ratio, first, .. } = &tree {
            assert!((*ratio - 0.8).abs() < 1e-6, "ratio 应翻转为 0.8,实际 {}", ratio);
            // 交换后 first 应是原 inner_h(含 ids[2..])
            assert!(first.contains(ids[2]));
            assert!(!first.contains(ids[0]));
        } else {
            panic!("根应仍为 Split");
        }
    }

    #[test]
    fn swap_with_sibling_twice_returns_to_original() {
        let (mut tree, ids) = build_nested();
        let r1 = if let SplitNode::Split { ratio, .. } = &tree { *ratio } else { panic!() };
        assert!(tree.swap_with_sibling(ids[0]));
        assert!(tree.swap_with_sibling(ids[0]));
        let r2 = if let SplitNode::Split { ratio, .. } = &tree { *ratio } else { panic!() };
        assert!((r1 - r2).abs() < 1e-6, "swap 两次应回到原 ratio");
        // 结构也回到原状(first 仍含 ids[0])
        if let SplitNode::Split { first, .. } = &tree {
            assert!(first.contains(ids[0]));
        }
    }

    #[test]
    fn swap_with_sibling_on_root_leaf_returns_false() {
        let pane = PaneId(7);
        let mut tree = SplitNode::Leaf(pane);
        assert!(!tree.swap_with_sibling(pane));
    }

    #[test]
    fn swap_with_sibling_on_unknown_pane_returns_false() {
        let (mut tree, _) = build_nested();
        let unknown = PaneId(9999);
        assert!(!tree.swap_with_sibling(unknown));
    }

    #[test]
    fn neighbor_in_order_next_wraps_at_boundary() {
        let (tree, ids) = build_nested();
        // pane_ids() DFS 先序:首先 inner_v1 (ids[0], ids[1]),
        // 然后 inner_h: ids[2], 然后 inner_v2 (ids[3], ids[4])
        let order = tree.pane_ids();
        assert_eq!(order, vec![ids[0], ids[1], ids[2], ids[3], ids[4]]);
        // next from last wraps to first
        assert_eq!(tree.neighbor_in_order(ids[4], 1), Some(ids[0]));
        // prev from first wraps to last
        assert_eq!(tree.neighbor_in_order(ids[0], -1), Some(ids[4]));
        // mid +1
        assert_eq!(tree.neighbor_in_order(ids[2], 1), Some(ids[3]));
        // mid -1
        assert_eq!(tree.neighbor_in_order(ids[2], -1), Some(ids[1]));
    }

    #[test]
    fn neighbor_in_order_unknown_pane_returns_none() {
        let (tree, _) = build_nested();
        assert_eq!(tree.neighbor_in_order(PaneId(8888), 1), None);
    }

    #[test]
    fn neighbor_in_order_single_pane_returns_self() {
        let p = PaneId(1);
        let tree = SplitNode::Leaf(p);
        assert_eq!(tree.neighbor_in_order(p, 1), Some(p));
        assert_eq!(tree.neighbor_in_order(p, -1), Some(p));
    }

    /// 复用 remove() 验证"移走 pane 后父 Split 折叠"行为符合 detach 语义。
    #[test]
    fn remove_collapses_parent_split() {
        let (mut tree, ids) = build_nested();
        // 移走 ids[0] → inner_v1 折叠为 Leaf(ids[1]),根的 first 变成 Leaf(ids[1])
        let remaining = tree.remove(ids[0]);
        assert!(remaining.is_some());
        // ids[0] 不再存在,ids[1..5] 仍在
        let ids_after = tree.pane_ids();
        assert_eq!(ids_after.len(), 4);
        assert!(!ids_after.contains(&ids[0]));
        for i in 1..5 { assert!(ids_after.contains(&ids[i])); }
    }

    #[test]
    fn remove_on_root_leaf_returns_none() {
        let p = PaneId(1);
        let mut tree = SplitNode::Leaf(p);
        assert_eq!(tree.remove(p), None);
    }

    /// GAP-5.6: remap rewrites leaf ids while preserving tree shape + ratios.
    #[test]
    fn remap_pane_ids_rewrites_leaves() {
        let (mut tree, ids) = build_nested();
        let before = tree.pane_ids();
        let mut mapping = std::collections::HashMap::new();
        // Shift every id by +1000.
        for id in &before {
            mapping.insert(*id, PaneId(id.0 + 1000));
        }
        tree.remap_pane_ids(&mapping);
        let after = tree.pane_ids();
        assert_eq!(after.len(), before.len());
        for (b, a) in before.iter().zip(after.iter()) {
            assert_eq!(a.0, b.0 + 1000);
        }
        // Original ids must be gone.
        for id in &ids {
            assert!(!after.contains(id));
        }
    }

    /// GAP-5.6: regenerate_split_ids must not disturb pane layout.
    #[test]
    fn regenerate_split_ids_preserves_panes() {
        let (mut tree, _ids) = build_nested();
        let before = tree.pane_ids();
        tree.regenerate_split_ids();
        let after = tree.pane_ids();
        assert_eq!(before, after);
    }

    /// GAP-5.6: serialize → deserialize must reconstruct the same leaf order
    /// and a representative ratio (the persistence path for cross-restart
    /// recovery).
    #[test]
    fn serde_round_trip_preserves_structure() {
        let (tree, _ids) = build_nested();
        let json = serde_json::to_string(&tree).expect("serialize");
        let back: SplitNode = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.pane_ids(), tree.pane_ids());
        // Root ratio (0.2) survives.
        match &back {
            SplitNode::Split { ratio, .. } => assert!((ratio - 0.2).abs() < 1e-6),
            _ => panic!("root must be a Split"),
        }
    }

    // ── GAP-6.6: resize_ratio / find_adjacent (resize-pane / select-pane geom) ──
    //
    // These two ops back `tmux resize-pane` and the (stubbed) geometric
    // `select-pane -L/-R/-U/-D`. `resize_ratio` was previously untested and
    // `find_adjacent` only exercised indirectly via layout; both are load-
    // bearing for the manual pixel-smoke resize/focus steps.

    /// `resize_ratio` clamps the shallowest ancestor Split's ratio to
    /// [MIN_RATIO, MAX_RATIO] — a huge delta must not panic or overflow.
    #[test]
    fn resize_ratio_clamps_to_min_and_max() {
        let (mut tree, ids) = build_nested();
        // ids[0] sits in root.first → resize_ratio moves the ROOT ratio
        // (the shallowest ancestor Split containing the pane). Root is 0.2.
        let mut ratios = Vec::new();

        tree.resize_ratio(ids[0], 100.0);
        collect_ratios(&tree, &mut ratios);
        assert!((ratios[0] - MAX_RATIO).abs() < 1e-6, "delta > MAX clamps: {}", ratios[0]);

        ratios.clear();
        tree.resize_ratio(ids[0], -100.0);
        collect_ratios(&tree, &mut ratios);
        assert!((ratios[0] - MIN_RATIO).abs() < 1e-6, "delta < MIN clamps: {}", ratios[0]);
    }

    /// A small delta only touches the shallowest Split; deeper splits on the
    /// path are left alone (guards against a future "resize everything" bug).
    #[test]
    fn resize_ratio_small_delta_only_touches_shallowest_split() {
        let (mut tree, ids) = build_nested();
        // [root=0.2, inner_v1=0.3, inner_h=0.7, inner_v2=0.4]
        tree.resize_ratio(ids[0], 0.1); // root 0.2 → 0.3
        let mut ratios = Vec::new();
        collect_ratios(&tree, &mut ratios);
        assert!((ratios[0] - 0.3).abs() < 1e-6, "root ratio moved: {}", ratios[0]);
        assert!((ratios[1] - 0.3).abs() < 1e-6, "deeper inner_v1 unchanged: {}", ratios[1]);
        assert!((ratios[2] - 0.7).abs() < 1e-6, "unrelated inner_h unchanged: {}", ratios[2]);
    }

    /// `find_adjacent` returns the geometric neighbor in the matching split
    /// direction (the op behind `select-pane -L/-R/-U/-D`).
    #[test]
    fn find_adjacent_returns_neighbor_in_matching_direction() {
        // build_nested: H[ V[a,b], H[c, V[d,e]] ]
        let (tree, ids) = build_nested();
        // a 的水平(右)邻居:跨根 H 分割线 → c (root.second.first_pane)
        assert_eq!(tree.find_adjacent(ids[0], SplitDirection::Horizontal), Some(ids[2]));
        // a 的垂直(下)邻居:同 inner_v1 → b
        assert_eq!(tree.find_adjacent(ids[0], SplitDirection::Vertical), Some(ids[1]));
        // c 的水平(左)邻居 → b (root.first.last_pane)
        assert_eq!(tree.find_adjacent(ids[2], SplitDirection::Horizontal), Some(ids[1]));
    }

    /// When no Split exists in the requested direction, `find_adjacent`
    /// returns None (the pane is on that edge of the layout).
    #[test]
    fn find_adjacent_none_when_no_split_in_that_direction() {
        // 纯水平树:无垂直分割,任何 pane 的 Vertical 邻居为 None
        let tree = SplitNode::Split {
            split_id: 1,
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(SplitNode::Leaf(PaneId(1))),
            second: Box::new(SplitNode::Leaf(PaneId(2))),
        };
        assert_eq!(tree.find_adjacent(PaneId(1), SplitDirection::Vertical), None);
        assert_eq!(tree.find_adjacent(PaneId(1), SplitDirection::Horizontal), Some(PaneId(2)));
    }

    /// join-pane tree op sequence: detach a leaf from tree A (collapsing its
    /// parent Split) and insert it as a sibling in tree B. Both trees must
    /// remain valid; A keeps ≥1 pane. This is the split-tree half of
    /// `apply_attach_pane` (the Arc-follows half is in `join_pane_e2e`).
    #[test]
    fn remove_then_split_into_other_tree_yields_valid_shape() {
        let a = PaneId(10);
        let b = PaneId(11);
        let c = PaneId(20);
        let mut tree_a = SplitNode::Split {
            split_id: 1,
            direction: SplitDirection::Vertical,
            ratio: 0.5,
            first: Box::new(SplitNode::Leaf(a)),
            second: Box::new(SplitNode::Leaf(b)),
        };
        let mut tree_b = SplitNode::single(c);

        // 源侧:detach a → A 折叠为单叶 b,仍 ≥1 pane
        assert_eq!(tree_a.remove(a), Some(b));
        assert_eq!(tree_a.pane_count(), 1);
        assert!(matches!(tree_a, SplitNode::Leaf(PaneId(11))));
        assert!(!tree_a.contains(a));

        // 目标侧:把 a 作为 c 的 sibling 插入(split 把 new_pane 放 second)
        assert!(tree_b.split(c, a, SplitDirection::Horizontal));
        assert_eq!(tree_b.pane_count(), 2);
        assert!(tree_b.contains(a) && tree_b.contains(c));
        assert_eq!(tree_b.pane_ids(), vec![c, a]);
    }
}
