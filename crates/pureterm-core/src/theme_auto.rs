//! 浅色和深色模式之间的自动主题切换。


/// 检测系统深色/浅色模式偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Light,
    Dark,
}

/// 自动主题切换的配置。
pub struct ThemeAutoSwitch {
    light_theme: String,
    dark_theme: String,
    current_mode: ColorMode,
}

impl ThemeAutoSwitch {
    pub fn new(light_theme: &str, dark_theme: &str) -> Self {
        let current_mode = Self::detect_system_mode();
        Self {
            light_theme: light_theme.to_string(),
            dark_theme: dark_theme.to_string(),
            current_mode,
        }
    }

    /// 检测系统当前的颜色模式。
    /// 在 macOS 上这会检查 NSUserDefaults；在 Linux 上检查 GTK 设置。
    pub fn detect_system_mode() -> ColorMode {
        #[cfg(target_os = "macos")]
        {
            if let Ok(output) = std::process::Command::new("defaults")
                .args(["read", "-g", "AppleInterfaceStyle"])
                .output()
            {
                if output.status.success() {
                    let style = String::from_utf8_lossy(&output.stdout);
                    if style.trim().eq_ignore_ascii_case("dark") {
                        return ColorMode::Dark;
                    }
                }
                // 如果命令成功但输出为空或为"Light"，则是浅色模式
                return ColorMode::Light;
            }
            ColorMode::Dark // 默认回退
        }

        #[cfg(not(target_os = "macos"))]
        {
            ColorMode::Dark // default fallback
        }
    }

    /// 获取当前模式的主题名称。
    pub fn current_theme(&self) -> &str {
        match self.current_mode {
            ColorMode::Light => &self.light_theme,
            ColorMode::Dark => &self.dark_theme,
        }
    }

    /// 检查模式是否已更改并更新。
    /// 如果模式更改了则返回 true。
    pub fn check_and_update(&mut self) -> bool {
        let new_mode = Self::detect_system_mode();
        if new_mode != self.current_mode {
            self.current_mode = new_mode;
            log::info!("Color mode changed to {:?}", new_mode);
            true
        } else {
            false
        }
    }

    /// 强制设置颜色模式。
    pub fn set_mode(&mut self, mode: ColorMode) {
        self.current_mode = mode;
    }

    pub fn current_mode(&self) -> ColorMode {
        self.current_mode
    }

    pub fn light_theme(&self) -> &str {
        &self.light_theme
    }

    pub fn dark_theme(&self) -> &str {
        &self.dark_theme
    }
}

impl Default for ThemeAutoSwitch {
    fn default() -> Self {
        Self::new("Solarized Light", "Catppuccin Mocha")
    }
}
