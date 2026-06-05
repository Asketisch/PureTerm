use std::collections::HashMap;

pub struct I18n {
    locale: String,
    translations: HashMap<String, HashMap<String, String>>,
}

impl I18n {
    pub fn new() -> Self {
        let mut translations = HashMap::new();
        translations.insert("en".to_string(), builtin_en());
        translations.insert("zh".to_string(), builtin_zh());
        Self {
            locale: "en".to_string(),
            translations,
        }
    }

    pub fn set_locale(&mut self, locale: &str) {
        if self.translations.contains_key(locale) {
            self.locale = locale.to_string();
        }
    }

    pub fn t(&self, key: &str) -> String {
        self.translations
            .get(&self.locale)
            .and_then(|m| m.get(key).cloned())
            .or_else(|| {
                self.translations
                    .get("en")
                    .and_then(|m| m.get(key).cloned())
            })
            .unwrap_or_else(|| key.to_string())
    }

    pub fn available_locales(&self) -> Vec<(&str, &str)> {
        vec![("en", "English"), ("zh", "中文")]
    }

    pub fn locale_display_name(&self) -> String {
        self.available_locales()
            .iter()
            .find(|(code, _)| *code == self.locale)
            .map(|(_, name)| name.to_string())
            .unwrap_or_else(|| self.locale.clone())
    }

    pub fn locale_code_for_display(&self, display: &str) -> Option<&str> {
        self.available_locales()
            .iter()
            .find(|(_, name)| *name == display)
            .map(|(code, _)| *code)
    }
}

pub struct PickMapping {
    displays: Vec<String>,
    configs: Vec<String>,
}

impl PickMapping {
    pub fn new(i18n: &I18n, prefix: &str, configs: &[&str]) -> Self {
        PickMapping {
            displays: configs
                .iter()
                .map(|c| i18n.t(&format!("{prefix}.{c}")))
                .collect(),
            configs: configs.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn display_options(&self) -> Vec<String> {
        self.displays.clone()
    }

    pub fn display_for(&self, config: &str) -> String {
        self.configs
            .iter()
            .position(|c| c == config)
            .and_then(|i| self.displays.get(i).cloned())
            .unwrap_or_else(|| config.to_string())
    }

    pub fn config_for(&self, display: &str) -> Option<String> {
        self.displays
            .iter()
            .position(|d| d == display)
            .and_then(|i| self.configs.get(i).cloned())
    }
}

fn builtin_en() -> HashMap<String, String> {
    let mut m = HashMap::new();

    // App
    m.insert("app.title".into(), "PureTerm Settings".into());

    // Sidebar
    m.insert("sidebar.general".into(), "General".into());
    m.insert("sidebar.appearance".into(), "Appearance".into());
    m.insert("sidebar.colors".into(), "Colors".into());
    m.insert("sidebar.terminal".into(), "Terminal".into());
    m.insert("sidebar.ssh_profiles".into(), "SSH Profiles".into());
    m.insert("sidebar.shortcuts".into(), "Shortcuts".into());

    // Page titles
    m.insert("page.general".into(), "General".into());
    m.insert("page.appearance".into(), "Appearance".into());
    m.insert("page.colors".into(), "Colors".into());
    m.insert("page.terminal".into(), "Terminal".into());
    m.insert("page.ssh_profiles".into(), "SSH Profiles".into());
    m.insert("page.keybindings".into(), "Keyboard Shortcuts".into());

    // General page
    m.insert("general.section.language".into(), "Language".into());
    m.insert("general.language".into(), "Language".into());
    m.insert("general.section.shell".into(), "Shell".into());
    m.insert("general.shell_program".into(), "Shell Program".into());
    m.insert("general.placeholder.shell".into(), "e.g. /bin/zsh".into());
    m.insert("general.working_directory".into(), "Working Directory".into());
    m.insert("general.placeholder.working_directory".into(), "Default working directory".into());
    m.insert("general.section.clipboard".into(), "Clipboard".into());
    m.insert("general.osc52".into(), "OSC 52".into());
    m.insert("general.section.behavior".into(), "Behavior".into());
    m.insert("general.live_reload".into(), "Live config reload".into());
    m.insert("general.ipc_socket".into(), "IPC socket".into());

    // Appearance page
    m.insert("appearance.section.font".into(), "Font".into());
    m.insert("appearance.family".into(), "Family".into());
    m.insert("appearance.placeholder.font".into(), "e.g. JetBrains Mono".into());
    m.insert("appearance.size".into(), "Size".into());
    m.insert("appearance.section.window".into(), "Window".into());
    m.insert("appearance.opacity".into(), "Opacity".into());
    m.insert("appearance.background_blur".into(), "Background blur".into());
    m.insert("appearance.padding".into(), "Padding".into());
    m.insert("appearance.decorations".into(), "Decorations".into());
    m.insert("appearance.section.cursor".into(), "Cursor".into());
    m.insert("appearance.shape".into(), "Shape".into());
    m.insert("appearance.blinking".into(), "Blinking".into());
    m.insert("appearance.thickness".into(), "Thickness".into());
    m.insert("appearance.unfocused_hollow".into(), "Unfocused hollow".into());

    // Colors page
    m.insert("colors.section.primary".into(), "Primary".into());
    m.insert("colors.foreground".into(), "Text Color".into());
    m.insert("colors.background".into(), "Background".into());
    m.insert("colors.section.normal".into(), "Normal Colors (0-7)".into());
    m.insert("colors.section.bright".into(), "Bright Colors (8-15)".into());

    // Terminal page
    m.insert("terminal.section.scrolling".into(), "Scrolling".into());
    m.insert("terminal.history".into(), "History (lines)".into());
    m.insert("terminal.scroll_multiplier".into(), "Scroll multiplier".into());
    m.insert("terminal.section.selection".into(), "Selection".into());
    m.insert("terminal.word_separators".into(), "Word separators".into());
    m.insert("terminal.save_to_clipboard".into(), "Save selection to clipboard".into());
    m.insert("terminal.section.mouse".into(), "Mouse".into());
    m.insert("terminal.hide_when_typing".into(), "Hide cursor when typing".into());
    m.insert("terminal.section.bell".into(), "Bell".into());
    m.insert("terminal.bell_duration".into(), "Duration (ms)".into());
    m.insert("terminal.bell_color".into(), "Color".into());

    // SSH page
    m.insert("ssh.add_profile".into(), "+ Add Profile".into());
    m.insert("ssh.delete".into(), "Delete".into());
    m.insert("ssh.name".into(), "Name".into());
    m.insert("ssh.host".into(), "Host".into());
    m.insert("ssh.user".into(), "User".into());
    m.insert("ssh.port".into(), "Port".into());
    m.insert("ssh.key_path".into(), "Key Path".into());
    m.insert("ssh.placeholder.name".into(), "Profile name".into());
    m.insert("ssh.placeholder.host".into(), "hostname or IP".into());
    m.insert("ssh.placeholder.user".into(), "username".into());
    m.insert("ssh.placeholder.key_path".into(), "~/.ssh/id_rsa".into());
    m.insert("ssh.unnamed".into(), "(unnamed)".into());
    m.insert("ssh.no_host".into(), "No host".into());
    m.insert("ssh.select_profile".into(), "Select a profile".into());
    m.insert("ssh.no_profiles".into(), "No profiles. Click '+ Add Profile' to create one.".into());

    // Keybindings page
    m.insert("keybindings.key".into(), "Key".into());
    m.insert("keybindings.modifiers".into(), "Modifiers".into());
    m.insert("keybindings.action".into(), "Action".into());
    m.insert("keybindings.empty".into(), "No custom keyboard bindings configured.".into());
    m.insert("keybindings.default_hint".into(), "Default bindings are built into PureTerm.".into());

    // Footer
    m.insert("footer.save".into(), "Save".into());
    m.insert("footer.revert".into(), "Revert".into());
    m.insert("footer.saved".into(), "Saved".into());
    m.insert("footer.reverted".into(), "Reverted".into());
    m.insert("footer.save_failed".into(), "Save failed: ".into());
    m.insert("footer.revert_failed".into(), "Revert failed: ".into());
    m.insert("footer.unsaved_changes".into(), "Unsaved changes".into());

    // Pick list display labels (config values stay English in TOML)
    m.insert("pick_list.osc52.Disabled".into(), "Disabled".into());
    m.insert("pick_list.osc52.OnlyCopy".into(), "Only Copy".into());
    m.insert("pick_list.osc52.OnlyPaste".into(), "Only Paste".into());
    m.insert("pick_list.osc52.CopyPaste".into(), "Copy & Paste".into());

    m.insert("pick_list.decorations.Full".into(), "Full".into());
    m.insert("pick_list.decorations.Transparent".into(), "Transparent".into());
    m.insert("pick_list.decorations.Buttonless".into(), "Buttonless".into());
    m.insert("pick_list.decorations.None".into(), "None".into());

    m.insert("pick_list.cursor.Block".into(), "Block".into());
    m.insert("pick_list.cursor.Underline".into(), "Underline".into());
    m.insert("pick_list.cursor.Beam".into(), "Beam".into());

    // Tab context menu
    m.insert("context_menu.close_tab".into(), "Close Tab".into());
    m.insert("context_menu.close_other_tabs".into(), "Close Other Tabs".into());
    m.insert("context_menu.close_tabs_left".into(), "Close Tabs to the Left".into());
    m.insert("context_menu.close_tabs_right".into(), "Close Tabs to the Right".into());
    m.insert("context_menu.move_to_new_window".into(), "Move Tab to New Window".into());
    m.insert("context_menu.show_all_tabs".into(), "Show All Tabs".into());
    m.insert("context_menu.rename_tab".into(), "Rename Tab".into());
    m.insert("context_menu.change_tab_color".into(), "Change Tab Color".into());

    // Tab UI
    m.insert("tab.rename_placeholder".into(), "Tab name".into());
    m.insert("tab.clear_color".into(), "Clear".into());
    m.insert("tab.search_placeholder".into(), "Search tabs...".into());
    m.insert("tab.no_match".into(), "No matching tabs".into());

    m
}

fn builtin_zh() -> HashMap<String, String> {
    let mut m = HashMap::new();

    // App
    m.insert("app.title".into(), "PureTerm 设置".into());

    // Sidebar
    m.insert("sidebar.general".into(), "通用".into());
    m.insert("sidebar.appearance".into(), "外观".into());
    m.insert("sidebar.colors".into(), "颜色".into());
    m.insert("sidebar.terminal".into(), "终端".into());
    m.insert("sidebar.ssh_profiles".into(), "SSH 配置".into());
    m.insert("sidebar.shortcuts".into(), "快捷键".into());

    // Page titles
    m.insert("page.general".into(), "通用".into());
    m.insert("page.appearance".into(), "外观".into());
    m.insert("page.colors".into(), "颜色".into());
    m.insert("page.terminal".into(), "终端".into());
    m.insert("page.ssh_profiles".into(), "SSH 配置".into());
    m.insert("page.keybindings".into(), "键盘快捷键".into());

    // General page
    m.insert("general.section.language".into(), "语言".into());
    m.insert("general.language".into(), "语言".into());
    m.insert("general.section.shell".into(), "Shell".into());
    m.insert("general.shell_program".into(), "Shell 程序".into());
    m.insert("general.placeholder.shell".into(), "例如 /bin/zsh".into());
    m.insert("general.working_directory".into(), "工作目录".into());
    m.insert("general.placeholder.working_directory".into(), "默认工作目录".into());
    m.insert("general.section.clipboard".into(), "剪贴板".into());
    m.insert("general.osc52".into(), "OSC 52".into());
    m.insert("general.section.behavior".into(), "行为".into());
    m.insert("general.live_reload".into(), "实时重载配置".into());
    m.insert("general.ipc_socket".into(), "IPC 套接字".into());

    // Appearance page
    m.insert("appearance.section.font".into(), "字体".into());
    m.insert("appearance.family".into(), "字体族".into());
    m.insert("appearance.placeholder.font".into(), "例如 JetBrains Mono".into());
    m.insert("appearance.size".into(), "大小".into());
    m.insert("appearance.section.window".into(), "窗口".into());
    m.insert("appearance.opacity".into(), "不透明度".into());
    m.insert("appearance.background_blur".into(), "背景模糊".into());
    m.insert("appearance.padding".into(), "内边距".into());
    m.insert("appearance.decorations".into(), "窗口装饰".into());
    m.insert("appearance.section.cursor".into(), "光标".into());
    m.insert("appearance.shape".into(), "形状".into());
    m.insert("appearance.blinking".into(), "闪烁".into());
    m.insert("appearance.thickness".into(), "粗细".into());
    m.insert("appearance.unfocused_hollow".into(), "失焦时空心".into());

    // Colors page
    m.insert("colors.section.primary".into(), "主要".into());
    m.insert("colors.foreground".into(), "文字颜色".into());
    m.insert("colors.background".into(), "背景色".into());
    m.insert("colors.section.normal".into(), "标准颜色 (0-7)".into());
    m.insert("colors.section.bright".into(), "亮色 (8-15)".into());

    // Terminal page
    m.insert("terminal.section.scrolling".into(), "滚动".into());
    m.insert("terminal.history".into(), "历史行数".into());
    m.insert("terminal.scroll_multiplier".into(), "滚动倍率".into());
    m.insert("terminal.section.selection".into(), "选择".into());
    m.insert("terminal.word_separators".into(), "分词字符".into());
    m.insert("terminal.save_to_clipboard".into(), "将选中内容保存到剪贴板".into());
    m.insert("terminal.section.mouse".into(), "鼠标".into());
    m.insert("terminal.hide_when_typing".into(), "输入时隐藏光标".into());
    m.insert("terminal.section.bell".into(), "响铃".into());
    m.insert("terminal.bell_duration".into(), "持续时间 (ms)".into());
    m.insert("terminal.bell_color".into(), "颜色".into());

    // SSH page
    m.insert("ssh.add_profile".into(), "+ 添加配置".into());
    m.insert("ssh.delete".into(), "删除".into());
    m.insert("ssh.name".into(), "名称".into());
    m.insert("ssh.host".into(), "主机".into());
    m.insert("ssh.user".into(), "用户".into());
    m.insert("ssh.port".into(), "端口".into());
    m.insert("ssh.key_path".into(), "密钥路径".into());
    m.insert("ssh.placeholder.name".into(), "配置名称".into());
    m.insert("ssh.placeholder.host".into(), "主机名或 IP".into());
    m.insert("ssh.placeholder.user".into(), "用户名".into());
    m.insert("ssh.placeholder.key_path".into(), "~/.ssh/id_rsa".into());
    m.insert("ssh.unnamed".into(), "(未命名)".into());
    m.insert("ssh.no_host".into(), "无主机".into());
    m.insert("ssh.select_profile".into(), "选择一个配置".into());
    m.insert("ssh.no_profiles".into(), "暂无配置。点击「+ 添加配置」创建。".into());

    // Keybindings page
    m.insert("keybindings.key".into(), "按键".into());
    m.insert("keybindings.modifiers".into(), "修饰键".into());
    m.insert("keybindings.action".into(), "动作".into());
    m.insert("keybindings.empty".into(), "未配置自定义键盘快捷键。".into());
    m.insert("keybindings.default_hint".into(), "默认快捷键已内置于 PureTerm。".into());

    // Footer
    m.insert("footer.save".into(), "保存".into());
    m.insert("footer.revert".into(), "还原".into());
    m.insert("footer.saved".into(), "已保存".into());
    m.insert("footer.reverted".into(), "已还原".into());
    m.insert("footer.save_failed".into(), "保存失败：".into());
    m.insert("footer.revert_failed".into(), "还原失败：".into());
    m.insert("footer.unsaved_changes".into(), "有未保存的更改".into());

    // Pick list display labels
    m.insert("pick_list.osc52.Disabled".into(), "禁用".into());
    m.insert("pick_list.osc52.OnlyCopy".into(), "仅复制".into());
    m.insert("pick_list.osc52.OnlyPaste".into(), "仅粘贴".into());
    m.insert("pick_list.osc52.CopyPaste".into(), "复制和粘贴".into());

    m.insert("pick_list.decorations.Full".into(), "完整".into());
    m.insert("pick_list.decorations.Transparent".into(), "透明".into());
    m.insert("pick_list.decorations.Buttonless".into(), "无按钮".into());
    m.insert("pick_list.decorations.None".into(), "无".into());

    m.insert("pick_list.cursor.Block".into(), "方块".into());
    m.insert("pick_list.cursor.Underline".into(), "下划线".into());
    m.insert("pick_list.cursor.Beam".into(), "竖线".into());

    // Tab context menu
    m.insert("context_menu.close_tab".into(), "关闭标签页".into());
    m.insert("context_menu.close_other_tabs".into(), "关闭其他标签页".into());
    m.insert("context_menu.close_tabs_left".into(), "关闭左侧标签页".into());
    m.insert("context_menu.close_tabs_right".into(), "关闭右侧标签页".into());
    m.insert("context_menu.move_to_new_window".into(), "移动到新窗口".into());
    m.insert("context_menu.show_all_tabs".into(), "显示所有标签页".into());
    m.insert("context_menu.rename_tab".into(), "重命名标签页".into());
    m.insert("context_menu.change_tab_color".into(), "修改标签页颜色".into());

    // Tab UI
    m.insert("tab.rename_placeholder".into(), "标签页名称".into());
    m.insert("tab.clear_color".into(), "清除".into());
    m.insert("tab.search_placeholder".into(), "搜索标签页...".into());
    m.insert("tab.no_match".into(), "无匹配标签页".into());

    m
}
