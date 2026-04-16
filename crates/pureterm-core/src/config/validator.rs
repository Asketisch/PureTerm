//! Configuration validator
//!
//! Validates config values and provides helpful error messages.

use super::Config;

/// Configuration validation result
#[derive(Debug)]
pub struct ValidationResult {
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn merge(&mut self, other: Self) {
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
    }
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

#[derive(Debug)]
pub struct ValidationWarning {
    pub field: String,
    pub message: String,
}

/// Validate a complete configuration
pub struct ConfigValidator;

impl ConfigValidator {
    /// Validate the entire config
    pub fn validate(config: &Config) -> ValidationResult {
        let mut result = ValidationResult::new();

        // Validate fonts
        result.merge(Self::validate_fonts(config));

        // Validate colors
        result.merge(Self::validate_colors(config));

        // Validate window settings
        result.merge(Self::validate_window(config));

        // Validate mouse/clipboard
        result.merge(Self::validate_input(config));

        // Validate shell/command
        result.merge(Self::validate_command(config));

        // Validate platform-specific
        result.merge(Self::validate_platform(config));

        result
    }

    fn validate_fonts(config: &Config) -> ValidationResult {
        let mut result = ValidationResult::new();

        if config.font_size < 1.0 || config.font_size > 256.0 {
            result.errors.push(ValidationError {
                field: "font-size".to_string(),
                message: format!(
                    "font-size must be between 1 and 256, got {}",
                    config.font_size
                ),
            });
        }

        if config.font_thicken_strength < 0.0 || config.font_thicken_strength > 1.0 {
            result.warnings.push(ValidationWarning {
                field: "font-thicken-strength".to_string(),
                message: format!(
                    "font-thicken-strength should be between 0 and 1, got {}",
                    config.font_thicken_strength
                ),
            });
        }

        result
    }

    fn validate_colors(config: &Config) -> ValidationResult {
        let mut result = ValidationResult::new();

        if config.background_opacity < 0.0 || config.background_opacity > 1.0 {
            result.errors.push(ValidationError {
                field: "background-opacity".to_string(),
                message: format!(
                    "background-opacity must be between 0 and 1, got {}",
                    config.background_opacity
                ),
            });
        }

        if config.cursor_opacity < 0.0 || config.cursor_opacity > 1.0 {
            result.errors.push(ValidationError {
                field: "cursor-opacity".to_string(),
                message: format!(
                    "cursor-opacity must be between 0 and 1, got {}",
                    config.cursor_opacity
                ),
            });
        }

        if config.minimum_contrast < 0.0 || config.minimum_contrast > 1.0 {
            result.errors.push(ValidationError {
                field: "minimum-contrast".to_string(),
                message: format!(
                    "minimum-contrast must be between 0 and 1, got {}",
                    config.minimum_contrast
                ),
            });
        }

        if config.faint_opacity < 0.0 || config.faint_opacity > 1.0 {
            result.errors.push(ValidationError {
                field: "faint-opacity".to_string(),
                message: format!(
                    "faint-opacity must be between 0 and 1, got {}",
                    config.faint_opacity
                ),
            });
        }

        result
    }

    fn validate_window(config: &Config) -> ValidationResult {
        let mut result = ValidationResult::new();

        if config.window_width.value <= 0.0 {
            result.errors.push(ValidationError {
                field: "window-width".to_string(),
                message: format!(
                    "window-width must be positive, got {}",
                    config.window_width.value
                ),
            });
        }

        if config.window_height.value <= 0.0 {
            result.errors.push(ValidationError {
                field: "window-height".to_string(),
                message: format!(
                    "window-height must be positive, got {}",
                    config.window_height.value
                ),
            });
        }

        if config.scrollback_limit > 1_000_000 {
            result.warnings.push(ValidationWarning {
                field: "scrollback-limit".to_string(),
                message: format!(
                    "scrollback-limit is very high ({}), may use significant memory",
                    config.scrollback_limit
                ),
            });
        }

        result
    }

    fn validate_input(config: &Config) -> ValidationResult {
        let mut result = ValidationResult::new();

        if config.mouse_scroll_multiplier <= 0.0 {
            result.errors.push(ValidationError {
                field: "mouse-scroll-multiplier".to_string(),
                message: format!(
                    "mouse-scroll-multiplier must be positive, got {}",
                    config.mouse_scroll_multiplier
                ),
            });
        }

        if config.click_repeat_interval == 0 {
            result.errors.push(ValidationError {
                field: "click-repeat-interval".to_string(),
                message: "click-repeat-interval must be non-zero".to_string(),
            });
        }

        result
    }

    fn validate_command(config: &Config) -> ValidationResult {
        let mut result = ValidationResult::new();

        if config.undo_timeout == 0 {
            result.warnings.push(ValidationWarning {
                field: "undo-timeout".to_string(),
                message: "undo-timeout is 0, undo will be disabled".to_string(),
            });
        }

        result
    }

    fn validate_platform(config: &Config) -> ValidationResult {
        let mut result = ValidationResult::new();

        #[cfg(target_os = "macos")]
        {
            if config.macos_option_as_alt && config.macos_option_as_alt {
                result.warnings.push(ValidationWarning {
                    field: "macos-option-as-alt".to_string(),
                    message: "macos-option-as-alt is enabled, option key will act as alt"
                        .to_string(),
                });
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            if !config.macos_non_native_fullscreen {
                // macOS-specific options on non-macOS platform
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_config() {
        let config = Config::default();
        let result = ConfigValidator::validate(&config);
        assert!(result.is_valid());
    }

    #[test]
    fn test_validate_invalid_font_size() {
        let config = Config {
            font_size: 500.0,
            ..Default::default()
        };
        let result = ConfigValidator::validate(&config);
        assert!(!result.is_valid());
        assert!(result.errors.iter().any(|e| e.field == "font-size"));
    }

    #[test]
    fn test_validate_invalid_opacity() {
        let config = Config {
            background_opacity: 1.5,
            ..Default::default()
        };
        let result = ConfigValidator::validate(&config);
        assert!(!result.is_valid());
        assert!(result.errors.iter().any(|e| e.field == "background-opacity"));
    }
}
