//! GAP-24：状态栏组件 trait + 逐帧数据快照。
//!
//! 底部栏的每个分段都是一个 `StatusBarComponent`。应用在 `view_bottom_bar`
//! 内（私有字段/方法都在作用域内）从实时状态构建 `BarData` 快照，
//! 并把 `&BarData` 传给每个组件的 `view`。组件各自持有统计状态
//!（CPU/内存/网络的环形缓冲等），经 `apply_stats`/`apply_git` 更新。
//!
//! 设计要点：
//! - `view` 返回 `BarElement<'static>`（拥有的 `iced::Element`）。控件由
//!   拥有的字符串 / 克隆样本构建，因此不会有借用把返回元素与 `&self` 或
//!   `&BarData` 纠缠在一起——绕开了 `view<'a>(&'a self, ...)` 会在注册表上
//!   强加的生存期体操。
//! - `BarData` 刻意只携带可移植类型（无 cfg 门控的 SSH/Vi 枚举）：
//!   cfg 门控的格式化留在 `view_bottom_bar`，它把 SSH/Vi 预格式化
//!   为 `(String, Color)` / `&'static str`。
//! - 需要 `Send`，因为 tokio 执行器要求 `PureTermApp` 是 `Send`，而
//!   注册表是它的一字段。（应用刻意不是 `Sync`。）
//!
//! 分组分类（chip 渲染器据此分配簇颜色与组间间距——
//! 见 [`crate::status_bar::chip`]）：
//! - `Context`：cwd / git / notifications（你在哪里）。
//! - `Runtime`：ssh / vimode / process / agent（在运行什么）。
//! - `System`：cpu / memory / network / battery / codeindex / lsp（机器指标）。
//! - `Meta`：cursor / clock（终端本地显示状态）。

use std::borrow::Cow;

use iced::{Color, Element, Theme};

use crate::iced_app::Message;
use crate::notification_store::NotificationSummary;

use super::git::GitStatus;
use super::stats::StatSnapshot;

/// 绑定到应用 `Message`/`Theme`/`Renderer` 的拥有的状态栏元素。
pub(crate) type BarElement<'a> = Element<'a, Message, iced::Theme, iced::Renderer>;

/// 状态栏分段所属的视觉簇。渲染器用它做 chip 着色 + 8px 组间间距。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BarGroup {
    /// cwd / git / notifications（你在哪里）
    Context,
    /// ssh / vimode / process / agent（在运行什么）
    Runtime,
    /// cpu / memory / network / battery / codeindex / lsp（机器指标）
    System,
    /// cursor / clock（终端本地显示状态）
    Meta,
}

/// 经 `BarData` 转发给 `Clock` 组件的时钟配置。
#[derive(Clone, Debug)]
pub struct ClockCfg {
    pub format: Cow<'static, str>,
    pub show_seconds: bool,
}

/// 组件可渲染的应用实时状态只读快照。
///
/// 所有逐面板/逐帧值（cwd、git、ssh、vi、process、agent、cursor）
/// 都由 `view_bottom_bar` 预计算。系统指标位于持统计数据的组件内部，
/// 不在这里。
pub struct BarData<'a> {
    pub cwd: Option<&'a str>,
    /// 即时纯文件系统分支读取（cwd/面板变化时总是新鲜的）。
    pub git_branch: Option<&'a str>,
    /// 异步 porcelain-v2 升级（首个 `StatusBarTick` 解析前为 None）。
    pub git_status: Option<&'a GitStatus>,
    /// 预格式化的 SSH 分段 `("  SSH …", color)`，若有。
    pub ssh: Option<(String, Color)>,
    /// 预格式化的 vi/复制模式标签，若 vi 模式激活。
    pub vi_mode: Option<&'static str>,
    pub process: Option<&'a str>,
    /// 预格式化的 agent 指示 `("⚡", color)`，若有。
    pub agent: Option<(String, Color)>,
    /// GAP-03：代码库索引进度 0–100（可选状态栏分段）。
    pub codeindex_percent: Option<u8>,
    /// GAP-35：非零时的 LSP 诊断计数 `(errors, warnings)`。
    pub lsp_summary: Option<(u32, u32)>,
    /// GAP-09：未读通知摘要（状态栏计数）。`None`/零 ⇒ 隐藏。
    pub notifications: Option<NotificationSummary>,
    pub cursor: (usize, usize),
    pub show_sparklines: bool,
    /// 时钟分段配置（format + show_seconds）。持有的；从 `[statusbar]`
    /// 配置转发，逐帧渲染无需再查配置。
    pub clock_cfg: &'a ClockCfg,
    /// 活动的 Iced 主题，转发过来让 chip 能从调色板派生自己的背景。
    /// 引用由 `iced::Application::theme` 拥有；此处借用是安全的，因为
    /// 状态栏在生成该快照的同一个 `view` 调用内渲染。
    pub theme_for_view: &'a Theme,
}

/// 底部状态栏的一个分段。
pub trait StatusBarComponent: Send {
    /// 稳定的组件 id（与 `[statusbar].components` 条目对应）。
    fn id(&self) -> &'static str;
    /// 该分段属于哪个视觉簇。chip 渲染器用它做颜色着色和 8px 组间
    /// 间隔。默认：`BarGroup::Context`（现有组件按模块级分组分类覆盖）。
    fn group(&self) -> BarGroup {
        BarGroup::Context
    }
    /// True ⇒ 在弹性间隔右侧渲染（如 clock、cursor）。
    fn align_right(&self) -> bool {
        false
    }
    /// 把分段渲染为拥有的元素。
    fn view(&self, data: &BarData<'_>) -> BarElement<'static>;
    /// 接收最新的系统统计快照（默认为空操作）。
    fn apply_stats(&mut self, _snap: &StatSnapshot) {}
    /// 接收最新增强的 git 状态（默认为空操作）。
    fn apply_git(&mut self, _git: &GitStatus) {}
}
