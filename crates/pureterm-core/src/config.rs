use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

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
    /// Config version for migration tracking.
    #[serde(default = "default_config_version")]
    pub version: u32,
}

fn default_config_version() -> u32 { 1 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GlassEffectStyle {
    None,      // 无玻璃效果
    Regular,   // macOS 常规材质
    Clear,     // macOS 26+ 透明材质
    Light,     // 浅色外观
    Dark,      // 深色外观
    Automatic, // 自动（平台默认）
}

impl Default for GlassEffectStyle {
    fn default() -> Self {
        Self::Automatic
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceConfig {
    #[serde(default = "default_tabs_location")]
    pub tabs_location: String,    // 标签栏位置：top 或 bottom
    #[serde(default = "default_theme")]
    pub theme: String,           // 终端配色方案名称
    #[serde(default = "default_ui_theme")]
    pub ui_theme: String,        // 界面主题：dark / light
    #[serde(default = "default_frame")]
    pub frame: String,           // 窗口边框样式
    #[serde(default)]
    pub opacity: f32,            // 窗口不透明度 (0.1-1.0)
    #[serde(default)]
    pub show_tab_bar: bool,      // 是否显示标签栏
    #[serde(default = "default_true")]
    pub show_title_bar: bool,   // 是否显示标题栏
    #[serde(default = "default_corner_radius")]
    pub corner_radius: f32,      // 窗口圆角半径（像素），默认 16
    #[serde(default)]
    pub glass_effect: GlassEffectStyle, // 玻璃效果样式
    #[serde(default = "default_background_blur")]
    pub background_blur: u32,   // 背景模糊强度 (0-100)
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            tabs_location: default_tabs_location(),
            theme: default_theme(),
            ui_theme: default_ui_theme(),
            frame: default_frame(),
            opacity: 1.0,
            show_tab_bar: true,
            show_title_bar: true,
            corner_radius: default_corner_radius(),
            glass_effect: GlassEffectStyle::default(),
            background_blur: default_background_blur(),
        }
    }
}

fn default_tabs_location() -> String { "top".into() }
fn default_theme() -> String { "dark".into() }
fn default_ui_theme() -> String { "dark".into() }
fn default_frame() -> String { "thin".into() }
fn default_true() -> bool { true }
fn default_corner_radius() -> f32 { 16.0 }
fn default_background_blur() -> u32 { 20 }

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
            version: default_config_version(),
        }
    }
}

pub struct ConfigService {
    config_dir: PathBuf,
    config: RwLock<AppConfig>,
    /// Sender for config reload notifications.
    reload_tx: Option<mpsc::Sender<()>>,
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
            reload_tx: None,
        })
    }

    /// Start watching the config file for changes.
    /// Returns a receiver that will be notified when the config file changes.
    pub fn start_watching(&mut self) -> Result<mpsc::Receiver<()>> {
        let (tx, rx) = mpsc::channel();
        let tx_clone = tx.clone();
        let config_path = Self::config_path();

        let mut watcher = RecommendedWatcher::new(
            move |res: std::result::Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    if event.kind.is_modify() {
                        let _ = tx_clone.send(());
                    }
                }
            },
            Config::default().with_poll_interval(Duration::from_secs(2)),
        )
        .map_err(|e| CoreError::Config(e.to_string()))?;

        watcher
            .watch(&config_path, RecursiveMode::NonRecursive)
            .map_err(|e| CoreError::Config(e.to_string()))?;

        self.reload_tx = Some(tx);
        Ok(rx)
    }

    /// Reload the config from disk.
    pub fn reload(&self) -> Result<()> {
        let config_path = Self::config_path();
        if !config_path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&config_path)?;
        let new_config: AppConfig = toml::from_str(&content)?;

        let mut config = self.config.write();
        *config = new_config;

        log::info!("Config reloaded from disk");
        Ok(())
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
