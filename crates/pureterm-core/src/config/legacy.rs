//! Legacy compatibility types
//!
//! These types are kept for backward compatibility with pureterm-ui and other crates.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::Result;
use crate::hotkey::HotkeysConfig;
use crate::profile::{Profile, ProfileGroup};
use crate::theme::ColorScheme;

use super::types::{GlassEffectStyle, InputMode};

// ── Legacy default value helpers ──────────────────────────────────────────────

pub(super) fn default_config_version() -> u32 { 1 }

fn legacy_tabs_location() -> String { "top".into() }
fn legacy_theme() -> String { "dark".into() }
fn legacy_ui_theme() -> String { "dark".into() }
fn legacy_frame() -> String { "thin".into() }
pub(super) fn legacy_true() -> bool { true }
fn legacy_corner_radius() -> f32 { 16.0 }
fn legacy_background_blur() -> u32 { 20 }
fn legacy_font_family() -> String { "JetBrains Mono".into() }
fn legacy_font_size() -> u16 { 13 }
fn legacy_scrollback() -> u32 { 25000 }
fn legacy_renderer() -> String { "wgpu".into() }
fn legacy_bell() -> String { "visual".into() }
fn legacy_cursor_style() -> String { "block".into() }

fn default_app_triggers() -> Vec<crate::trigger::Trigger> {
    crate::trigger::TriggerService::default_triggers()
}

// ── Legacy Structs ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub appearance: AppearanceConfig,
    #[serde(default)]
    pub terminal: TerminalConfig,
    #[serde(default)]
    pub hotkeys: HotkeysConfig,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub groups: Vec<ProfileGroup>,
    #[serde(default = "default_config_version")]
    pub version: u32,
    /// Redact detected secrets (API keys, passwords, tokens) in terminal output.
    #[serde(default)]
    pub secret_redaction: bool,
    /// Show confirmation dialog before quitting when processes are still running.
    #[serde(default = "legacy_true")]
    pub confirm_quit: bool,
    /// Prevent system sleep during long-running commands.
    #[serde(default = "legacy_true")]
    pub prevent_sleep: bool,
    /// Output-stream regex triggers (matched per completed line).
    #[serde(default = "default_app_triggers")]
    pub triggers: Vec<crate::trigger::Trigger>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            appearance: AppearanceConfig::default(),
            terminal: TerminalConfig::default(),
            hotkeys: HotkeysConfig::default(),
            profiles: Vec::new(),
            groups: Vec::new(),
            version: default_config_version(),
            secret_redaction: false,
            confirm_quit: true,
            prevent_sleep: true,
            triggers: default_app_triggers(),
        }
    }
}

impl AppConfig {
    pub fn default_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("pureterm")
    }

    pub fn themes_dir() -> PathBuf {
        Self::config_dir().join("themes")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearanceConfig {
    #[serde(default = "legacy_tabs_location")]
    pub tabs_location: String,
    #[serde(default = "legacy_theme")]
    pub theme: String,
    #[serde(default = "legacy_ui_theme")]
    pub ui_theme: String,
    #[serde(default = "legacy_frame")]
    pub frame: String,
    #[serde(default)]
    pub opacity: f32,
    #[serde(default)]
    pub show_tab_bar: bool,
    #[serde(default = "legacy_true")]
    pub show_title_bar: bool,
    #[serde(default = "legacy_corner_radius")]
    pub corner_radius: f32,
    #[serde(default)]
    pub glass_effect: GlassEffectStyle,
    #[serde(default = "legacy_background_blur")]
    pub background_blur: u32,
    #[serde(default)]
    pub pane_dim_opacity: f32,
    #[serde(default)]
    pub focus_pane_on_hover: bool,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            tabs_location: legacy_tabs_location(),
            theme: legacy_theme(),
            ui_theme: legacy_ui_theme(),
            frame: legacy_frame(),
            opacity: 1.0,
            show_tab_bar: true,
            show_title_bar: true,
            corner_radius: legacy_corner_radius(),
            glass_effect: GlassEffectStyle::default(),
            background_blur: legacy_background_blur(),
            pane_dim_opacity: 0.0,
            focus_pane_on_hover: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalConfig {
    #[serde(default = "legacy_font_family")]
    pub font_family: String,
    #[serde(default)]
    pub font_path: Option<String>,
    #[serde(default = "legacy_font_size")]
    pub font_size: u16,
    #[serde(default = "legacy_font_size")]
    pub font_size_mac: u16,
    #[serde(default = "legacy_scrollback")]
    pub scrollback_lines: u32,
    #[serde(default = "legacy_renderer")]
    pub renderer: String,
    #[serde(default = "legacy_bell")]
    pub bell: String,
    #[serde(default = "legacy_true")]
    pub ligatures: bool,
    #[serde(default = "legacy_cursor_style")]
    pub cursor_style: String,
    #[serde(default = "legacy_true")]
    pub cursor_blink: bool,
    #[serde(default = "legacy_true")]
    pub copy_on_select: bool,
    #[serde(default)]
    pub color_scheme: Option<ColorScheme>,
    #[serde(default)]
    pub input_mode: InputMode,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_family: legacy_font_family(),
            font_path: None,
            font_size: legacy_font_size(),
            font_size_mac: legacy_font_size(),
            scrollback_lines: legacy_scrollback(),
            renderer: legacy_renderer(),
            bell: legacy_bell(),
            ligatures: true,
            cursor_style: legacy_cursor_style(),
            cursor_blink: true,
            copy_on_select: true,
            color_scheme: None,
            input_mode: InputMode::default(),
        }
    }
}

// ── Legacy helper functions ───────────────────────────────────────────────────

/// Load AppConfig from the default config path
pub(super) fn load_app_config() -> Result<AppConfig> {
    let config_path = AppConfig::default_path();
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    } else {
        Ok(AppConfig::default())
    }
}

/// Save AppConfig to the default config path
pub(super) fn save_app_config(config: &AppConfig) -> Result<()> {
    let config_path = AppConfig::default_path();
    let content = toml::to_string_pretty(config)
        .map_err(|e| crate::error::CoreError::Config(e.to_string()))?;
    std::fs::write(&config_path, content)?;
    Ok(())
}
