use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::error::{CoreError, Result};
use crate::hotkey::HotkeysConfig;
use crate::profile::{Profile, ProfileGroup};
use crate::theme::ColorScheme;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceConfig {
    #[serde(default = "default_tabs_location")]
    pub tabs_location: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_frame")]
    pub frame: String,
    #[serde(default)]
    pub opacity: f32,
    #[serde(default)]
    pub show_tab_bar: bool,
    #[serde(default = "default_true")]
    pub show_title_bar: bool,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            tabs_location: default_tabs_location(),
            theme: default_theme(),
            frame: default_frame(),
            opacity: 1.0,
            show_tab_bar: true,
            show_title_bar: true,
        }
    }
}

fn default_tabs_location() -> String { "top".into() }
fn default_theme() -> String { "dark".into() }
fn default_frame() -> String { "thin".into() }
fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalConfig {
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: u16,
    #[serde(default = "default_font_size")]
    pub font_size_mac: u16,
    #[serde(default = "default_scrollback")]
    pub scrollback_lines: u32,
    #[serde(default = "default_renderer")]
    pub renderer: String,
    #[serde(default = "default_bell")]
    pub bell: String,
    #[serde(default = "default_true")]
    pub ligatures: bool,
    #[serde(default = "default_cursor_style")]
    pub cursor_style: String,
    #[serde(default = "default_true")]
    pub cursor_blink: bool,
    #[serde(default = "default_true")]
    pub copy_on_select: bool,
    #[serde(default)]
    pub color_scheme: Option<ColorScheme>,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            font_size_mac: default_font_size(),
            scrollback_lines: default_scrollback(),
            renderer: default_renderer(),
            bell: default_bell(),
            ligatures: true,
            cursor_style: default_cursor_style(),
            cursor_blink: true,
            copy_on_select: true,
            color_scheme: None,
        }
    }
}

fn default_font_family() -> String { "JetBrains Mono".into() }
fn default_font_size() -> u16 { 14 }
fn default_scrollback() -> u32 { 25000 }
fn default_renderer() -> String { "wgpu".into() }
fn default_bell() -> String { "visual".into() }
fn default_cursor_style() -> String { "block".into() }

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            appearance: AppearanceConfig::default(),
            terminal: TerminalConfig::default(),
            hotkeys: HotkeysConfig::default(),
            profiles: Vec::new(),
            groups: Vec::new(),
        }
    }
}

pub struct ConfigService {
    config_dir: PathBuf,
    config: RwLock<AppConfig>,
}

impl ConfigService {
    pub fn load() -> Result<Self> {
        let config_dir = Self::config_dir();
        fs::create_dir_all(&config_dir)?;

        let config_path = Self::config_path();
        let config = if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            toml::from_str(&content)?
        } else {
            let config = AppConfig::default();
            let _ = fs::write(&config_path, toml::to_string_pretty(&config).unwrap());
            config
        };

        Ok(Self {
            config_dir,
            config: RwLock::new(config),
        })
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path();
        let config = self.config.read();
        let content = toml::to_string_pretty(&*config)
            .map_err(|e| CoreError::Config(e.to_string()))?;
        fs::write(&config_path, content)?;
        Ok(())
    }

    pub fn get(&self) -> parking_lot::RwLockReadGuard<AppConfig> {
        self.config.read()
    }

    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut AppConfig),
    {
        let mut config = self.config.write();
        f(&mut config);
    }

    pub fn config_path() -> PathBuf {
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

    pub fn plugins_dir() -> PathBuf {
        Self::config_dir().join("plugins")
    }
}
