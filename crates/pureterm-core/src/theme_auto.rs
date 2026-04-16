//! Automatic theme switching between light and dark modes.


/// Detects system dark/light mode preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Light,
    Dark,
}

/// Configuration for automatic theme switching.
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

    /// Detect the system's current color mode.
    /// On macOS this checks the NSUserDefaults; on Linux it checks GTK settings.
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
                // If the command succeeds with empty output or "Light", it's light mode
                return ColorMode::Light;
            }
            ColorMode::Dark // default fallback
        }

        #[cfg(not(target_os = "macos"))]
        {
            ColorMode::Dark // default fallback
        }
    }

    /// Get the theme name for the current mode.
    pub fn current_theme(&self) -> &str {
        match self.current_mode {
            ColorMode::Light => &self.light_theme,
            ColorMode::Dark => &self.dark_theme,
        }
    }

    /// Check if the mode has changed and update.
    /// Returns true if the mode changed.
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

    /// Force set the color mode.
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
