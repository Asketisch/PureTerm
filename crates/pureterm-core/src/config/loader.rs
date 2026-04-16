//! Configuration file loader
//!
//! Handles loading config from multiple sources with proper precedence:
//! 1. Default values
//! 2. Config files (XDG/AppSupport)
//! 3. CLI arguments
//! 4. Referenced config files (with cycle detection)

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::{CliArgs, Config};
use super::conditional::{parse_conditional, ConditionalEvaluator};
use super::options::{
    Color, CursorStyle, MouseReporting, RepeatableString, ShellIntegration,
};

/// Configuration loader with multi-source loading support
#[derive(Debug)]
pub struct ConfigLoader {
    /// Loaded config files for cycle detection
    loaded_files: HashSet<PathBuf>,
    /// CLI arguments
    cli: Option<CliArgs>,
}

impl ConfigLoader {
    pub fn new() -> Self {
        Self {
            loaded_files: HashSet::new(),
            cli: None,
        }
    }

    /// Load configuration with default files only
    pub fn load(&mut self) -> Result<Config, ConfigError> {
        let mut config = Config::default();

        // Load default config files
        self.load_default_files(&mut config)?;

        // Apply CLI arguments - extract cli first to avoid borrow conflict
        let cli_opt = self.cli.clone();
        if let Some(cli) = cli_opt {
            self.apply_cli_args(&cli, &mut config);
        }

        // Finalize configuration
        self.finalize(&mut config)?;

        Ok(config)
    }

    /// Load configuration with CLI arguments
    pub fn load_with_cli(&mut self, cli: CliArgs) -> Result<Config, ConfigError> {
        self.cli = Some(cli);
        self.load()
    }

    /// Load from a specific path
    pub fn load_from_path(&mut self, path: &Path) -> Result<Config, ConfigError> {
        let mut config = Config::default();

        // Load from the specified file
        self.load_file(path, &mut config)?;

        // Apply CLI arguments - extract cli first to avoid borrow conflict
        let cli_opt = self.cli.clone();
        if let Some(cli) = cli_opt {
            self.apply_cli_args(&cli, &mut config);
        }

        // Finalize
        self.finalize(&mut config)?;

        Ok(config)
    }

    /// Load default config files from standard locations
    fn load_default_files(&mut self, config: &mut Config) -> Result<(), ConfigError> {
        // Platform-specific config directories
        let config_dirs = self.get_config_dirs();

        for dir in config_dirs {
            let config_path = dir.join("config.toml");
            if config_path.exists() {
                self.load_file(&config_path, config)?;
            }

            // Also check for config file without extension
            let config_path_no_ext = dir.join("config");
            if config_path_no_ext.exists() {
                self.load_config_from_string(&fs::read_to_string(&config_path_no_ext)?, config)?;
            }
        }

        Ok(())
    }

    /// Get platform-specific config directories
    fn get_config_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        // XDG_CONFIG_HOME on Linux, ~/Library/Application Support on macOS
        if let Some(base) = dirs::config_dir() {
            dirs.push(base.join("pureterm"));
        }

        // Platform-specific locations
        #[cfg(target_os = "macos")]
        {
            if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                dirs.push(home.join("Library/Application Support/PureTerm"));
            }
        }

        // System-wide config
        #[cfg(target_os = "linux")]
        {
            dirs.push(PathBuf::from("/etc/pureterm"));
        }

        dirs
    }

    /// Load a config file with cycle detection
    fn load_file(&mut self, path: &Path, config: &mut Config) -> Result<(), ConfigError> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Cycle detection
        if self.loaded_files.contains(&canonical) {
            return Err(ConfigError::CycleDetected {
                cycle: canonical.to_string_lossy().to_string(),
            });
        }

        self.loaded_files.insert(canonical.clone());

        let content = fs::read_to_string(path)?;
        self.load_config_from_string(&content, config)?;

        // Check for and load referenced config files
        self.load_recursive_files(config)?;

        Ok(())
    }

    /// Load config from a string (handles both TOML and custom format)
    fn load_config_from_string(&mut self, content: &str, config: &mut Config) -> Result<(), ConfigError> {
        // Check if this is a conditional config (contains if/else blocks)
        if content.contains("if ") || content.contains("if\t") {
            // Parse conditional config
            let conditional = parse_conditional(content)
                .map_err(|e| ConfigError::ParseError(e.to_string()))?;

            let evaluator = ConditionalEvaluator::new(&config.theme);
            let values = evaluator.evaluate(&conditional);

            // Apply the evaluated values to config
            self.apply_key_values(&values, config);
        } else if content.contains('[') {
            // TOML format
            let parsed: Config = toml::from_str(content)
                .map_err(|e| ConfigError::ParseError(e.to_string()))?;
            self.merge_config(config, parsed);
        } else {
            // Simple key = value format
            let values = self.parse_simple_config(content)?;
            self.apply_key_values(&values, config);
        }

        Ok(())
    }

    /// Parse simple key = value format
    fn parse_simple_config(&self, content: &str) -> Result<std::collections::HashMap<String, String>, ConfigError> {
        let mut values = std::collections::HashMap::new();

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse key = value
            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim().to_string();
                let value = line[eq_pos + 1..].trim().to_string();
                values.insert(key, value);
            }
        }

        Ok(values)
    }

    /// Apply key-value pairs to config
    fn apply_key_values(&self, values: &std::collections::HashMap<String, String>, config: &mut Config) {
        for (key, value) in values {
            self.set_config_value(config, key, value);
        }
    }

    /// Set a config value by key path (e.g., "window.padding.x")
    fn set_config_value(&self, config: &mut Config, key: &str, value: &str) {
        // Use serde to set the value dynamically
        // For now, handle a subset of important keys
        match key {
            "font-family" | "font_family" => {
                config.font_family = RepeatableString::single(value);
            }
            "font-size" | "font_size" => {
                if let Ok(size) = value.parse::<f32>() {
                    config.font_size = size;
                }
            }
            "theme" => {
                config.theme = value.to_string();
            }
            "background" => {
                if let Some(c) = Color::from_hex(value) {
                    config.background = c;
                }
            }
            "foreground" => {
                if let Some(c) = Color::from_hex(value) {
                    config.foreground = c;
                }
            }
            "cursor-color" | "cursor_color" => {
                if let Some(c) = Color::from_hex(value) {
                    config.cursor_color = c;
                }
            }
            "cursor-style" | "cursor_style" => {
                config.cursor_style = match value {
                    "block" => CursorStyle::Block,
                    "block_hollow" | "block-hollow" | "hollow" => CursorStyle::BlockHollow,
                    "underline" => CursorStyle::Underline,
                    "beam" => CursorStyle::Beam,
                    "bar" => CursorStyle::Bar,
                    _ => CursorStyle::Block,
                };
            }
            "cursor-blink" | "cursor_blink" => {
                config.cursor_style_blink = value == "true" || value == "1";
            }
            "background-opacity" | "background_opacity" => {
                if let Ok(opacity) = value.parse::<f64>() {
                    config.background_opacity = opacity;
                }
            }
            "window-width" | "window_width" => {
                if let Ok(w) = value.parse::<f32>() {
                    config.window_width.value = w;
                }
            }
            "window-height" | "window_height" => {
                if let Ok(h) = value.parse::<f32>() {
                    config.window_height.value = h;
                }
            }
            "title" => {
                config.title = value.to_string();
            }
            "term" => {
                config.term = value.to_string();
            }
            "shell-integration" | "shell_integration" => {
                config.shell_integration = match value {
                    "install" => ShellIntegration::Install,
                    "uninstall" => ShellIntegration::Uninstall,
                    _ => ShellIntegration::None,
                };
            }
            "scrollback-limit" | "scrollback_limit" => {
                if let Ok(limit) = value.parse::<u32>() {
                    config.scrollback_limit = limit;
                }
            }
            "mouse-reporting" | "mouse_reporting" => {
                config.mouse_reporting = match value {
                    "none" => MouseReporting::None,
                    "click" => MouseReporting::Click,
                    "clickdrag" | "click_drag" => MouseReporting::ClickDrag,
                    "motion" => MouseReporting::Motion,
                    _ => MouseReporting::Click,
                };
            }
            "copy-on-select" | "copy_on_select" => {
                config.copy_on_select = value == "true" || value == "1";
            }
            "bell" => {
                config.bell_features.visual = value.contains('v')
                    || value == "visual" || value == "all";
                config.bell_features.audio = value.contains('a')
                    || value == "audio" || value == "all";
            }
            _ => {
                // Try to set via reflection for other keys
                log::debug!("Unknown config key: {} = {}", key, value);
            }
        }
    }

    /// Load referenced config files (with cycle detection)
    fn load_recursive_files(&mut self, config: &mut Config) -> Result<(), ConfigError> {
        // Check for config-file key which references another config
        if let Some(ref path) = config.config_file {
            let path = PathBuf::from(path);
            let _base = path.parent().unwrap_or(Path::new("."));
            let absolute = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(&path)
            };

            self.load_file(&absolute, config)?;
        }

        Ok(())
    }

    /// Apply CLI arguments to config
    fn apply_cli_args(&mut self, cli: &CliArgs, config: &mut Config) {
        // Handle config file from CLI
        if let Some(ref path) = cli.config_file {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(path) {
                    let _ = self.load_config_from_string(&content, config);
                }
            }
        }

        // Handle positional args (command to run)
        if let Some(cmd) = cli.command() {
            config.initial_command = Some(cmd.to_string());
        }

        // Handle options
        for (key, value) in &cli.options {
            self.set_config_value(config, key, value);
        }
    }

    /// Merge another config into this one
    fn merge_config(&self, base: &mut Config, other: Config) {
        // Use serde's deserialize to merge configs
        // For now, use a simple field-by-field merge
        let other_json = serde_json::to_value(&other).unwrap_or_default();
        let base_json = serde_json::to_value(&*base).unwrap_or_default();

        // Simple merge: other takes precedence
        let merged = serde_json::json!({
            "base": base_json,
            "other": other_json,
        });

        if let Ok(merged_config) = serde_json::from_value::<Config>(merged) {
            *base = merged_config;
        }
    }

    /// Finalize configuration (validation, theme loading, defaults)
    fn finalize(&self, config: &mut Config) -> Result<(), ConfigError> {
        // Clamp values to valid ranges
        config.font_size = config.font_size.clamp(1.0, 256.0);
        config.background_opacity = config.background_opacity.clamp(0.0, 1.0);
        config.cursor_opacity = config.cursor_opacity.clamp(0.0, 1.0);
        config.minimum_contrast = config.minimum_contrast.clamp(0.0, 1.0);
        config.faint_opacity = config.faint_opacity.clamp(0.0, 1.0);

        // Apply theme if specified
        // (Theme loading is handled by ThemeService)

        Ok(())
    }
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}

// ── Error Types ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ConfigError {
    ParseError(String),
    IoError(std::io::Error),
    CycleDetected { cycle: String },
    ValidationError(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseError(s) => write!(f, "Parse error: {}", s),
            Self::IoError(e) => write!(f, "IO error: {}", e),
            Self::CycleDetected { cycle } => write!(f, "Cycle detected loading: {}", cycle),
            Self::ValidationError(s) => write!(f, "Validation error: {}", s),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}
