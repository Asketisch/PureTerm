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

    /// Current locale code (e.g. "en", "zh"). Used by page `apply()` to
    /// write back the locale that's actually in effect, not the one
    /// the page was loaded with.
    pub fn current_locale(&self) -> &str {
        &self.locale
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

    /// Localize a key with `{name}` placeholders substituted by `args`.
    /// Unknown placeholders are left in place. Missing args leave the
    /// placeholder in place (so the resulting text is still readable
    /// during incremental translations).
    ///
    /// PureTerm: SSH dialog / settings panel / command palette use
    /// this to interpolate profile names, error messages, etc.
    pub fn t_fmt(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut s = self.t(key);
        for (k, v) in args {
            s = s.replace(&format!("{{{}}}", k), v);
        }
        s
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
    m.insert("appearance.blur_style".into(), "Blur Style".into());
    // PureTerm: 设置窗口自身的磨砂开关（与「Background blur」区分）。
    m.insert("appearance.settings_window_blur".into(), "Settings window blur".into());
    m.insert("appearance.padding".into(), "Padding".into());
    m.insert("appearance.decorations".into(), "Decorations".into());
    m.insert("appearance.section.cursor".into(), "Cursor".into());
    m.insert("appearance.shape".into(), "Shape".into());
    m.insert("appearance.blinking".into(), "Blinking".into());
    m.insert("appearance.thickness".into(), "Thickness".into());
    m.insert("appearance.unfocused_hollow".into(), "Unfocused hollow".into());

    // PureTerm: 背景图设置（仅作用于设置窗口，不影响终端）
    m.insert("appearance.background_image".into(), "Background Image".into());
    m.insert("appearance.background_image_opacity".into(), "Image Opacity".into());
    m.insert("appearance.choose_image".into(), "Choose Image…".into());
    m.insert("appearance.clear".into(), "Clear".into());
    m.insert("appearance.no_image".into(), "(no image selected)".into());

    // Colors page
    m.insert("colors.section.primary".into(), "Primary".into());
    m.insert("colors.foreground".into(), "Text Color".into());
    m.insert("colors.background".into(), "Background".into());
    m.insert("colors.section.normal".into(), "Normal Colors (0-7)".into());
    m.insert("colors.section.bright".into(), "Bright Colors (8-15)".into());
    // PureTerm: 取色器触发按钮文案。
    m.insert("colors.picker_button".into(), "Pick".into());

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

    // PureTerm: blur style pick list
    m.insert("pick_list.blur_style.System".into(), "System Default".into());
    m.insert("pick_list.blur_style.Titlebar".into(), "Titlebar (Light)".into());
    m.insert("pick_list.blur_style.Selection".into(), "Selection".into());
    m.insert("pick_list.blur_style.Menu".into(), "Menu (Medium)".into());
    m.insert("pick_list.blur_style.Sidebar".into(), "Sidebar (Strong)".into());
    m.insert("pick_list.blur_style.HUDWindow".into(), "HUD (Dark)".into());
    m.insert("pick_list.blur_style.FullScreenUI".into(), "Full Screen (Heavy)".into());

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

    // PureTerm: SSH connection dialog (main UI, not settings page)
    m.insert("ssh.dialog.title".into(), "New SSH Connection".into());
    m.insert("ssh.dialog.empty".into(), "No SSH profiles configured.".into());
    m.insert("ssh.dialog.empty_hint".into(), "Edit ~/.config/pureterm/pureterm.toml — add an [[ssh.profiles]] section.".into());
    m.insert("ssh.dialog.error.no_profiles".into(), "No SSH profiles configured. Click 'Open SSH Settings' below to add one, or edit ~/.config/pureterm/pureterm.toml and add an [[ssh.profiles]] entry.".into());
    m.insert("ssh.dialog.button.connect".into(), "Connect".into());
    m.insert("ssh.dialog.button.cancel".into(), "Cancel".into());
    m.insert("ssh.dialog.button.open_settings".into(), "Open SSH Settings".into());

    // PureTerm: SSH settings side panel (right-side editor)
    m.insert("ssh.panel.title".into(), "SSH Profiles".into());
    m.insert("ssh.panel.add".into(), "+ New".into());
    m.insert("ssh.panel.editing".into(), "Editing #{idx}".into());
    m.insert("ssh.panel.label.name".into(), "Name".into());
    m.insert("ssh.panel.label.host".into(), "Host".into());
    m.insert("ssh.panel.label.user".into(), "User (optional)".into());
    m.insert("ssh.panel.label.port".into(), "Port (optional)".into());
    m.insert("ssh.panel.label.key_path".into(), "Key Path (optional)".into());
    m.insert("ssh.panel.label.password".into(), "Password (optional)".into());
    m.insert("ssh.panel.button.save".into(), "Save".into());
    m.insert("ssh.panel.button.delete".into(), "Delete".into());
    m.insert("ssh.panel.placeholder.empty".into(), "Select a profile to edit, or click + New".into());
    m.insert("ssh.panel.profile_not_found".into(), "Profile not found".into());
    m.insert("ssh.new_profile".into(), "New Profile {n}".into());

    // PureTerm: Master password section
    m.insert("ssh.master.title".into(), "Master Password".into());
    m.insert("ssh.master.placeholder".into(), "Enter master password".into());
    m.insert("ssh.master.unlocked".into(), "Credential store unlocked".into());
    m.insert("ssh.master.locked".into(), "Locked — set a master password to encrypt credentials".into());
    m.insert("ssh.master.unlock_fail".into(), "Unlock failed: {error}".into());
    m.insert("ssh.master.set".into(), "Master password set. Click Unlock to use it.".into());
    m.insert("ssh.master.setup_fail".into(), "Setup failed: {error}".into());
    m.insert("ssh.master.unlock".into(), "Unlock / Setup".into());
    m.insert("ssh.master.reunlock".into(), "Re-unlock".into());

    // PureTerm: Command palette
    m.insert("palette.title".into(), "Command Palette (⌘⇧P)".into());
    m.insert("palette.title.profiles".into(), "Pick SSH Profile".into());
    m.insert("palette.placeholder".into(), "Type to search…".into());
    m.insert("palette.empty".into(), "No matches".into());
    m.insert("palette.empty.profiles".into(), "No SSH profiles configured".into());
    m.insert("palette.hint.actions".into(), "↑↓ to move · Enter to run · Esc to close".into());
    m.insert("palette.hint.profiles".into(), "Enter to connect · Esc to back out".into());

    // PureTerm: Command palette — builtin action labels/hints/search
    m.insert("palette.action.new_tab.label".into(), "New Tab".into());
    m.insert("palette.action.new_tab.hint".into(), "Open a fresh local tab".into());
    m.insert("palette.action.new_tab.search".into(), "new tab open local".into());
    m.insert("palette.action.close_tab.label".into(), "Close Tab".into());
    m.insert("palette.action.close_tab.hint".into(), "Close the active tab".into());
    m.insert("palette.action.close_tab.search".into(), "close tab".into());
    m.insert("palette.action.next_tab.label".into(), "Next Tab".into());
    m.insert("palette.action.next_tab.hint".into(), "Switch to the next tab".into());
    m.insert("palette.action.next_tab.search".into(), "next tab cycle".into());
    m.insert("palette.action.prev_tab.label".into(), "Previous Tab".into());
    m.insert("palette.action.prev_tab.hint".into(), "Switch to the previous tab".into());
    m.insert("palette.action.prev_tab.search".into(), "previous tab cycle".into());
    m.insert("palette.action.split_h.label".into(), "Split Horizontal".into());
    m.insert("palette.action.split_h.hint".into(), "Split the active pane horizontally".into());
    m.insert("palette.action.split_h.search".into(), "split horizontal pane".into());
    m.insert("palette.action.split_v.label".into(), "Split Vertical".into());
    m.insert("palette.action.split_v.hint".into(), "Split the active pane vertically".into());
    m.insert("palette.action.split_v.search".into(), "split vertical pane".into());
    m.insert("palette.action.settings.label".into(), "Open Settings".into());
    m.insert("palette.action.settings.hint".into(), "Open the independent settings window".into());
    m.insert("palette.action.settings.search".into(), "settings preferences".into());
    m.insert("palette.action.toggle_vi_mode.label".into(), "Toggle Vi/Copy Mode".into());
    m.insert("palette.action.toggle_vi_mode.hint".into(), "Enter vi mode for keyboard-driven selection".into());
    m.insert("palette.action.toggle_vi_mode.search".into(), "vi copy mode selection vim".into());
    m.insert("palette.action.ssh_connect.label".into(), "SSH: Connect to a server".into());
    m.insert("palette.action.ssh_connect.hint".into(), "Pick a profile and connect".into());
    m.insert("palette.action.ssh_connect.search".into(), "ssh connect new connection".into());
    m.insert("palette.action.ssh_settings.label".into(), "SSH: Open settings panel".into());
    m.insert("palette.action.ssh_settings.hint".into(), "Open the SSH profiles side panel".into());
    m.insert("palette.action.ssh_settings.search".into(), "ssh settings profiles editor".into());
    m.insert("palette.action.ssh_list.label".into(), "SSH: List profiles (jump to profile)".into());
    m.insert("palette.action.ssh_list.hint".into(), "Browse SSH profiles".into());
    m.insert("palette.action.ssh_list.search".into(), "ssh list profiles show".into());

    // PureTerm: Context menu (main UI)
    m.insert("context_menu.new_ssh_tab".into(), "New SSH Tab…".into());

    // PureTerm: SSH errors
    m.insert("ssh.error.invalid_profile".into(), "Invalid profile index: {idx}".into());
    m.insert("ssh.error.manager_not_init".into(), "SSH manager not initialized".into());
    m.insert("ssh.error.start_session".into(), "Failed to start SSH session: {err}".into());
    m.insert("ssh.error.spawn_failed".into(), "ssh spawn failed: {err}".into());

    // PureTerm: Tab title templates
    m.insert("tab.title.format".into(), "Tab {n}".into());
    m.insert("tab.title.ssh".into(), "SSH: {name}".into());
    m.insert("tab.title.ssh_locked".into(), "🔒 SSH: {name}".into());

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
    m.insert("appearance.blur_style".into(), "模糊风格".into());
    // PureTerm: 设置窗口自身的磨砂开关。
    m.insert("appearance.settings_window_blur".into(), "设置窗口模糊".into());
    m.insert("appearance.padding".into(), "内边距".into());
    m.insert("appearance.decorations".into(), "窗口装饰".into());
    m.insert("appearance.section.cursor".into(), "光标".into());
    m.insert("appearance.shape".into(), "形状".into());
    m.insert("appearance.blinking".into(), "闪烁".into());
    m.insert("appearance.thickness".into(), "粗细".into());
    m.insert("appearance.unfocused_hollow".into(), "失焦时空心".into());

    // PureTerm: 背景图设置
    m.insert("appearance.background_image".into(), "背景图片".into());
    m.insert("appearance.background_image_opacity".into(), "图片不透明度".into());
    m.insert("appearance.choose_image".into(), "选择图片…".into());
    m.insert("appearance.clear".into(), "清除".into());
    m.insert("appearance.no_image".into(), "（未选择图片）".into());

    // Colors page
    m.insert("colors.section.primary".into(), "主要".into());
    m.insert("colors.foreground".into(), "文字颜色".into());
    m.insert("colors.background".into(), "背景色".into());
    m.insert("colors.section.normal".into(), "标准颜色 (0-7)".into());
    m.insert("colors.section.bright".into(), "亮色 (8-15)".into());
    // PureTerm: 取色器触发按钮文案。
    m.insert("colors.picker_button".into(), "取色".into());

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

    // PureTerm: 模糊风格选择
    m.insert("pick_list.blur_style.System".into(), "系统默认".into());
    m.insert("pick_list.blur_style.Titlebar".into(), "标题栏（浅）".into());
    m.insert("pick_list.blur_style.Selection".into(), "选中区域".into());
    m.insert("pick_list.blur_style.Menu".into(), "菜单（中）".into());
    m.insert("pick_list.blur_style.Sidebar".into(), "侧边栏（强）".into());
    m.insert("pick_list.blur_style.HUDWindow".into(), "HUD（深色）".into());
    m.insert("pick_list.blur_style.FullScreenUI".into(), "全屏（重）".into());

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

    // PureTerm: SSH 连接对话框（主 UI，非设置页）
    m.insert("ssh.dialog.title".into(), "新建 SSH 连接".into());
    m.insert("ssh.dialog.empty".into(), "暂无 SSH 配置。".into());
    m.insert("ssh.dialog.empty_hint".into(), "编辑 ~/.config/pureterm/pureterm.toml 并添加 [[ssh.profiles]] 配置段。".into());
    m.insert("ssh.dialog.error.no_profiles".into(), "暂无 SSH 配置。点击下方「打开 SSH 设置」添加，或编辑 ~/.config/pureterm/pureterm.toml 并添加 [[ssh.profiles]] 配置段。".into());
    m.insert("ssh.dialog.button.connect".into(), "连接".into());
    m.insert("ssh.dialog.button.cancel".into(), "取消".into());
    m.insert("ssh.dialog.button.open_settings".into(), "打开 SSH 设置".into());

    // PureTerm: SSH 设置侧栏（右侧编辑器）
    m.insert("ssh.panel.title".into(), "SSH 配置".into());
    m.insert("ssh.panel.add".into(), "+ 新建".into());
    m.insert("ssh.panel.editing".into(), "正在编辑 #{idx}".into());
    m.insert("ssh.panel.label.name".into(), "名称".into());
    m.insert("ssh.panel.label.host".into(), "主机".into());
    m.insert("ssh.panel.label.user".into(), "用户（可选）".into());
    m.insert("ssh.panel.label.port".into(), "端口（可选）".into());
    m.insert("ssh.panel.label.key_path".into(), "密钥路径（可选）".into());
    m.insert("ssh.panel.label.password".into(), "密码（可选）".into());
    m.insert("ssh.panel.button.save".into(), "保存".into());
    m.insert("ssh.panel.button.delete".into(), "删除".into());
    m.insert("ssh.panel.placeholder.empty".into(), "选择一个配置进行编辑，或点击「+ 新建」".into());
    m.insert("ssh.panel.profile_not_found".into(), "未找到配置".into());
    m.insert("ssh.new_profile".into(), "新配置 {n}".into());

    // PureTerm: 主密码区
    m.insert("ssh.master.title".into(), "主密码".into());
    m.insert("ssh.master.placeholder".into(), "请输入主密码".into());
    m.insert("ssh.master.unlocked".into(), "凭据存储已解锁".into());
    m.insert("ssh.master.locked".into(), "已锁定 — 请设置主密码以加密凭据".into());
    m.insert("ssh.master.unlock_fail".into(), "解锁失败：{error}".into());
    m.insert("ssh.master.set".into(), "主密码已设置。点击「解锁」以使用。".into());
    m.insert("ssh.master.setup_fail".into(), "设置失败：{error}".into());
    m.insert("ssh.master.unlock".into(), "解锁 / 设置".into());
    m.insert("ssh.master.reunlock".into(), "重新解锁".into());

    // PureTerm: 命令面板
    m.insert("palette.title".into(), "命令面板 (⌘⇧P)".into());
    m.insert("palette.title.profiles".into(), "选择 SSH 配置".into());
    m.insert("palette.placeholder".into(), "输入以搜索…".into());
    m.insert("palette.empty".into(), "无匹配项".into());
    m.insert("palette.empty.profiles".into(), "暂无 SSH 配置".into());
    m.insert("palette.hint.actions".into(), "↑↓ 移动 · Enter 执行 · Esc 关闭".into());
    m.insert("palette.hint.profiles".into(), "Enter 连接 · Esc 返回".into());

    // PureTerm: 命令面板 — 内置 action 标签/提示/搜索关键字
    m.insert("palette.action.new_tab.label".into(), "新建标签页".into());
    m.insert("palette.action.new_tab.hint".into(), "打开新的本地标签页".into());
    m.insert("palette.action.new_tab.search".into(), "新建 标签页 打开".into());
    m.insert("palette.action.close_tab.label".into(), "关闭标签页".into());
    m.insert("palette.action.close_tab.hint".into(), "关闭当前标签页".into());
    m.insert("palette.action.close_tab.search".into(), "关闭 标签页".into());
    m.insert("palette.action.next_tab.label".into(), "下一个标签页".into());
    m.insert("palette.action.next_tab.hint".into(), "切换到下一个标签页".into());
    m.insert("palette.action.next_tab.search".into(), "下一个 标签页 切换".into());
    m.insert("palette.action.prev_tab.label".into(), "上一个标签页".into());
    m.insert("palette.action.prev_tab.hint".into(), "切换到上一个标签页".into());
    m.insert("palette.action.prev_tab.search".into(), "上一个 标签页 切换".into());
    m.insert("palette.action.split_h.label".into(), "水平分屏".into());
    m.insert("palette.action.split_h.hint".into(), "将当前面板水平分屏".into());
    m.insert("palette.action.split_h.search".into(), "分屏 水平".into());
    m.insert("palette.action.split_v.label".into(), "垂直分屏".into());
    m.insert("palette.action.split_v.hint".into(), "将当前面板垂直分屏".into());
    m.insert("palette.action.split_v.search".into(), "分屏 垂直".into());
    m.insert("palette.action.settings.label".into(), "打开设置".into());
    m.insert("palette.action.settings.hint".into(), "打开独立设置窗口".into());
    m.insert("palette.action.settings.search".into(), "设置 偏好".into());
    m.insert("palette.action.toggle_vi_mode.label".into(), "切换 Vi/复制模式".into());
    m.insert("palette.action.toggle_vi_mode.hint".into(), "进入 vi 模式进行键盘驱动的文本选择".into());
    m.insert("palette.action.toggle_vi_mode.search".into(), "vi 复制 模式 选择 vim".into());
    m.insert("palette.action.ssh_connect.label".into(), "SSH：连接服务器".into());
    m.insert("palette.action.ssh_connect.hint".into(), "选择配置并连接".into());
    m.insert("palette.action.ssh_connect.search".into(), "ssh 连接 新建 远程".into());
    m.insert("palette.action.ssh_settings.label".into(), "SSH：打开设置面板".into());
    m.insert("palette.action.ssh_settings.hint".into(), "打开 SSH 配置侧栏".into());
    m.insert("palette.action.ssh_settings.search".into(), "ssh 设置 配置 编辑".into());
    m.insert("palette.action.ssh_list.label".into(), "SSH：列出配置（跳转）".into());
    m.insert("palette.action.ssh_list.hint".into(), "浏览 SSH 配置".into());
    m.insert("palette.action.ssh_list.search".into(), "ssh 列表 配置 显示".into());

    // PureTerm: 右键菜单（主 UI）
    m.insert("context_menu.new_ssh_tab".into(), "新建 SSH Tab…".into());

    // PureTerm: SSH 错误
    m.insert("ssh.error.invalid_profile".into(), "无效的配置索引：{idx}".into());
    m.insert("ssh.error.manager_not_init".into(), "SSH 管理器未初始化".into());
    m.insert("ssh.error.start_session".into(), "启动 SSH 会话失败：{err}".into());
    m.insert("ssh.error.spawn_failed".into(), "ssh 启动失败：{err}".into());

    // PureTerm: Tab 标题模板
    m.insert("tab.title.format".into(), "标签页 {n}".into());
    m.insert("tab.title.ssh".into(), "SSH：{name}".into());
    m.insert("tab.title.ssh_locked".into(), "🔒 SSH：{name}".into());

    m
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_locale_is_english() {
        let i = I18n::new();
        assert_eq!(i.current_locale(), "en");
    }

    #[test]
    fn set_locale_zh_returns_chinese() {
        let mut i = I18n::new();
        i.set_locale("zh");
        assert_eq!(i.current_locale(), "zh");
    }

    #[test]
    fn set_locale_unknown_keeps_current() {
        let mut i = I18n::new();
        i.set_locale("xx-YY"); // not in the table -> ignored
        assert_eq!(i.current_locale(), "en");
    }

    #[test]
    fn t_known_key_returns_value() {
        let i = I18n::new();
        let v = i.t("tab.title.format");
        assert!(v.contains("{n}"));
    }

    #[test]
    fn t_unknown_key_returns_key() {
        let i = I18n::new();
        // Unknown keys should round-trip the input rather than crash.
        let v = i.t("missing.example.key");
        assert_eq!(v, "missing.example.key");
    }

    #[test]
    fn t_fmt_replaces_args() {
        let i = I18n::new();
        let v = i.t_fmt("tab.title.format", &[("n", "5")]);
        assert!(v.contains("5"));
    }

    #[test]
    fn available_locales_lists_known() {
        let i = I18n::new();
        let locs = i.available_locales();
        let codes: Vec<&str> = locs.iter().map(|(c, _)| *c).collect();
        assert!(codes.contains(&"en"));
        assert!(codes.contains(&"zh"));
    }

    #[test]
    fn locale_fallback_when_key_missing() {
        let mut i = I18n::new();
        i.set_locale("zh");
        // Keys missing in zh should fall back to en.
        let v = i.t("missing.key");
        assert_eq!(v, "missing.key");
    }
}
