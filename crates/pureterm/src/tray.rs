//! PureTerm 的系统托盘集成。

/// 系统托盘管理器。
/// macOS: 通过 objc 消息使用 NSStatusItem。
/// Linux: 通过 D-Bus 使用 StatusNotifierItem。
/// Windows: 使用 Shell_NotifyIcon。
pub struct SystemTray {
    visible: bool,
    tooltip: String,
}

impl SystemTray {
    pub fn new() -> Self {
        Self {
            visible: false,
            tooltip: "PureTerm".to_string(),
        }
    }

    /// 显示系统托盘图标。
    pub fn show(&mut self) {
        self.visible = true;
        log::info!("System tray icon shown");
        // 平台特定的托盘创建将在这里进行
    }

    /// 隐藏系统托盘图标。
    pub fn hide(&mut self) {
        self.visible = false;
        log::info!("System tray icon hidden");
    }

    /// 设置托盘提示。
    pub fn set_tooltip(&mut self, tooltip: &str) {
        self.tooltip = tooltip.to_string();
    }

    /// 托盘图标当前是否可见。
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// 处理托盘事件。从事件循环调用。
    pub fn handle_event(&mut self, event: TrayEvent) -> TrayAction {
        match event {
            TrayEvent::Click => TrayAction::ToggleWindow,
            TrayEvent::RightClick => TrayAction::ShowMenu,
            TrayEvent::DoubleClick => TrayAction::OpenNewTab,
        }
    }
}

impl Default for SystemTray {
    fn default() -> Self {
        Self::new()
    }
}

/// 系统托盘的事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    Click,
    RightClick,
    DoubleClick,
}

/// 由托盘事件触发的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    ToggleWindow,
    ShowMenu,
    OpenNewTab,
    Quit,
}
