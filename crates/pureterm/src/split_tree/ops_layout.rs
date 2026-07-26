//! 像素几何布局查询:面板矩形 + 分割线位置。
//!
//! 这些方法把抽象的 `SplitNode` 树(只存比例)映射到具体的像素坐标:
//! - `compute_layout`:每个叶子的 `PaneRect`(渲染真值)
//! - `compute_dividers` / `compute_divider_for_pane` /
//!   `compute_divider_by_split_id`:分割线矩形(用于绘制 + 命中)
//! - `divider_at_point` / `pane_at_point`:坐标 → 分割线 / pane 的命中测试
//!
//! 所有几何都按 `DIVIDER_HIT_WIDTH` 扣除分割线占用宽度,与 Iced view
//! 中 divider widget 的实际占用一致。

use std::collections::HashMap;

use super::{
    DividerRect, DIVIDER_HIT_WIDTH, PaneId, PaneRect, SplitDirection, SplitNode,
};

impl SplitNode {
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
}
