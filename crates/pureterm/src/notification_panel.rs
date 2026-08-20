//! GAP-09:通知叠加面板——最近通知(OSC 9/99/777 与响铃)的
//! 只读历史,含来源窗格、来源类型、
//! 场合与时间戳。点击条目会聚焦其来源窗格。
//!
//! 本模块持有面板的*状态*。视图(`App::view_notifications_panel`)
//! 与 `Message` 接线位于 `iced_app.rs`,那里有应用的私有
//! 字段(`NotificationStore`、窗格 id、终端上下文)。
//!
//! 面板每帧实时读取内存中的 `NotificationStore`,因此
//! 这里的状态只有 open 标志——无异步取数、无过期、无快照。

/// 通知叠加面板的状态。
#[derive(Debug, Clone, Default)]
pub struct NotificationPanelState {
    /// 面板可见性。
    pub open: bool,
}

impl NotificationPanelState {
    /// 初始的关闭状态面板(应用启动时使用)。
    pub fn new() -> Self {
        Self::default()
    }

    /// 切换可见性(⌘⇧M 动作)。
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// 关闭面板。
    pub fn close(&mut self) {
        self.open = false;
    }
}
