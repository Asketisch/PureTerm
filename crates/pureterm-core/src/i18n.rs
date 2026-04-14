//! Internationalization support for PureTerm.
//!
//! Provides locale-based string lookup for UI text.
//! Locales are loaded from TOML files in the config directory.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Internationalization service.
pub struct I18n {
    /// Current locale.
    locale: String,
    /// Loaded translations.
    translations: HashMap<String, HashMap<String, String>>,
}

impl I18n {
    /// Create a new i18n service with the default locale.
    pub fn new(locale: &str) -> Self {
        Self {
            locale: locale.to_string(),
            translations: HashMap::new(),
        }
    }

    /// Set the current locale.
    pub fn set_locale(&mut self, locale: &str) {
        self.locale = locale.to_string();
    }

    /// Get the current locale.
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Load translations from a TOML file.
    pub fn load_from_file(&mut self, path: &PathBuf) -> std::io::Result<()> {
        let content = fs::read_to_string(path)?;
        let translations: HashMap<String, String> = toml::from_str(&content)
            .unwrap_or_default();
        self.translations.insert(self.locale.clone(), translations);
        Ok(())
    }

    /// Load translations from the locales directory.
    pub fn load_from_dir(&mut self, dir: &PathBuf) -> std::io::Result<()> {
        if !dir.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "toml") {
                let locale = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("en")
                    .to_string();
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(trans) = toml::from_str(&content) {
                        self.translations.insert(locale, trans);
                    }
                }
            }
        }
        Ok(())
    }

    /// Translate a key, returning the translation or the key itself.
    pub fn t(&self, key: &str) -> String {
        self.translations
            .get(&self.locale)
            .and_then(|t| t.get(key).cloned())
            .unwrap_or_else(|| key.to_string())
    }

    /// Get all available locales.
    pub fn available_locales(&self) -> Vec<&str> {
        self.translations.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self::new("en")
    }
}

/// Built-in translations for English.
pub fn builtin_en() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("app.name".to_string(), "PureTerm".to_string());
    map.insert("app.tagline".to_string(), "GPU-accelerated terminal emulator".to_string());
    map.insert("menu.file".to_string(), "File".to_string());
    map.insert("menu.edit".to_string(), "Edit".to_string());
    map.insert("menu.view".to_string(), "View".to_string());
    map.insert("menu.terminal".to_string(), "Terminal".to_string());
    map.insert("menu.help".to_string(), "Help".to_string());
    map.insert("menu.new_tab".to_string(), "New Tab".to_string());
    map.insert("menu.close_tab".to_string(), "Close Tab".to_string());
    map.insert("menu.settings".to_string(), "Settings".to_string());
    map.insert("menu.quit".to_string(), "Quit".to_string());
    map.insert("settings.appearance".to_string(), "Appearance".to_string());
    map.insert("settings.terminal".to_string(), "Terminal".to_string());
    map.insert("settings.profiles".to_string(), "Profiles".to_string());
    map.insert("settings.hotkeys".to_string(), "Hotkeys".to_string());
    map.insert("settings.theme".to_string(), "Theme".to_string());
    map.insert("settings.font".to_string(), "Font".to_string());
    map.insert("settings.font_size".to_string(), "Font Size".to_string());
    map.insert("welcome.quick_connect".to_string(), "Quick Connect".to_string());
    map.insert("welcome.new_local".to_string(), "New Local Terminal".to_string());
    map.insert("welcome.profiles".to_string(), "Profiles".to_string());
    map.insert("welcome.connect".to_string(), "Connect".to_string());
    map.insert("welcome.connect_selected".to_string(), "Connect to Selected".to_string());
    map.insert("button.save".to_string(), "Save".to_string());
    map.insert("button.cancel".to_string(), "Cancel".to_string());
    map.insert("button.delete".to_string(), "Delete".to_string());
    map.insert("button.edit".to_string(), "Edit".to_string());
    map.insert("button.close".to_string(), "Close".to_string());
    map.insert("button.connect".to_string(), "Connect".to_string());
    map.insert("error.connection_failed".to_string(), "Connection failed".to_string());
    map.insert("error.auth_failed".to_string(), "Authentication failed".to_string());
    map
}

/// Built-in translations for Chinese.
pub fn builtin_zh() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("app.name".to_string(), "PureTerm".to_string());
    map.insert("app.tagline".to_string(), "GPU 加速终端模拟器".to_string());
    map.insert("menu.file".to_string(), "文件".to_string());
    map.insert("menu.edit".to_string(), "编辑".to_string());
    map.insert("menu.view".to_string(), "视图".to_string());
    map.insert("menu.terminal".to_string(), "终端".to_string());
    map.insert("menu.help".to_string(), "帮助".to_string());
    map.insert("menu.new_tab".to_string(), "新建标签".to_string());
    map.insert("menu.close_tab".to_string(), "关闭标签".to_string());
    map.insert("menu.settings".to_string(), "设置".to_string());
    map.insert("menu.quit".to_string(), "退出".to_string());
    map.insert("settings.appearance".to_string(), "外观".to_string());
    map.insert("settings.terminal".to_string(), "终端".to_string());
    map.insert("settings.profiles".to_string(), "配置".to_string());
    map.insert("settings.hotkeys".to_string(), "快捷键".to_string());
    map.insert("settings.theme".to_string(), "主题".to_string());
    map.insert("settings.font".to_string(), "字体".to_string());
    map.insert("settings.font_size".to_string(), "字体大小".to_string());
    map.insert("welcome.quick_connect".to_string(), "快速连接".to_string());
    map.insert("welcome.new_local".to_string(), "新建本地终端".to_string());
    map.insert("welcome.profiles".to_string(), "配置列表".to_string());
    map.insert("welcome.connect".to_string(), "连接".to_string());
    map.insert("welcome.connect_selected".to_string(), "连接选中项".to_string());
    map.insert("button.save".to_string(), "保存".to_string());
    map.insert("button.cancel".to_string(), "取消".to_string());
    map.insert("button.delete".to_string(), "删除".to_string());
    map.insert("button.edit".to_string(), "编辑".to_string());
    map.insert("button.close".to_string(), "关闭".to_string());
    map.insert("button.connect".to_string(), "连接".to_string());
    map.insert("error.connection_failed".to_string(), "连接失败".to_string());
    map.insert("error.auth_failed".to_string(), "认证失败".to_string());
    map
}

impl I18n {
    /// Load built-in translations.
    pub fn load_builtins(&mut self) {
        self.translations.insert("en".to_string(), builtin_en());
        self.translations.insert("zh".to_string(), builtin_zh());
    }
}