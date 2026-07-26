//! 可变的二叉分割面板树。
//!
//! 每个内部节点存储分割方向和比例，叶节点存储 PaneId。
//! 支持递归嵌套（水平+垂直分割）。
//!
//! 本文件是模块根:仅承载数据结构定义(PaneRect/PaneId/Surface/Pane/
//! SplitDirection/SplitNode/DividerRect)与常量;`SplitNode` 的方法按
//! 职责拆到 [`mod@ops_modify`] / [`mod@ops_resize`] / [`mod@ops_query`]
//! / [`mod@ops_layout`] / [`mod@ops_swap`] 子模块,测试见
//! [`mod@tests`] 与 [`mod@tmux_tests`]。

use std::sync::Arc;

use crate::browser::BrowserContext;
use crate::terminal_context::TerminalContext;

mod ops_layout;
mod ops_modify;
mod ops_query;
mod ops_resize;
mod ops_swap;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tmux_tests;

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
/// 单个 surface 的后端:一个 PTY(终端)或一个 WebView(浏览器)。
///
/// GAP-2.1: 由 `PaneContent` 重命名为 `Surface`,作为后端一等类型名。
/// GAP-3.5:在终端之外引入 `Web` 变体,使一个 pane 能容纳 WebView。
/// 用枚举而非 trait:仅两种变体,每个 match 点更直接,且快照序列化
/// 需要判别式(`session_restore::PaneSnapshot::kind`)。
pub enum Surface {
    /// 终端(本地 PTY / SSH PTY)。
    Terminal(Arc<TerminalContext>),
    /// 内置浏览器(GAP-3.5 WebView)。
    Web(Arc<BrowserContext>),
}

/// 向后兼容别名:旧名 `PaneContent` 等价于 `Surface`(GAP-2.1 重命名)。
/// 新代码应使用 `Surface`;此别名保留一个 release 后移除。
pub type PaneContent = Surface;

/// 面板:持有一个 surface(终端或浏览器)+ 在分割树中的位置。
pub struct Pane {
    pub id: PaneId,
    pub content: Surface,
    pub title: String,
}

impl Pane {
    /// 若是终端 pane,返回其终端上下文。
    pub fn terminal(&self) -> Option<&Arc<TerminalContext>> {
        match &self.content {
            Surface::Terminal(t) => Some(t),
            _ => None,
        }
    }

    /// 若是浏览器 pane,返回其浏览器上下文。
    pub fn web(&self) -> Option<&Arc<BrowserContext>> {
        match &self.content {
            Surface::Web(w) => Some(w),
            _ => None,
        }
    }

    /// 是否为浏览器 pane。
    pub fn is_web(&self) -> bool {
        matches!(self.content, Surface::Web(_))
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
///
/// `pub(crate)` 以便 `split_tree` 的各 `ops_*` 子模块共享。
pub(crate) fn next_split_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
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
