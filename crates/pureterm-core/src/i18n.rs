//! PureTerm 的国际化支持。
//!
//! 提供基于区域设置的 UI 文本字符串查找。
//! 区域设置从配置目录中的 TOML 文件加载。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// 国际化服务。
pub struct I18n {
    /// 当前区域设置。
    locale: String,
    /// 已加载的翻译。
    translations: HashMap<String, HashMap<String, String>>,
}

impl I18n {
    /// 使用默认区域设置创建新的 i18n 服务。
    pub fn new(locale: &str) -> Self {
        Self {
            locale: locale.to_string(),
            translations: HashMap::new(),
        }
    }

    /// 设置当前区域设置。
    pub fn set_locale(&mut self, locale: &str) {
        self.locale = locale.to_string();
    }

    /// 获取当前区域设置。
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// 从 TOML 文件加载翻译。
    pub fn load_from_file(&mut self, path: &PathBuf) -> std::io::Result<()> {
        let content = fs::read_to_string(path)?;
        let translations: HashMap<String, String> = toml::from_str(&content)
            .unwrap_or_default();
        self.translations.insert(self.locale.clone(), translations);
        Ok(())
    }

    /// 从区域设置目录加载翻译。
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

    /// 翻译一个键,返回翻译或键本身。
    pub fn t(&self, key: &str) -> String {
        self.translations
            .get(&self.locale)
            .and_then(|t| t.get(key).cloned())
            .unwrap_or_else(|| key.to_string())
    }

    /// 获取所有可用的区域设置。
    pub fn available_locales(&self) -> Vec<&str> {
        self.translations.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self::new("en")
    }
}

/// 英语的内置翻译。
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

/// 中文的内置翻译。
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
    /// 加载内置翻译。
    pub fn load_builtins(&mut self) {
        self.translations.insert("en".to_string(), builtin_en());
        self.translations.insert("zh".to_string(), builtin_zh());
    }
}