//! GAP-22: MacBook Touch Bar — NSTouchBar scaffold.

/// `[touch_bar]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TouchBarConfig {
    pub enabled: bool,
    pub show_tabs: bool,
    pub show_new_tab: bool,
    pub show_split: bool,
}

impl Default for TouchBarConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            show_tabs: true,
            show_new_tab: true,
            show_split: true,
        }
    }
}

/// Install Touch Bar items for the main window. MVP stub.
pub fn refresh_touch_bar(cfg: &TouchBarConfig) {
    if !cfg.enabled {
        return;
    }
    #[cfg(target_os = "macos")]
    log::info!("GAP-22: Touch Bar refresh requested (NSTouchBar hook pending)");
    #[cfg(not(target_os = "macos"))]
    let _ = cfg;
}
