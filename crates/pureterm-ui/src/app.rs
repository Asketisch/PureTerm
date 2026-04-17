//! 应用 — 应用程序运行时
//!
//! 基于 Ghostty 的 App.zig:管理所有 surface、共享字体网格、
//! 消息路由和配置。

use pureterm_core::config::AppConfig;
use pureterm_core::hotkey::HotkeyService;

use crate::surface::{SurfaceId, SplitDirection, SplitResizeParameter};
use crate::tabs::TabManager;

/// 应用级消息(来自 Ghostty 的邮箱模式)
#[derive(Debug, Clone)]
pub enum AppMessage {
    Quit,
    NewWindow,
    NewTab,
    CloseTab,
    NewSplit(SplitDirection),
    ToggleFullscreen,
    ToggleMaximize,
    ToggleQuickTerminal,
    MoveTab(isize),
    GotoTab(usize),
    ResizeSplit(SplitResizeParameter),
    EqualizeSplits,
    MoveFocus(SplitDirection),
    ReloadConfig,
    ToggleInspector,
    ToggleSearch,
    ConfigChanged(Box<AppConfig>),
    SurfaceDestroyed(SurfaceId),
}

/// 主应用程序状态。
pub struct App {
    pub config: AppConfig,
    pub tab_manager: TabManager,
    pub hotkey_service: HotkeyService,
    pub show_settings: bool,
    pub search_visible: bool,
    pub search_query: String,
    pub quitting: bool,

    // ── Ghostty 风格的 App 字段 ──────────────────────────────────────────
    /// 所有活动 surface ID
    pub surfaces: Vec<SurfaceId>,
    /// 当前聚焦的 surface
    pub focused_surface: Option<SurfaceId>,
    /// 应用程序级邮箱,用于消息传递
    pub mailbox: Vec<AppMessage>,
    /// 下一个 surface ID
    next_surface_id: u64,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let hotkey_service = HotkeyService::new(&config.hotkeys);
        Self {
            config,
            tab_manager: TabManager::new(),
            hotkey_service,
            show_settings: false,
            search_visible: false,
            search_query: String::new(),
            quitting: false,
            surfaces: Vec::new(),
            focused_surface: None,
            mailbox: Vec::new(),
            next_surface_id: 1,
        }
    }

    /// 生成唯一的 surface ID
    pub fn allocate_surface_id(&mut self) -> SurfaceId {
        let id = self.next_surface_id;
        self.next_surface_id += 1;
        SurfaceId(id)
    }

    /// 注册新 surface
    pub fn add_surface(&mut self, id: SurfaceId) {
        if !self.surfaces.contains(&id) {
            self.surfaces.push(id);
        }
        self.focused_surface = Some(id);
    }

    /// 移除 surface
    pub fn remove_surface(&mut self, id: SurfaceId) {
        self.surfaces.retain(|s| *s != id);
        if self.focused_surface == Some(id) {
            self.focused_surface = self.surfaces.last().copied();
        }
    }

    /// 处理邮箱中的所有待处理消息
    pub fn process_messages(&mut self) {
        let messages: Vec<AppMessage> = self.mailbox.drain(..).collect();
        for msg in messages {
            self.handle_message(msg);
        }
    }

    /// 处理单个应用消息
    fn handle_message(&mut self, msg: AppMessage) {
        match msg {
            AppMessage::Quit => {
                self.quitting = true;
            }
            AppMessage::NewTab => {
                // 由主循环处理
            }
            AppMessage::CloseTab => {
                self.tab_manager.close_active_tab();
            }
            AppMessage::ReloadConfig => {
                // 配置重新加载由主循环处理
            }
            AppMessage::ToggleSearch => {
                self.search_visible = !self.search_visible;
            }
            AppMessage::ConfigChanged(config) => {
                self.config = *config;
                self.hotkey_service = HotkeyService::new(&self.config.hotkeys);
            }
            AppMessage::SurfaceDestroyed(id) => {
                self.remove_surface(id);
            }
            _ => {
                // 其他消息由主循环处理
            }
        }
    }

    /// 处理按键事件并返回是否已消费。
    pub fn handle_key(&mut self, key: &str, ctrl: bool, alt: bool, shift: bool, super_key: bool) -> bool {
        let modifiers = pureterm_core::hotkey::Modifiers {
            ctrl,
            alt,
            shift,
            super_key,
        };

        if self.hotkey_service.matches("tab_new", key, modifiers) {
            return true;
        }
        if self.hotkey_service.matches("tab_close", key, modifiers) {
            self.tab_manager.close_active_tab();
            return true;
        }
        if self.hotkey_service.matches("search", key, modifiers) {
            self.search_visible = !self.search_visible;
            return true;
        }
        if self.hotkey_service.matches("prev_tab", key, modifiers) {
            self.tab_manager.previous_tab();
            return true;
        }
        if self.hotkey_service.matches("next_tab", key, modifiers) {
            self.tab_manager.next_tab();
            return true;
        }

        false
    }

    /// 切换设置面板。
    pub fn toggle_settings(&mut self) {
        self.show_settings = !self.show_settings;
    }

    /// 退出应用程序。
    pub fn quit(&mut self) {
        self.quitting = true;
    }
}
