//! PureTerm Configuration System
//!
//! Implements Ghostty-style config with 100+ options, CLI parsing,
//! conditional config, themes, and validation.

mod cli;
mod conditional;
pub mod import;
mod legacy;
mod loader;
mod options;
mod service;
mod types;
mod validator;

// Re-exports from options (shared types: Color, Size, enums, default functions, etc.)
pub use options::*;

// Re-exports from CLI
pub use cli::CliArgs;

// Re-exports from conditional
pub use conditional::ConditionalConfig;

// Re-exports from loader
pub use loader::{ConfigLoader, ConfigError};

// Re-exports from validator
pub use validator::ConfigValidator;

// Re-exports from legacy types
pub use legacy::AppConfig;
pub use legacy::AppearanceConfig;
pub use legacy::TerminalConfig;

// Re-exports from shared types
pub use types::GlassEffectStyle;

// Re-export service
pub use service::ConfigService;

// ── New Config struct (100+ options) ──────────────────────────────────────────

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Main application configuration with 100+ options
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    // ── Font Configuration ─────────────────────────────────────────────────
    #[serde(default = "default_font_family")]
    pub font_family: RepeatableString,
    #[serde(default = "default_font_family_bold")]
    pub font_family_bold: RepeatableString,
    #[serde(default = "default_font_family_italic")]
    pub font_family_italic: RepeatableString,
    #[serde(default = "default_font_family_bold_italic")]
    pub font_family_bold_italic: RepeatableString,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_font_size")]
    pub font_size_mac: f32,
    #[serde(default)]
    pub font_style: FontStyle,
    #[serde(default)]
    pub font_style_bold: FontStyle,
    #[serde(default)]
    pub font_style_italic: FontStyle,
    #[serde(default)]
    pub font_style_bold_italic: FontStyle,
    #[serde(default)]
    pub font_feature: Vec<FontFeature>,
    #[serde(default)]
    pub font_variation: Vec<FontVariation>,
    #[serde(default)]
    pub font_variation_bold: Vec<FontVariation>,
    #[serde(default)]
    pub font_variation_italic: Vec<FontVariation>,
    #[serde(default)]
    pub font_variation_bold_italic: Vec<FontVariation>,
    #[serde(default = "default_font_synthetic_style")]
    pub font_synthetic_style: SyntheticStyle,
    #[serde(default)]
    pub font_codepoint_map: HashMap<String, String>,
    #[serde(default)]
    pub font_shaping_break: Vec<String>,
    #[serde(default)]
    pub font_thicken: bool,
    #[serde(default = "default_font_thicken_strength")]
    pub font_thicken_strength: f32,

    // ── Colors/Theme ───────────────────────────────────────────────────────
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub background: Color,
    #[serde(default)]
    pub foreground: Color,
    #[serde(default)]
    pub cursor_color: Color,
    #[serde(default = "default_cursor_opacity")]
    pub cursor_opacity: f64,
    #[serde(default = "default_cursor_style")]
    pub cursor_style: CursorStyle,
    #[serde(default = "default_cursor_style_blink")]
    pub cursor_style_blink: bool,
    #[serde(default)]
    pub cursor_text: Color,
    #[serde(default)]
    pub palette: HashMap<String, Color>,
    #[serde(default)]
    pub palette_generate: PaletteGenerate,
    #[serde(default)]
    pub palette_harmonious: f64,
    #[serde(default)]
    pub selection_foreground: Color,
    #[serde(default)]
    pub selection_background: Color,
    #[serde(default = "default_selection_clear_on_copy")]
    pub selection_clear_on_copy: bool,
    #[serde(default)]
    pub selection_word_chars: String,
    #[serde(default = "default_minimum_contrast")]
    pub minimum_contrast: f64,
    #[serde(default = "default_bold_color")]
    pub bold_color: BoldColor,
    #[serde(default = "default_faint_opacity")]
    pub faint_opacity: f64,
    #[serde(default)]
    pub search_foreground: Color,
    #[serde(default)]
    pub search_background: Color,
    #[serde(default)]
    pub search_selected_foreground: Color,
    #[serde(default)]
    pub search_selected_background: Color,
    #[serde(default)]
    pub background_image: Option<String>,
    #[serde(default = "default_background_image_opacity")]
    pub background_image_opacity: f64,
    #[serde(default = "default_background_image_position")]
    pub background_image_position: String,
    #[serde(default = "default_background_image_fit")]
    pub background_image_fit: String,
    #[serde(default = "default_background_image_repeat")]
    pub background_image_repeat: bool,
    #[serde(default = "default_alpha_blending")]
    pub alpha_blending: f64,

    // ── Rendering/Metrics ──────────────────────────────────────────────────
    #[serde(default = "default_adjust_cell_width")]
    pub adjust_cell_width: f32,
    #[serde(default = "default_adjust_cell_height")]
    pub adjust_cell_height: f32,
    #[serde(default = "default_adjust_font_baseline")]
    pub adjust_font_baseline: i32,
    #[serde(default = "default_adjust_underline_position")]
    pub adjust_underline_position: i32,
    #[serde(default = "default_adjust_underline_thickness")]
    pub adjust_underline_thickness: u32,
    #[serde(default = "default_adjust_strikethrough_position")]
    pub adjust_strikethrough_position: i32,
    #[serde(default = "default_adjust_strikethrough_thickness")]
    pub adjust_strikethrough_thickness: u32,
    #[serde(default = "default_adjust_overline_position")]
    pub adjust_overline_position: i32,
    #[serde(default = "default_adjust_overline_thickness")]
    pub adjust_overline_thickness: u32,
    #[serde(default = "default_adjust_cursor_thickness")]
    pub adjust_cursor_thickness: u32,
    #[serde(default = "default_adjust_cursor_height")]
    pub adjust_cursor_height: u32,
    #[serde(default = "default_adjust_box_thickness")]
    pub adjust_box_thickness: u32,
    #[serde(default = "default_adjust_icon_height")]
    pub adjust_icon_height: u32,
    #[serde(default = "default_grapheme_width_method")]
    pub grapheme_width_method: GraphemeWidthMethod,
    #[serde(default = "default_freetype_load_flags")]
    pub freetype_load_flags: String,

    // ── Window/Layout ─────────────────────────────────────────────────────
    #[serde(default = "default_window_padding_x")]
    pub window_padding_x: WindowPadding,
    #[serde(default = "default_window_padding_y")]
    pub window_padding_y: WindowPadding,
    #[serde(default = "default_window_padding_balance")]
    pub window_padding_balance: bool,
    #[serde(default)]
    pub window_padding_color: Color,
    #[serde(default = "default_window_vsync")]
    pub window_vsync: bool,
    #[serde(default = "default_window_decorations")]
    pub window_decorations: WindowDecorations,
    #[serde(default)]
    pub window_title_font_family: Option<String>,
    #[serde(default)]
    pub window_subtitle: Option<String>,
    #[serde(default)]
    pub window_theme: Option<String>,
    #[serde(default)]
    pub window_colorspace: String,
    #[serde(default = "default_window_height")]
    pub window_height: Size,
    #[serde(default = "default_window_width")]
    pub window_width: Size,
    #[serde(default)]
    pub window_position_x: Option<i32>,
    #[serde(default)]
    pub window_position_y: Option<i32>,
    #[serde(default = "default_window_save_state")]
    pub window_save_state: bool,
    #[serde(default = "default_window_step_resize")]
    pub window_step_resize: bool,
    #[serde(default = "default_window_new_tab_position")]
    pub window_new_tab_position: String,
    #[serde(default = "default_window_show_tab_bar")]
    pub window_show_tab_bar: bool,
    #[serde(default)]
    pub window_titlebar_background: Color,
    #[serde(default)]
    pub window_titlebar_foreground: Color,
    #[serde(default = "default_window_inherit_working_directory")]
    pub window_inherit_working_directory: bool,
    #[serde(default = "default_window_inherit_font_size")]
    pub window_inherit_font_size: bool,
    #[serde(default = "default_maximize")]
    pub maximize: bool,
    #[serde(default = "default_fullscreen")]
    pub fullscreen: bool,
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default = "default_class")]
    pub class: String,

    // ── Background/Opacity ─────────────────────────────────────────────────
    #[serde(default = "default_background_opacity")]
    pub background_opacity: f64,
    #[serde(default = "default_background_opacity_cells")]
    pub background_opacity_cells: f64,
    #[serde(default = "default_background_blur")]
    pub background_blur: f64,
    #[serde(default = "default_unfocused_split_opacity")]
    pub unfocused_split_opacity: f64,
    #[serde(default = "default_unfocused_split_fill")]
    pub unfocused_split_fill: f64,
    #[serde(default)]
    pub split_divider_color: Color,
    #[serde(default = "default_split_preserve_zoom")]
    pub split_preserve_zoom: bool,

    // ── Mouse/Keyboard/Input ───────────────────────────────────────────────
    #[serde(default = "default_mouse_hide_while_typing")]
    pub mouse_hide_while_typing: bool,
    #[serde(default = "default_mouse_shift_capture")]
    pub mouse_shift_capture: bool,
    #[serde(default = "default_mouse_reporting")]
    pub mouse_reporting: MouseReporting,
    #[serde(default = "default_mouse_scroll_multiplier")]
    pub mouse_scroll_multiplier: f64,
    #[serde(default = "default_clipboard_read")]
    pub clipboard_read: bool,
    #[serde(default = "default_clipboard_write")]
    pub clipboard_write: bool,
    #[serde(default = "default_clipboard_trim_trailing_spaces")]
    pub clipboard_trim_trailing_spaces: bool,
    #[serde(default)]
    pub clipboard_paste_protection: bool,
    #[serde(default = "default_clipboard_paste_bracketed_safe")]
    pub clipboard_paste_bracketed_safe: bool,
    #[serde(default)]
    pub clipboard_codepoint_map: HashMap<String, String>,
    #[serde(default)]
    pub keybind: Vec<KeyBinding>,
    #[serde(default)]
    pub key_remap: Vec<KeyRemap>,
    #[serde(default = "default_scroll_to_bottom")]
    pub scroll_to_bottom: bool,
    #[serde(default = "default_click_repeat_interval")]
    pub click_repeat_interval: u32,
    #[serde(default = "default_right_click_action")]
    pub right_click_action: RightClickAction,
    #[serde(default = "default_vt_kam_allowed")]
    pub vt_kam_allowed: bool,

    // ── Command/Process ───────────────────────────────────────────────────
    #[serde(default)]
    pub command: Option<Command>,
    #[serde(default)]
    pub initial_command: Option<String>,
    #[serde(default = "default_wait_after_command")]
    pub wait_after_command: u32,
    #[serde(default = "default_abnormal_command_exit_runtime")]
    pub abnormal_command_exit_runtime: bool,
    #[serde(default = "default_notify_on_command_finish")]
    pub notify_on_command_finish: bool,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub input: Option<String>,

    // ── Scrollback ────────────────────────────────────────────────────────
    #[serde(default = "default_scrollback_limit")]
    pub scrollback_limit: u32,
    #[serde(default = "default_scrollbar")]
    pub scrollbar: Scrollbar,

    // ── Links ─────────────────────────────────────────────────────────────
    #[serde(default)]
    pub link: Vec<LinkRule>,
    #[serde(default)]
    pub link_url: Option<String>,
    #[serde(default = "default_link_previews")]
    pub link_previews: bool,

    // ── Shell Integration ─────────────────────────────────────────────────
    #[serde(default = "default_shell_integration")]
    pub shell_integration: ShellIntegration,
    #[serde(default)]
    pub shell_integration_features: Vec<String>,

    // ── Quick Terminal ────────────────────────────────────────────────────
    #[serde(default = "default_quick_terminal_position")]
    pub quick_terminal_position: String,
    #[serde(default = "default_quick_terminal_size")]
    pub quick_terminal_size: Size,
    #[serde(default = "default_quick_terminal_animation_duration")]
    pub quick_terminal_animation_duration: u32,
    #[serde(default = "default_quick_terminal_autohide")]
    pub quick_terminal_autohide: bool,
    #[serde(default = "default_quick_terminal_space_behavior")]
    pub quick_terminal_space_behavior: String,
    #[serde(default = "default_quick_terminal_screen")]
    pub quick_terminal_screen: String,

    // ── Desktop Notifications ──────────────────────────────────────────────
    #[serde(default = "default_desktop_notifications")]
    pub desktop_notifications: bool,
    #[serde(default)]
    pub progress_style: ProgressStyle,
    #[serde(default = "default_app_notifications")]
    pub app_notifications: bool,
    #[serde(default = "default_bell_features")]
    pub bell_features: BellFeatures,
    #[serde(default)]
    pub bell_audio_path: Option<String>,
    #[serde(default = "default_bell_audio_volume")]
    pub bell_audio_volume: f64,
    #[serde(default = "default_osc_color_report_format")]
    pub osc_color_report_format: String,

    // ── Other ─────────────────────────────────────────────────────────────
    #[serde(default)]
    pub config_file: Option<String>,
    #[serde(default)]
    pub config_default_files: Vec<String>,
    #[serde(default = "default_confirm_close_surface")]
    pub confirm_close_surface: bool,
    #[serde(default = "default_quit_after_last_window_closed")]
    pub quit_after_last_window_closed: bool,
    #[serde(default)]
    pub initial_window: Option<String>,
    #[serde(default = "default_undo_timeout")]
    pub undo_timeout: u32,
    #[serde(default = "default_focus_follows_mouse")]
    pub focus_follows_mouse: bool,
    #[serde(default = "default_copy_on_select")]
    pub copy_on_select: bool,
    #[serde(default = "default_title_report")]
    pub title_report: bool,
    #[serde(default = "default_image_storage_limit")]
    pub image_storage_limit: u32,
    #[serde(default = "default_term")]
    pub term: String,
    #[serde(default)]
    pub enquiry_response: Option<String>,
    #[serde(default = "default_async_backend")]
    pub async_backend: bool,
    #[serde(default)]
    pub command_palette_entry: Vec<CommandPaletteEntry>,
    #[serde(default)]
    pub resize_overlay: ResizeOverlay,
    #[serde(default)]
    pub resize_position: Option<String>,
    #[serde(default = "default_resize_duration")]
    pub resize_duration: u32,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default = "default_language")]
    pub language: String,

    // ── Platform Specific ─────────────────────────────────────────────────
    // macOS
    #[serde(default = "default_macos_non_native_fullscreen")]
    pub macos_non_native_fullscreen: bool,
    #[serde(default)]
    pub macos_window_buttons: Vec<String>,
    #[serde(default)]
    pub macos_titlebar_style: Option<String>,
    #[serde(default)]
    pub macos_proxy_icon: Option<String>,
    #[serde(default = "default_macos_dock_drop_behavior")]
    pub macos_dock_drop_behavior: String,
    #[serde(default = "default_macos_option_as_alt")]
    pub macos_option_as_alt: bool,
    #[serde(default = "default_macos_window_shadow")]
    pub macos_window_shadow: bool,
    #[serde(default = "default_macos_hidden")]
    pub macos_hidden: bool,
    #[serde(default)]
    pub macos_auto_secure_input: Option<bool>,
    #[serde(default)]
    pub macos_secure_input_indication: Option<bool>,
    #[serde(default)]
    pub macos_applescript: Option<String>,
    #[serde(default)]
    pub macos_icon: Option<String>,
    #[serde(default)]
    pub macos_custom_icon: Option<String>,
    #[serde(default)]
    pub macos_icon_frame: Option<String>,
    #[serde(default)]
    pub macos_icon_ghost_color: Option<Color>,
    #[serde(default)]
    pub macos_icon_screen_color: Option<Color>,
    #[serde(default)]
    pub macos_shortcuts: Vec<String>,

    // GTK/Linux
    #[serde(default = "default_gtk_opengl_debug")]
    pub gtk_opengl_debug: bool,
    #[serde(default = "default_gtk_single_instance")]
    pub gtk_single_instance: bool,
    #[serde(default)]
    pub gtk_titlebar: Option<String>,
    #[serde(default)]
    pub gtk_tabs_location: Option<String>,
    #[serde(default = "default_gtk_titlebar_hide_when_maximized")]
    pub gtk_titlebar_hide_when_maximized: bool,
    #[serde(default)]
    pub gtk_toolbar_style: Option<String>,
    #[serde(default)]
    pub gtk_titlebar_style: Option<String>,
    #[serde(default = "default_gtk_wide_tabs")]
    pub gtk_wide_tabs: bool,
    #[serde(default)]
    pub gtk_custom_css: Option<String>,
    #[serde(default)]
    pub linux_cgroup: Option<String>,
    #[serde(default = "default_linux_memory_limit")]
    pub linux_memory_limit: u32,
    #[serde(default = "default_linux_processes_limit")]
    pub linux_processes_limit: u32,
    #[serde(default = "default_linux_hard_fail")]
    pub linux_hard_fail: bool,
    #[serde(default)]
    pub x11_instance_name: Option<String>,

    // ── Auto Update ───────────────────────────────────────────────────────
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,
    #[serde(default = "default_auto_update_channel")]
    pub auto_update_channel: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // Font
            font_family: default_font_family(),
            font_family_bold: default_font_family_bold(),
            font_family_italic: default_font_family_italic(),
            font_family_bold_italic: default_font_family_bold_italic(),
            font_size: default_font_size(),
            font_size_mac: default_font_size(),
            font_style: FontStyle::default(),
            font_style_bold: FontStyle::default(),
            font_style_italic: FontStyle::default(),
            font_style_bold_italic: FontStyle::default(),
            font_feature: Vec::new(),
            font_variation: Vec::new(),
            font_variation_bold: Vec::new(),
            font_variation_italic: Vec::new(),
            font_variation_bold_italic: Vec::new(),
            font_synthetic_style: default_font_synthetic_style(),
            font_codepoint_map: HashMap::new(),
            font_shaping_break: Vec::new(),
            font_thicken: false,
            font_thicken_strength: default_font_thicken_strength(),

            // Colors
            theme: default_theme(),
            background: Color::default(),
            foreground: Color::default(),
            cursor_color: Color::default(),
            cursor_opacity: default_cursor_opacity(),
            cursor_style: default_cursor_style(),
            cursor_style_blink: default_cursor_style_blink(),
            cursor_text: Color::default(),
            palette: HashMap::new(),
            palette_generate: PaletteGenerate::default(),
            palette_harmonious: 0.0,
            selection_foreground: Color::default(),
            selection_background: Color::default(),
            selection_clear_on_copy: default_selection_clear_on_copy(),
            selection_word_chars: String::new(),
            minimum_contrast: default_minimum_contrast(),
            bold_color: default_bold_color(),
            faint_opacity: default_faint_opacity(),
            search_foreground: Color::default(),
            search_background: Color::default(),
            search_selected_foreground: Color::default(),
            search_selected_background: Color::default(),
            background_image: None,
            background_image_opacity: default_background_image_opacity(),
            background_image_position: default_background_image_position(),
            background_image_fit: default_background_image_fit(),
            background_image_repeat: default_background_image_repeat(),
            alpha_blending: default_alpha_blending(),

            // Rendering
            adjust_cell_width: default_adjust_cell_width(),
            adjust_cell_height: default_adjust_cell_height(),
            adjust_font_baseline: default_adjust_font_baseline(),
            adjust_underline_position: default_adjust_underline_position(),
            adjust_underline_thickness: default_adjust_underline_thickness(),
            adjust_strikethrough_position: default_adjust_strikethrough_position(),
            adjust_strikethrough_thickness: default_adjust_strikethrough_thickness(),
            adjust_overline_position: default_adjust_overline_position(),
            adjust_overline_thickness: default_adjust_overline_thickness(),
            adjust_cursor_thickness: default_adjust_cursor_thickness(),
            adjust_cursor_height: default_adjust_cursor_height(),
            adjust_box_thickness: default_adjust_box_thickness(),
            adjust_icon_height: default_adjust_icon_height(),
            grapheme_width_method: default_grapheme_width_method(),
            freetype_load_flags: default_freetype_load_flags(),

            // Window
            window_padding_x: default_window_padding_x(),
            window_padding_y: default_window_padding_y(),
            window_padding_balance: default_window_padding_balance(),
            window_padding_color: Color::default(),
            window_vsync: default_window_vsync(),
            window_decorations: default_window_decorations(),
            window_title_font_family: None,
            window_subtitle: None,
            window_theme: None,
            window_colorspace: String::new(),
            window_height: default_window_height(),
            window_width: default_window_width(),
            window_position_x: None,
            window_position_y: None,
            window_save_state: default_window_save_state(),
            window_step_resize: default_window_step_resize(),
            window_new_tab_position: default_window_new_tab_position(),
            window_show_tab_bar: default_window_show_tab_bar(),
            window_titlebar_background: Color::default(),
            window_titlebar_foreground: Color::default(),
            window_inherit_working_directory: default_window_inherit_working_directory(),
            window_inherit_font_size: default_window_inherit_font_size(),
            maximize: default_maximize(),
            fullscreen: default_fullscreen(),
            title: default_title(),
            class: default_class(),

            // Background
            background_opacity: default_background_opacity(),
            background_opacity_cells: default_background_opacity_cells(),
            background_blur: default_background_blur(),
            unfocused_split_opacity: default_unfocused_split_opacity(),
            unfocused_split_fill: default_unfocused_split_fill(),
            split_divider_color: Color::default(),
            split_preserve_zoom: default_split_preserve_zoom(),

            // Mouse/Keyboard
            mouse_hide_while_typing: default_mouse_hide_while_typing(),
            mouse_shift_capture: default_mouse_shift_capture(),
            mouse_reporting: default_mouse_reporting(),
            mouse_scroll_multiplier: default_mouse_scroll_multiplier(),
            clipboard_read: default_clipboard_read(),
            clipboard_write: default_clipboard_write(),
            clipboard_trim_trailing_spaces: default_clipboard_trim_trailing_spaces(),
            clipboard_paste_protection: false,
            clipboard_paste_bracketed_safe: default_clipboard_paste_bracketed_safe(),
            clipboard_codepoint_map: HashMap::new(),
            keybind: Vec::new(),
            key_remap: Vec::new(),
            scroll_to_bottom: default_scroll_to_bottom(),
            click_repeat_interval: default_click_repeat_interval(),
            right_click_action: default_right_click_action(),
            vt_kam_allowed: default_vt_kam_allowed(),

            // Command
            command: None,
            initial_command: None,
            wait_after_command: default_wait_after_command(),
            abnormal_command_exit_runtime: default_abnormal_command_exit_runtime(),
            notify_on_command_finish: default_notify_on_command_finish(),
            env: HashMap::new(),
            input: None,

            // Scrollback
            scrollback_limit: default_scrollback_limit(),
            scrollbar: default_scrollbar(),

            // Links
            link: Vec::new(),
            link_url: None,
            link_previews: default_link_previews(),

            // Shell
            shell_integration: default_shell_integration(),
            shell_integration_features: Vec::new(),

            // Quick Terminal
            quick_terminal_position: default_quick_terminal_position(),
            quick_terminal_size: default_quick_terminal_size(),
            quick_terminal_animation_duration: default_quick_terminal_animation_duration(),
            quick_terminal_autohide: default_quick_terminal_autohide(),
            quick_terminal_space_behavior: default_quick_terminal_space_behavior(),
            quick_terminal_screen: default_quick_terminal_screen(),

            // Notifications
            desktop_notifications: default_desktop_notifications(),
            progress_style: ProgressStyle::default(),
            app_notifications: default_app_notifications(),
            bell_features: default_bell_features(),
            bell_audio_path: None,
            bell_audio_volume: default_bell_audio_volume(),
            osc_color_report_format: default_osc_color_report_format(),

            // Other
            config_file: None,
            config_default_files: Vec::new(),
            confirm_close_surface: default_confirm_close_surface(),
            quit_after_last_window_closed: default_quit_after_last_window_closed(),
            initial_window: None,
            undo_timeout: default_undo_timeout(),
            focus_follows_mouse: default_focus_follows_mouse(),
            copy_on_select: default_copy_on_select(),
            title_report: default_title_report(),
            image_storage_limit: default_image_storage_limit(),
            term: default_term(),
            enquiry_response: None,
            async_backend: default_async_backend(),
            command_palette_entry: Vec::new(),
            resize_overlay: ResizeOverlay::default(),
            resize_position: None,
            resize_duration: default_resize_duration(),
            working_directory: None,
            language: default_language(),

            // macOS
            macos_non_native_fullscreen: default_macos_non_native_fullscreen(),
            macos_window_buttons: Vec::new(),
            macos_titlebar_style: None,
            macos_proxy_icon: None,
            macos_dock_drop_behavior: default_macos_dock_drop_behavior(),
            macos_option_as_alt: default_macos_option_as_alt(),
            macos_window_shadow: default_macos_window_shadow(),
            macos_hidden: default_macos_hidden(),
            macos_auto_secure_input: None,
            macos_secure_input_indication: None,
            macos_applescript: None,
            macos_icon: None,
            macos_custom_icon: None,
            macos_icon_frame: None,
            macos_icon_ghost_color: None,
            macos_icon_screen_color: None,
            macos_shortcuts: Vec::new(),

            // GTK/Linux
            gtk_opengl_debug: default_gtk_opengl_debug(),
            gtk_single_instance: default_gtk_single_instance(),
            gtk_titlebar: None,
            gtk_tabs_location: None,
            gtk_titlebar_hide_when_maximized: default_gtk_titlebar_hide_when_maximized(),
            gtk_toolbar_style: None,
            gtk_titlebar_style: None,
            gtk_wide_tabs: default_gtk_wide_tabs(),
            gtk_custom_css: None,
            linux_cgroup: None,
            linux_memory_limit: default_linux_memory_limit(),
            linux_processes_limit: default_linux_processes_limit(),
            linux_hard_fail: default_linux_hard_fail(),
            x11_instance_name: None,

            // Auto Update
            auto_update: default_auto_update(),
            auto_update_channel: default_auto_update_channel(),
        }
    }
}
