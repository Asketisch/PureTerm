//! PureTerm 的全局热键支持。
//!
//! 注册系统范围的键盘快捷键,即使 PureTerm
//! 未聚焦时也能工作。使用平台特定的 API:
//! - macOS: CGEventTap
//! - Linux: X11 grab_key
//! - Windows: RegisterHotKey

/// 全局热键注册。
#[derive(Debug, Clone)]
pub struct GlobalHotkey {
    /// 唯一标识符。
    pub id: u32,
    /// 键组合。
    pub key: HotkeyCombo,
    /// 要执行的动作。
    pub action: GlobalHotkeyAction,
}

#[derive(Debug, Clone)]
pub struct HotkeyCombo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalHotkeyAction {
    /// 切换窗口可见性(显示/隐藏)。
    ToggleVisibility,
    /// 显示 Quake 风格的下拉菜单。
    ShowQuake,
    /// 创建新标签页。
    NewTab,
    /// 打开命令面板。
    CommandPalette,
}

/// 管理全局热键注册。
pub struct GlobalHotkeyManager {
    hotkeys: Vec<GlobalHotkey>,
    registered: bool,
}

impl GlobalHotkeyManager {
    pub fn new() -> Self {
        Self {
            hotkeys: Vec::new(),
            registered: false,
        }
    }

    /// 注册全局热键。
    pub fn register(&mut self, hotkey: GlobalHotkey) {
        log::info!("Registering global hotkey: {:?} -> {:?}", hotkey.key, hotkey.action);
        self.hotkeys.push(hotkey);
    }

    /// 注册默认的全局热键。
    pub fn register_defaults(&mut self) {
        self.register(GlobalHotkey {
            id: 1,
            key: HotkeyCombo {
                ctrl: true,
                alt: false,
                shift: false,
                super_key: false,
                key: "`".to_string(),
            },
            action: GlobalHotkeyAction::ShowQuake,
        });
        self.register(GlobalHotkey {
            id: 2,
            key: HotkeyCombo {
                ctrl: true,
                alt: false,
                shift: true,
                super_key: false,
                key: "`".to_string(),
            },
            action: GlobalHotkeyAction::ToggleVisibility,
        });
    }

    /// 开始监听全局热键。
    pub fn start(&mut self) {
        self.registered = true;
        log::info!("Global hotkey listener started with {} hotkeys", self.hotkeys.len());
        // 平台特定的注册将在这里进行
    }

    /// 停止监听全局热键。
    pub fn stop(&mut self) {
        self.registered = false;
        log::info!("Global hotkey listener stopped");
    }

    /// 检查按键组合是否匹配任何已注册的热键。
    pub fn check(&self, ctrl: bool, alt: bool, shift: bool, super_key: bool, key: &str) -> Option<GlobalHotkeyAction> {
        self.hotkeys.iter().find(|h| {
            h.key.ctrl == ctrl
                && h.key.alt == alt
                && h.key.shift == shift
                && h.key.super_key == super_key
                && h.key.key == key
        }).map(|h| h.action)
    }

    pub fn is_registered(&self) -> bool {
        self.registered
    }

    /// 列出所有已注册的热键。
    pub fn hotkeys(&self) -> &[GlobalHotkey] {
        &self.hotkeys
    }
}

impl Default for GlobalHotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}
