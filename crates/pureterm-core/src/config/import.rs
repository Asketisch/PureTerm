//! Settings import from iTerm2 and Alacritty.
//!
//! Parses configuration from other terminal emulators and converts them
//! into PureTerm's `AppConfig` format.

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use super::AppConfig;
use crate::theme::ColorScheme;

// ── Import Result ─────────────────────────────────────────────────────────────

/// Result of an import operation.
#[derive(Debug)]
pub struct ImportResult {
    /// The imported config.
    pub config: AppConfig,
    /// Number of settings successfully imported.
    pub imported_count: usize,
    /// Warnings encountered during import.
    pub warnings: Vec<String>,
}

// ── iTerm2 Import ─────────────────────────────────────────────────────────────

/// iTerm2 profile parsed from plist JSON.
#[derive(Debug, Deserialize, Default)]
struct Iterm2Profile {
    name: Option<String>,
    #[serde(rename = "Normal Font")]
    normal_font: Option<String>,
    #[serde(rename = "Font Size")]
    font_size: Option<f64>,
    #[serde(rename = "Cursor Type")]
    cursor_type: Option<i64>,
    #[serde(rename = "Cursor Bold")]
    cursor_bold: Option<bool>,
    #[serde(rename = "Blinking Cursor")]
    blinking_cursor: Option<bool>,
    #[serde(rename = "Scrollback Lines")]
    scrollback_lines: Option<i64>,
    #[serde(rename = "Transparency")]
    transparency: Option<f64>,
    #[serde(rename = "Background Color")]
    background_color: Option<Iterm2Color>,
    #[serde(rename = "Foreground Color")]
    foreground_color: Option<Iterm2Color>,
    #[serde(rename = "Cursor Color")]
    cursor_color: Option<Iterm2Color>,
    #[serde(rename = "Bold Color")]
    bold_color: Option<Iterm2Color>,
    #[serde(rename = "Selection Color")]
    selection_color: Option<Iterm2Color>,
    #[serde(rename = "Selected Text Color")]
    selected_text_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 0 Color")]
    ansi_0_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 1 Color")]
    ansi_1_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 2 Color")]
    ansi_2_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 3 Color")]
    ansi_3_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 4 Color")]
    ansi_4_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 5 Color")]
    ansi_5_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 6 Color")]
    ansi_6_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 7 Color")]
    ansi_7_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 8 Color")]
    ansi_8_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 9 Color")]
    ansi_9_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 10 Color")]
    ansi_10_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 11 Color")]
    ansi_11_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 12 Color")]
    ansi_12_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 13 Color")]
    ansi_13_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 14 Color")]
    ansi_14_color: Option<Iterm2Color>,
    #[serde(rename = "Ansi 15 Color")]
    ansi_15_color: Option<Iterm2Color>,
    #[serde(rename = "Use Bold Font")]
    use_bold_font: Option<bool>,
    #[serde(rename = "Use Non-ASCII Font")]
    use_non_ascii_font: Option<bool>,
    #[serde(rename = "ASCII Anti Aliased")]
    ascii_anti_aliased: Option<bool>,
    #[serde(rename = "Ambiguous Double Width")]
    ambiguous_double_width: Option<bool>,
    #[serde(rename = "Silence Bell")]
    silence_bell: Option<bool>,
    #[serde(rename = "Visual Bell")]
    visual_bell: Option<bool>,
    #[serde(rename = "Columns")]
    columns: Option<i64>,
    #[serde(rename = "Rows")]
    rows: Option<i64>,
}

/// iTerm2 color component (from plist JSON).
#[derive(Debug, Deserialize, Default)]
struct Iterm2Color {
    #[serde(rename = "Red Component")]
    red_component: Option<f64>,
    #[serde(rename = "Green Component")]
    green_component: Option<f64>,
    #[serde(rename = "Blue Component")]
    blue_component: Option<f64>,
    #[serde(rename = "Alpha Component")]
    alpha_component: Option<f64>,
}

impl Iterm2Color {
    /// Convert to hex color string like "#rrggbb".
    fn to_hex(&self) -> Option<String> {
        let r = self.red_component?;
        let g = self.green_component?;
        let b = self.blue_component?;
        Some(format!(
            "#{:02x}{:02x}{:02x}",
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
        ))
    }
}

/// Import settings from iTerm2.
///
/// Parses iTerm2's `Profiles.plist` file by converting it to JSON via `plutil`,
/// then extracting relevant settings.
pub fn import_from_iterm2() -> Result<ImportResult, String> {
    let plist_path = get_iterm2_plist_path()?;

    // Use plutil to convert plist to JSON
    let output = std::process::Command::new("plutil")
        .args(["-convert", "json", "-o", "-", &plist_path.to_string_lossy()])
        .output()
        .map_err(|e| format!("Failed to run plutil: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "plutil failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let plist: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse plist JSON: {}", e))?;

    // Extract profiles from the plist structure
    let profiles = extract_iterm2_profiles(&plist);

    if profiles.is_empty() {
        return Err("No iTerm2 profiles found".into());
    }

    // Use the first (default) profile
    let profile = &profiles[0];
    let mut config = AppConfig::default();
    let mut imported_count = 0;
    let mut warnings = Vec::new();

    // Font settings
    if let Some(ref font_str) = profile.normal_font {
        // iTerm2 font format: "FontName Size", e.g. "Menlo-Regular 14"
        if let Some(font_name) = font_str.rsplit_once(' ').map(|(name, _)| name) {
            config.terminal.font_family = clean_font_name(font_name);
            imported_count += 1;
        }
    }

    if let Some(size) = profile.font_size {
        config.terminal.font_size = size as u16;
        config.terminal.font_size_mac = size as u16;
        imported_count += 1;
    }

    // Cursor settings
    if let Some(cursor_type) = profile.cursor_type {
        config.terminal.cursor_style = match cursor_type {
            0 => "block".to_string(),
            1 => "vertical".to_string(),
            2 => "underline".to_string(),
            _ => {
                warnings.push(format!("Unknown iTerm2 cursor type: {}", cursor_type));
                "block".to_string()
            }
        };
        imported_count += 1;
    }

    if let Some(blinking) = profile.blinking_cursor {
        config.terminal.cursor_blink = blinking;
        imported_count += 1;
    }

    // Scrollback
    if let Some(lines) = profile.scrollback_lines {
        config.terminal.scrollback_lines = lines as u32;
        imported_count += 1;
    }

    // Opacity/transparency
    if let Some(transparency) = profile.transparency {
        if transparency > 0.0 {
            config.appearance.opacity = (1.0 - transparency as f32).max(0.1);
            imported_count += 1;
        }
    }

    // Bell settings
    if profile.silence_bell.unwrap_or(false) {
        config.terminal.bell = "none".to_string();
        imported_count += 1;
    } else if profile.visual_bell.unwrap_or(false) {
        config.terminal.bell = "visual".to_string();
        imported_count += 1;
    }

    // Build color scheme from profile colors
    let has_bg = profile.background_color.is_some();
    let has_fg = profile.foreground_color.is_some();

    if has_bg || has_fg {
        let mut scheme_name = "iTerm2 Imported".to_string();
        if let Some(ref name) = profile.name {
            scheme_name = format!("iTerm2: {}", name);
        }

        let background = profile
            .background_color
            .as_ref()
            .and_then(|c| c.to_hex())
            .unwrap_or_else(|| "#000000".to_string());

        let foreground = profile
            .foreground_color
            .as_ref()
            .and_then(|c| c.to_hex())
            .unwrap_or_else(|| "#ffffff".to_string());

        let cursor = profile
            .cursor_color
            .as_ref()
            .and_then(|c| c.to_hex())
            .unwrap_or_else(|| "#ffffff".to_string());

        let selection_bg = profile
            .selection_color
            .as_ref()
            .and_then(|c| c.to_hex())
            .unwrap_or_else(|| "#4d4d4d".to_string());

        let selection_fg = profile
            .selected_text_color
            .as_ref()
            .and_then(|c| c.to_hex())
            .unwrap_or_else(|| foreground.clone());

        // Build ANSI palette from the 16 ANSI color slots
        let ansi_sources: Vec<Option<&Iterm2Color>> = vec![
            profile.ansi_0_color.as_ref(),
            profile.ansi_1_color.as_ref(),
            profile.ansi_2_color.as_ref(),
            profile.ansi_3_color.as_ref(),
            profile.ansi_4_color.as_ref(),
            profile.ansi_5_color.as_ref(),
            profile.ansi_6_color.as_ref(),
            profile.ansi_7_color.as_ref(),
            profile.ansi_8_color.as_ref(),
            profile.ansi_9_color.as_ref(),
            profile.ansi_10_color.as_ref(),
            profile.ansi_11_color.as_ref(),
            profile.ansi_12_color.as_ref(),
            profile.ansi_13_color.as_ref(),
            profile.ansi_14_color.as_ref(),
            profile.ansi_15_color.as_ref(),
        ];

        let default_colors: [&str; 16] = [
            "#000000", "#cc0000", "#00cc00", "#cccc00", "#0000cc", "#cc00cc", "#00cccc", "#cccccc",
            "#555555", "#ff5555", "#55ff55", "#ffff55", "#5555ff", "#ff55ff", "#55ffff", "#ffffff",
        ];

        let colors: [String; 16] = std::array::from_fn(|i| {
            ansi_sources[i]
                .and_then(|c| c.to_hex())
                .unwrap_or_else(|| default_colors[i].to_string())
        });

        let color_scheme = ColorScheme {
            name: scheme_name,
            foreground,
            background,
            cursor,
            selection_background: selection_bg,
            selection_foreground: selection_fg,
            colors,
        };

        config.terminal.color_scheme = Some(color_scheme);
        imported_count += 1;
    }

    Ok(ImportResult {
        config,
        imported_count,
        warnings,
    })
}

/// Get the path to iTerm2's plist file.
fn get_iterm2_plist_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let path = home.join("Library/Preferences/com.googlecode.iterm2.plist");
    if path.exists() {
        Ok(path)
    } else {
        Err(format!("iTerm2 plist not found at {:?}", path))
    }
}

/// Extract profiles from iTerm2 plist JSON structure.
fn extract_iterm2_profiles(plist: &serde_json::Value) -> Vec<Iterm2Profile> {
    let mut profiles = Vec::new();

    // Pattern 1: "Profiles" -> "Bookmark" -> [array of profiles]
    if let Some(profiles_obj) = plist.get("Profiles") {
        if let Some(bookmark) = profiles_obj.get("Bookmark") {
            if let Some(arr) = bookmark.as_array() {
                for item in arr {
                    if let Ok(p) = serde_json::from_value::<Iterm2Profile>(item.clone()) {
                        profiles.push(p);
                    }
                }
            }
        }
    }

    // Pattern 2: "New Bookmarks" array
    if profiles.is_empty() {
        if let Some(profiles_dict) = plist
            .get("New Bookmarks")
            .or_else(|| plist.get("Profiles"))
        {
            if let Some(arr) = profiles_dict.as_array() {
                for item in arr {
                    if let Ok(p) = serde_json::from_value::<Iterm2Profile>(item.clone()) {
                        profiles.push(p);
                    }
                }
            }
        }
    }

    profiles
}

/// Clean up a font name from iTerm2 format.
fn clean_font_name(name: &str) -> String {
    // Remove -Regular, -Bold, -Light suffixes for the family name
    let name = name.trim();
    for suffix in &["-Regular", "-Bold", "-Light", "-Medium", "-Thin"] {
        if name.ends_with(suffix) {
            return name[..name.len() - suffix.len()].to_string();
        }
    }
    name.to_string()
}

// ── Alacritty Import ──────────────────────────────────────────────────────────

/// Alacritty config structure (partial, for import).
#[derive(Debug, Deserialize, Default)]
struct AlacrittyConfig {
    #[serde(default)]
    font: AlacrittyFont,
    #[serde(default)]
    colors: AlacrittyColors,
    #[serde(default)]
    cursor: AlacrittyCursor,
    #[serde(default)]
    scrolling: AlacrittyScrolling,
    #[serde(default)]
    window: AlacrittyWindow,
    #[serde(default)]
    selection: AlacrittySelection,
    #[serde(default)]
    bell: AlacrittyBell,
    #[serde(default, rename = "background_opacity")]
    background_opacity: Option<f64>,
    #[serde(default)]
    env: std::collections::HashMap<String, String>,
    #[serde(default)]
    shell: Option<AlacrittyShell>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyFont {
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    size: Option<f64>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    bold: Option<AlacrittyFontDetail>,
    #[serde(default)]
    italic: Option<AlacrittyFontDetail>,
    #[serde(default)]
    bold_italic: Option<AlacrittyFontDetail>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyFontDetail {
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    style: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyColors {
    #[serde(default)]
    primary: Option<AlacrittyPrimaryColors>,
    #[serde(default)]
    cursor: Option<AlacrittyCursorColors>,
    #[serde(default)]
    selection: Option<AlacrittySelectionColors>,
    #[serde(default)]
    normal: Option<AlacrittyAnsiColors>,
    #[serde(default)]
    bright: Option<AlacrittyAnsiColors>,
    #[serde(default)]
    dim: Option<AlacrittyAnsiColors>,
    #[serde(default)]
    indexed_colors: Vec<AlacrittyIndexedColor>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyPrimaryColors {
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    foreground: Option<String>,
    #[serde(default)]
    dim_foreground: Option<String>,
    #[serde(default)]
    bright_foreground: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyCursorColors {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittySelectionColors {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    background: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyAnsiColors {
    #[serde(default)]
    black: Option<String>,
    #[serde(default)]
    red: Option<String>,
    #[serde(default)]
    green: Option<String>,
    #[serde(default)]
    yellow: Option<String>,
    #[serde(default)]
    blue: Option<String>,
    #[serde(default)]
    magenta: Option<String>,
    #[serde(default)]
    cyan: Option<String>,
    #[serde(default)]
    white: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyIndexedColor {
    #[serde(default)]
    index: u8,
    #[serde(default)]
    color: String,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyCursor {
    #[serde(default)]
    style: Option<AlacrittyCursorStyle>,
    #[serde(default)]
    blinking: Option<String>,
    #[serde(default)]
    unfocused_hollow: Option<bool>,
    #[serde(default)]
    thickness: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyCursorStyle {
    #[serde(default)]
    shape: Option<String>,
    #[serde(default)]
    blinking: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyScrolling {
    #[serde(default)]
    history: Option<u32>,
    #[serde(default)]
    multiplier: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyWindow {
    #[serde(default)]
    opacity: Option<f64>,
    #[serde(default)]
    dimensions: Option<AlacrittyDimensions>,
    #[serde(default)]
    padding: Option<AlacrittyPadding>,
    #[serde(default)]
    decorations: Option<String>,
    #[serde(default)]
    dynamic_title: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyDimensions {
    #[serde(default)]
    columns: Option<u32>,
    #[serde(default)]
    lines: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyPadding {
    #[serde(default)]
    x: Option<u32>,
    #[serde(default)]
    y: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittySelection {
    #[serde(default)]
    semantic_escape_chars: Option<String>,
    #[serde(default)]
    save_to_clipboard: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyBell {
    #[serde(default)]
    animation: Option<String>,
    #[serde(default)]
    duration: Option<u32>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    command: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AlacrittyShell {
    #[serde(default)]
    program: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
}

/// Import settings from Alacritty.
///
/// Parses Alacritty's TOML config file from standard locations.
pub fn import_from_alacritty() -> Result<ImportResult, String> {
    let config_path = find_alacritty_config()?;

    let content =
        fs::read_to_string(&config_path).map_err(|e| format!("Failed to read Alacritty config: {}", e))?;

    let alacritty: AlacrittyConfig =
        toml::from_str(&content).map_err(|e| format!("Failed to parse Alacritty TOML: {}", e))?;

    let mut config = AppConfig::default();
    let mut imported_count = 0;
    let mut warnings = Vec::new();

    // Font settings
    if let Some(ref family) = alacritty.font.family {
        config.terminal.font_family = family.clone();
        imported_count += 1;
    } else if let Some(ref bold) = alacritty.font.bold {
        if let Some(ref family) = bold.family {
            config.terminal.font_family = family.clone();
            imported_count += 1;
        }
    }

    if let Some(size) = alacritty.font.size {
        config.terminal.font_size = size as u16;
        config.terminal.font_size_mac = size as u16;
        imported_count += 1;
    }

    // Cursor settings
    let cursor_style_val = alacritty
        .cursor
        .style
        .as_ref()
        .and_then(|s| s.shape.as_ref());

    if let Some(shape) = cursor_style_val {
        config.terminal.cursor_style = match shape.as_str() {
            "Block" => "block".to_string(),
            "Underline" => "underline".to_string(),
            "Beam" => "beam".to_string(),
            _ => {
                warnings.push(format!("Unknown Alacritty cursor shape: {}", shape));
                "block".to_string()
            }
        };
        imported_count += 1;
    }

    // Cursor blinking
    let blinking = alacritty
        .cursor
        .blinking
        .as_deref()
        .or_else(|| alacritty.cursor.style.as_ref().and_then(|s| s.blinking.as_deref()));

    if let Some(blink) = blinking {
        config.terminal.cursor_blink = blink != "Never" && blink != "Off";
        imported_count += 1;
    }

    // Scrollback
    if let Some(history) = alacritty.scrolling.history {
        config.terminal.scrollback_lines = history;
        imported_count += 1;
    }

    // Window opacity / background opacity
    let opacity = alacritty
        .window
        .opacity
        .or(alacritty.background_opacity);
    if let Some(op) = opacity {
        if op < 1.0 {
            config.appearance.opacity = op as f32;
            imported_count += 1;
        }
    }

    // Bell
    if alacritty.bell.duration.is_some() {
        config.terminal.bell = "visual".to_string();
        imported_count += 1;
    }

    // Selection
    if let Some(true) = alacritty.selection.save_to_clipboard {
        config.terminal.copy_on_select = true;
        imported_count += 1;
    }

    // Colors
    let mut has_colors = false;

    let background = alacritty
        .colors
        .primary
        .as_ref()
        .and_then(|p| p.background.clone())
        .unwrap_or_else(|| "#000000".to_string());

    let foreground = alacritty
        .colors
        .primary
        .as_ref()
        .and_then(|p| p.foreground.clone())
        .unwrap_or_else(|| "#ffffff".to_string());

    let cursor = alacritty
        .colors
        .cursor
        .as_ref()
        .and_then(|c| c.cursor.clone())
        .unwrap_or_else(|| "#ffffff".to_string());

    let selection_bg = alacritty
        .colors
        .selection
        .as_ref()
        .and_then(|s| s.background.clone())
        .unwrap_or_else(|| "#4d4d4d".to_string());

    let selection_fg = alacritty
        .colors
        .selection
        .as_ref()
        .and_then(|s| s.text.clone())
        .unwrap_or_else(|| foreground.clone());

    // Mark that we have colors if primary colors exist
    if alacritty.colors.primary.is_some() {
        has_colors = true;
    }

    // Build ANSI palette from Alacritty normal + bright colors
    let default_colors: [&str; 16] = [
        "#000000", "#cc0000", "#00cc00", "#cccc00", "#0000cc", "#cc00cc", "#00cccc", "#cccccc",
        "#555555", "#ff5555", "#55ff55", "#ffff55", "#5555ff", "#ff55ff", "#55ffff", "#ffffff",
    ];

    let mut colors: [String; 16] = std::array::from_fn(|i| default_colors[i].to_string());

    if let Some(ref normal) = alacritty.colors.normal {
        let pairs: [(usize, Option<&String>); 8] = [
            (0, normal.black.as_ref()),
            (1, normal.red.as_ref()),
            (2, normal.green.as_ref()),
            (3, normal.yellow.as_ref()),
            (4, normal.blue.as_ref()),
            (5, normal.magenta.as_ref()),
            (6, normal.cyan.as_ref()),
            (7, normal.white.as_ref()),
        ];
        for (idx, color_opt) in &pairs {
            if let Some(c) = color_opt {
                colors[*idx] = c.to_string();
                has_colors = true;
            }
        }
    }

    if let Some(ref bright) = alacritty.colors.bright {
        let pairs: [(usize, Option<&String>); 8] = [
            (8, bright.black.as_ref()),
            (9, bright.red.as_ref()),
            (10, bright.green.as_ref()),
            (11, bright.yellow.as_ref()),
            (12, bright.blue.as_ref()),
            (13, bright.magenta.as_ref()),
            (14, bright.cyan.as_ref()),
            (15, bright.white.as_ref()),
        ];
        for (idx, color_opt) in &pairs {
            if let Some(c) = color_opt {
                colors[*idx] = c.to_string();
                has_colors = true;
            }
        }
    }

    if has_colors {
        let color_scheme = ColorScheme {
            name: "Alacritty Imported".to_string(),
            foreground,
            background,
            cursor,
            selection_background: selection_bg,
            selection_foreground: selection_fg,
            colors,
        };
        config.terminal.color_scheme = Some(color_scheme);
        imported_count += 1;
    }

    Ok(ImportResult {
        config,
        imported_count,
        warnings,
    })
}

/// Find the Alacritty config file in standard locations.
fn find_alacritty_config() -> Result<PathBuf, String> {
    let candidates = get_alacritty_config_paths();

    for path in &candidates {
        if path.exists() {
            return Ok(path.clone());
        }
    }

    Err(format!(
        "Alacritty config not found. Searched: {:?}",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
    ))
}

/// Get candidate paths for Alacritty config.
fn get_alacritty_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // XDG config dir (~/.config/alacritty/)
    if let Some(config_dir) = dirs::config_dir() {
        paths.push(config_dir.join("alacritty").join("alacritty.toml"));
        paths.push(config_dir.join("alacritty").join("alacritty.yml"));
    }

    // Home directory
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".alacritty.toml"));
        paths.push(home.join(".alacritty.yml"));
    }

    // macOS specific
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            paths.push(
                home.join(".config")
                    .join("alacritty")
                    .join("alacritty.toml"),
            );
        }
    }

    paths
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_font_name() {
        assert_eq!(clean_font_name("Menlo-Regular"), "Menlo");
        assert_eq!(clean_font_name("Menlo-Bold"), "Menlo");
        assert_eq!(clean_font_name("JetBrains Mono"), "JetBrains Mono");
        assert_eq!(clean_font_name("SF Mono-Light"), "SF Mono");
    }

    #[test]
    fn test_alacritty_config_paths() {
        let paths = get_alacritty_config_paths();
        assert!(!paths.is_empty());
        // Verify all paths end with alacritty.toml or alacritty.yml
        for path in &paths {
            let name = path.file_name().unwrap().to_string_lossy();
            assert!(
                name.ends_with("alacritry.toml")
                    || name.ends_with("alacritty.toml")
                    || name.ends_with("alacritty.yml"),
                "Expected alacritty config file, got: {}",
                name
            );
        }
    }

    #[test]
    fn test_import_alacritty_toml() {
        let toml_content = r####"
[font]
family = "JetBrains Mono"
size = 14.0

[cursor]
blinking = "Always"

[cursor.style]
shape = "Block"
blinking = "On"

[scrolling]
history = 10000

[colors.primary]
background = "#1d1f21"
foreground = "#c5c8c6"

[colors.normal]
black = "#1d1f21"
red = "#cc6666"
green = "#b5bd68"
yellow = "#f0c674"
blue = "#81a2be"
magenta = "#b294bb"
cyan = "#8abeb7"
white = "#c5c8c6"

[colors.bright]
black = "#969896"
red = "#cc6666"
green = "#b5bd68"
yellow = "#f0c674"
blue = "#81a2be"
magenta = "#b294bb"
cyan = "#8abeb7"
white = "#ffffff"

[window]
opacity = 0.95

[selection]
save_to_clipboard = true
"####;

        let alacritty: AlacrittyConfig = toml::from_str(toml_content).unwrap();

        assert_eq!(alacritty.font.family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(alacritty.font.size, Some(14.0));
        assert_eq!(alacritty.scrolling.history, Some(10000));
        assert_eq!(alacritty.window.opacity, Some(0.95));

        let cursor_style = alacritty.cursor.style.as_ref().unwrap();
        assert_eq!(cursor_style.shape.as_deref(), Some("Block"));
    }

    #[test]
    fn test_iterm2_color_conversion() {
        let color = Iterm2Color {
            red_component: Some(1.0),
            green_component: Some(0.5),
            blue_component: Some(0.0),
            alpha_component: Some(1.0),
        };
        let hex = color.to_hex().unwrap();
        // 0.5 * 255.0 = 127.5, truncated to 127 = 0x7f
        assert_eq!(hex, "#ff7f00");
    }
}
