//! Configuration options types and defaults
//!
//! Contains all 100+ configuration options with their types and default values.

use serde::{Deserialize, Serialize};

// ── Color Type ────────────────────────────────────────────────────────────────

/// RGBA Color representation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub fn with_alpha(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim_start_matches('#');
        match hex.len() {
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Self::new(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some(Self::with_alpha(r, g, b, a))
            }
            _ => None,
        }
    }

    pub fn to_hex(&self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    pub fn rgb(&self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }

    pub fn rgba(&self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::new(255, 255, 255)
    }
}

// ── Repeatable String ─────────────────────────────────────────────────────────

/// A string that can have multiple fallback values
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct RepeatableString {
    pub values: Vec<String>,
}

impl RepeatableString {
    pub fn new(values: Vec<String>) -> Self {
        Self { values }
    }

    pub fn single(value: impl Into<String>) -> Self {
        Self {
            values: vec![value.into()],
        }
    }

    pub fn primary(&self) -> Option<&str> {
        self.values.first().map(|s| s.as_str())
    }
}

// ── Font Style ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FontStyle {
    pub italic: bool,
    pub oblique: bool,
}

impl FontStyle {
    pub fn italic() -> Self {
        Self {
            italic: true,
            oblique: false,
        }
    }
}

// ── Synthetic Style ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SyntheticStyle {
    pub bold: bool,
    pub italic: bool,
    pub bold_italic: bool,
}

// ── Font Feature ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontFeature {
    pub name: String,
    pub value: i32,
}

impl FontFeature {
    pub fn new(name: impl Into<String>, value: i32) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
}

// ── Font Variation ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontVariation {
    pub tag: String,
    pub value: f32,
}

impl FontVariation {
    pub fn new(tag: impl Into<String>, value: f32) -> Self {
        Self {
            tag: tag.into(),
            value,
        }
    }
}

// ── Palette Generate ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum PaletteGenerate {
    #[default]
    None,
    All,
    Light,
    Dark,
}

// ── Bold Color ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(untagged)]
pub enum BoldColor {
    #[default]
    Default,
    Normal,
    Color(Color),
}

// ── Grapheme Width Method ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum GraphemeWidthMethod {
    #[default]
    Css,
    Unicode,
    Wcwidth,
}

// ── Window Padding ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WindowPadding {
    Pixels(u32),
    Cells(f32),
    Auto,
}

impl Default for WindowPadding {
    fn default() -> Self {
        Self::Cells(0.0)
    }
}

// ── Window Decorations ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum WindowDecorations {
    #[default]
    Full,
    None,
    Transparent,
}

// ── Size ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
pub struct Size {
    pub value: f32,
    pub unit: SizeUnit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum SizeUnit {
    #[default]
    Cells,
    Pixels,
    Percent,
}

// ── Mouse Reporting ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum MouseReporting {
    #[default]
    None,
    Click,
    ClickDrag,
    Motion,
}

// ── Key Binding ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyBinding {
    pub key: String,
    pub action: KeyAction,
    #[serde(default)]
    pub mods: KeyModifiers,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct KeyModifiers {
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub super_: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum KeyAction {
    /// Send the key to the terminal
    Send(String),
    /// Run a command
    Run(String),
    /// Paste from clipboard
    Paste,
    /// Copy selection to clipboard
    Copy,
    /// Scroll to bottom
    ScrollToBottom,
    /// Scroll one page up
    PageUp,
    /// Scroll one page down
    PageDown,
    /// Increase font size
    FontSizeUp,
    /// Decrease font size
    FontSizeDown,
    /// Reset font size
    FontSizeReset,
    /// Toggle fullscreen
    ToggleFullscreen,
    /// Toggle maximized
    ToggleMaximize,
    /// Hide window
    Hide,
    /// Quit application
    Quit,
    /// New tab
    NewTab,
    /// Close tab
    CloseTab,
    /// Next tab
    NextTab,
    /// Previous tab
    PrevTab,
}

// ── Key Remap ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyRemap {
    pub from: String,
    pub to: String,
}

// ── Right Click Action ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum RightClickAction {
    #[default]
    Paste,
    CopyPaste,
    Copy,
    Nothing,
}

// ── Command ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Command {
    Shell(Vec<String>),
    Direct(Vec<String>),
}

// ── Scrollbar ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum Scrollbar {
    #[default]
    Hidden,
    Visible,
    Overlay,
}

// ── Link Rule ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LinkRule {
    pub pattern: String,
    #[serde(default)]
    pub mods: KeyModifiers,
}

// ── Shell Integration ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum ShellIntegration {
    #[default]
    Install,
    Uninstall,
    None,
}

// ── Progress Style ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum ProgressStyle {
    #[default]
    Bar,
    Diamond,
    Pulse,
    Static,
}

// ── Bell Features ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct BellFeatures {
    #[serde(default = "default_true")]
    pub visual: bool,
    #[serde(default = "default_true")]
    pub audio: bool,
    #[serde(default)]
    pub notification: bool,
}

// ── Command Palette Entry ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommandPaletteEntry {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub keybind: Option<String>,
}

// ── Resize Overlay ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum ResizeOverlay {
    #[default]
    Overlay,
    Float,
    None,
}

// ── Default Value Functions ──────────────────────────────────────────────────

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn default_font_family() -> RepeatableString {
    RepeatableString::single("JetBrains Mono")
}

pub(crate) fn default_font_family_bold() -> RepeatableString {
    RepeatableString::default()
}

pub(crate) fn default_font_family_italic() -> RepeatableString {
    RepeatableString::default()
}

pub(crate) fn default_font_family_bold_italic() -> RepeatableString {
    RepeatableString::default()
}

pub(crate) fn default_font_size() -> f32 {
    if cfg!(target_os = "macos") { 13.0 } else { 12.0 }
}

pub(crate) fn default_font_synthetic_style() -> SyntheticStyle {
    SyntheticStyle {
        bold: true,
        italic: true,
        bold_italic: true,
    }
}

pub(crate) fn default_font_thicken_strength() -> f32 {
    0.5
}

pub(crate) fn default_theme() -> String {
    "dark".into()
}

pub(crate) fn default_cursor_opacity() -> f64 {
    1.0
}

pub(crate) fn default_cursor_style() -> CursorStyle {
    CursorStyle::Block
}

pub(crate) fn default_cursor_style_blink() -> bool {
    true
}

pub(crate) fn default_selection_clear_on_copy() -> bool {
    true
}

pub(crate) fn default_minimum_contrast() -> f64 {
    0.0
}

pub(crate) fn default_bold_color() -> BoldColor {
    BoldColor::Default
}

pub(crate) fn default_faint_opacity() -> f64 {
    0.5
}

pub(crate) fn default_background_image_opacity() -> f64 {
    1.0
}

pub(crate) fn default_background_image_position() -> String {
    "center".into()
}

pub(crate) fn default_background_image_fit() -> String {
    "cover".into()
}

pub(crate) fn default_background_image_repeat() -> bool {
    false
}

pub(crate) fn default_alpha_blending() -> f64 {
    1.0
}

pub(crate) fn default_adjust_cell_width() -> f32 {
    0.0
}

pub(crate) fn default_adjust_cell_height() -> f32 {
    0.0
}

pub(crate) fn default_adjust_font_baseline() -> i32 {
    0
}

pub(crate) fn default_adjust_underline_position() -> i32 {
    0
}

pub(crate) fn default_adjust_underline_thickness() -> u32 {
    0
}

pub(crate) fn default_adjust_strikethrough_position() -> i32 {
    0
}

pub(crate) fn default_adjust_strikethrough_thickness() -> u32 {
    0
}

pub(crate) fn default_adjust_overline_position() -> i32 {
    0
}

pub(crate) fn default_adjust_overline_thickness() -> u32 {
    0
}

pub(crate) fn default_adjust_cursor_thickness() -> u32 {
    0
}

pub(crate) fn default_adjust_cursor_height() -> u32 {
    0
}

pub(crate) fn default_adjust_box_thickness() -> u32 {
    0
}

pub(crate) fn default_adjust_icon_height() -> u32 {
    0
}

pub(crate) fn default_grapheme_width_method() -> GraphemeWidthMethod {
    GraphemeWidthMethod::Css
}

pub(crate) fn default_freetype_load_flags() -> String {
    String::new()
}

pub(crate) fn default_window_padding_x() -> WindowPadding {
    WindowPadding::Cells(0.0)
}

pub(crate) fn default_window_padding_y() -> WindowPadding {
    WindowPadding::Cells(0.0)
}

pub(crate) fn default_window_padding_balance() -> bool {
    false
}

pub(crate) fn default_window_vsync() -> bool {
    true
}

pub(crate) fn default_window_decorations() -> WindowDecorations {
    WindowDecorations::Full
}

pub(crate) fn default_window_height() -> Size {
    Size {
        value: 24.0,
        unit: SizeUnit::Cells,
    }
}

pub(crate) fn default_window_width() -> Size {
    Size {
        value: 80.0,
        unit: SizeUnit::Cells,
    }
}

pub(crate) fn default_window_save_state() -> bool {
    true
}

pub(crate) fn default_window_step_resize() -> bool {
    false
}

pub(crate) fn default_window_new_tab_position() -> String {
    "after".into()
}

pub(crate) fn default_window_show_tab_bar() -> bool {
    true
}

pub(crate) fn default_window_inherit_working_directory() -> bool {
    true
}

pub(crate) fn default_window_inherit_font_size() -> bool {
    false
}

pub(crate) fn default_maximize() -> bool {
    false
}

pub(crate) fn default_fullscreen() -> bool {
    false
}

pub(crate) fn default_title() -> String {
    "PureTerm".into()
}

pub(crate) fn default_class() -> String {
    "pureterm".into()
}

pub(crate) fn default_background_opacity() -> f64 {
    1.0
}

pub(crate) fn default_background_opacity_cells() -> f64 {
    1.0
}

pub(crate) fn default_background_blur() -> f64 {
    0.0
}

pub(crate) fn default_unfocused_split_opacity() -> f64 {
    0.5
}

pub(crate) fn default_unfocused_split_fill() -> f64 {
    0.0
}

pub(crate) fn default_split_preserve_zoom() -> bool {
    true
}

pub(crate) fn default_mouse_hide_while_typing() -> bool {
    true
}

pub(crate) fn default_mouse_shift_capture() -> bool {
    false
}

pub(crate) fn default_mouse_reporting() -> MouseReporting {
    MouseReporting::Click
}

pub(crate) fn default_mouse_scroll_multiplier() -> f64 {
    1.0
}

pub(crate) fn default_clipboard_read() -> bool {
    true
}

pub(crate) fn default_clipboard_write() -> bool {
    true
}

pub(crate) fn default_clipboard_trim_trailing_spaces() -> bool {
    true
}

pub(crate) fn default_clipboard_paste_bracketed_safe() -> bool {
    true
}

pub(crate) fn default_scroll_to_bottom() -> bool {
    true
}

pub(crate) fn default_click_repeat_interval() -> u32 {
    500
}

pub(crate) fn default_right_click_action() -> RightClickAction {
    RightClickAction::Paste
}

pub(crate) fn default_vt_kam_allowed() -> bool {
    false
}

pub(crate) fn default_wait_after_command() -> u32 {
    0
}

pub(crate) fn default_abnormal_command_exit_runtime() -> bool {
    false
}

pub(crate) fn default_notify_on_command_finish() -> bool {
    false
}

pub(crate) fn default_scrollback_limit() -> u32 {
    25000
}

pub(crate) fn default_scrollbar() -> Scrollbar {
    Scrollbar::Hidden
}

pub(crate) fn default_link_previews() -> bool {
    true
}

pub(crate) fn default_shell_integration() -> ShellIntegration {
    ShellIntegration::Install
}

pub(crate) fn default_quick_terminal_position() -> String {
    "top".into()
}

pub(crate) fn default_quick_terminal_size() -> Size {
    Size {
        value: 50.0,
        unit: SizeUnit::Percent,
    }
}

pub(crate) fn default_quick_terminal_animation_duration() -> u32 {
    200
}

pub(crate) fn default_quick_terminal_autohide() -> bool {
    true
}

pub(crate) fn default_quick_terminal_space_behavior() -> String {
    "invisible".into()
}

pub(crate) fn default_quick_terminal_screen() -> String {
    "primary".into()
}

pub(crate) fn default_desktop_notifications() -> bool {
    true
}

pub(crate) fn default_app_notifications() -> bool {
    true
}

pub(crate) fn default_bell_features() -> BellFeatures {
    BellFeatures {
        visual: true,
        audio: true,
        notification: false,
    }
}

pub(crate) fn default_bell_audio_volume() -> f64 {
    1.0
}

pub(crate) fn default_osc_color_report_format() -> String {
    "hex".into()
}

pub(crate) fn default_confirm_close_surface() -> bool {
    false
}

pub(crate) fn default_quit_after_last_window_closed() -> bool {
    false
}

pub(crate) fn default_undo_timeout() -> u32 {
    5000
}

pub(crate) fn default_focus_follows_mouse() -> bool {
    false
}

pub(crate) fn default_copy_on_select() -> bool {
    true
}

pub(crate) fn default_title_report() -> bool {
    false
}

pub(crate) fn default_image_storage_limit() -> u32 {
    10 * 1024 * 1024
}

pub(crate) fn default_term() -> String {
    "xterm-256color".into()
}

pub(crate) fn default_async_backend() -> bool {
    true
}

pub(crate) fn default_resize_duration() -> u32 {
    100
}

pub(crate) fn default_language() -> String {
    "en".into()
}

pub(crate) fn default_macos_non_native_fullscreen() -> bool {
    false
}

pub(crate) fn default_macos_dock_drop_behavior() -> String {
    "none".into()
}

pub(crate) fn default_macos_option_as_alt() -> bool {
    false
}

pub(crate) fn default_macos_window_shadow() -> bool {
    true
}

pub(crate) fn default_macos_hidden() -> bool {
    false
}

pub(crate) fn default_gtk_opengl_debug() -> bool {
    false
}

pub(crate) fn default_gtk_single_instance() -> bool {
    false
}

pub(crate) fn default_gtk_titlebar_hide_when_maximized() -> bool {
    false
}

pub(crate) fn default_gtk_wide_tabs() -> bool {
    false
}

pub(crate) fn default_linux_memory_limit() -> u32 {
    0
}

pub(crate) fn default_linux_processes_limit() -> u32 {
    0
}

pub(crate) fn default_linux_hard_fail() -> bool {
    false
}

pub(crate) fn default_auto_update() -> bool {
    true
}

pub(crate) fn default_auto_update_channel() -> String {
    "stable".into()
}

// ── Cursor Style ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum CursorStyle {
    #[default]
    Block,
    /// Hollow/outline block cursor, typically shown when unfocused.
    BlockHollow,
    Underline,
    Beam,
    Bar,
}
