//! P4 GAP 运行时组件包——配置与状态均由 `iced_app` 装配。

/// 全部 P4 特性配置与运行时状态(GAP-05 … GAP-61、GAP-4.6)。
#[derive(Clone, Debug)]
pub struct P4Features {
    pub voice_input_cfg: crate::voice_input::VoiceInputConfig,
    pub voice_input: crate::voice_input::VoiceInputState,
    pub terminal_graphics_cfg: crate::terminal_graphics::TerminalGraphicsConfig,
    pub graphics_cache: crate::terminal_graphics::GraphicsCache,
    /// T-core-closure:活跃图像存储。渲染器从这里拉取已解码的 RGBA
    /// (或编码 PNG)载荷;分块图形重组器
    /// 负责喂入。取代此前已成死代码的 `GraphicsCache` 字段
    /// (为向后兼容保留——`GraphicsCache` 仍被
    /// 旧调用方使用;新代码应改用 `image_store`)。
    pub image_store: crate::terminal_graphics::ImageStore,
    /// T-core-closure:跨读取分块图形重组器。
    pub graphics_assembler: crate::terminal_graphics::GraphicsAssembler,
    pub glass_cfg: crate::macos_glass_effect::GlassEffectConfig,
    pub touch_bar_cfg: crate::macos_touch_bar::TouchBarConfig,
    pub tab_bar_cfg: crate::visual_tab_bar::VisualTabBarConfig,
    pub window_border_cfg: crate::window_border::WindowBorderConfig,
    pub badge_cfg: crate::badge_overlay::BadgeOverlayConfig,
    pub file_preview_cfg: crate::file_preview::FilePreviewConfig,
    pub file_transfer_cfg: crate::file_transfer::FileTransferConfig,
    pub file_transfer: crate::file_transfer::FileTransferState,
    pub shared_session_cfg: crate::shared_sessions::SharedSessionConfig,
    pub auto_update_cfg: crate::auto_update::AutoUpdateConfig,
    pub plugins_cfg: crate::plugin_system::PluginSystemConfig,
    pub cloud_sync_cfg: crate::cloud_sync::CloudSyncConfig,
    pub workflows_cfg: crate::workflows::WorkflowsConfig,
    pub notebooks_cfg: crate::notebooks::NotebooksConfig,
    pub background_cfg: crate::background_image::BackgroundImageConfig,
    pub window_effects_cfg: crate::window_effects::WindowEffectsConfig,
    pub focus_follows_cfg: crate::focus_follows_mouse::FocusFollowsMouseConfig,
    pub focus_follows: crate::focus_follows_mouse::FocusFollowsMouseState,
    pub haptic_cfg: crate::macos_haptic::HapticConfig,
    pub theme_cfg: crate::custom_theme::CustomThemeConfig,
    pub emoji_cfg: crate::emoji_rendering::EmojiRenderingConfig,
    pub pane_capture_cfg: crate::pane_capture::PaneCaptureConfig,
    pub print_cfg: crate::print_support::PrintSupportConfig,
    pub accessibility_cfg: crate::accessibility::AccessibilityConfig,
    pub accessibility: crate::accessibility::AccessibilityState,
    // T-core-graphics v3: 后台图像解码通道。
    //
    // `image_decode_tx`: 主线程在收到 `Event::GraphicsImage` 时,把待解码图像
    // 推入。worker 线程解码后通过 `image_decode_rx` 回传 RGBA 像素。
    //
    // 主线程在 `prepare()` 前 `try_recv()` 排空所有就绪的 `DecodedImage`,
    // 喂给 `ImageQuadState::insert` 后进入 GPU atlas。
    //
    // 通道在 `P4Features::load` 时由 `image_decode_worker::spawn()` 创建,
    // 跟随 `P4Features` 整个生命周期(进程退出时自动 drop)。
    pub image_decode_tx:
        std::sync::mpsc::Sender<alacritty_terminal::event::GraphicsImage>,
    /// T-core-graphics v3: 后台解码 worker 的接收端。`Receiver` 不能 `Clone`,
    /// 所以包成 `Arc<Mutex<...>>` 让 `P4Features` 整体可 `Clone`。
    /// 主线程在 `prepare()` 前持有锁 `try_recv()` 排空。
    pub image_decode_rx:
        std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Receiver<crate::image_decode_worker::DecodedImage>>>,
}

impl P4Features {
    pub fn load(path: &std::path::Path) -> Self {
        let terminal_graphics_cfg = crate::app_config::AppConfig::load_terminal_graphics(path);
        // T-core-graphics v3: 后台图像解码通道(独立线程,主线程不阻塞)。
        let (image_decode_tx, image_decode_rx) = crate::image_decode_worker::spawn();
        let image_decode_rx = std::sync::Arc::new(std::sync::Mutex::new(image_decode_rx));
        Self {
            voice_input_cfg: crate::app_config::AppConfig::load_voice_input(path),
            voice_input: crate::voice_input::VoiceInputState::default(),
            terminal_graphics_cfg: terminal_graphics_cfg.clone(),
            graphics_cache: crate::terminal_graphics::GraphicsCache::new(
                terminal_graphics_cfg.max_images,
            ),
            // T-core-closure:活跃图像存储取代已成死代码的
            // `GraphicsCache`——当 GUI 渲染路径开始
            // 从中拉取时生效。旧缓存保留给仍只想要
            // 元数据占位的调用方。
            image_store: crate::terminal_graphics::ImageStore::new(
                terminal_graphics_cfg.max_images,
                terminal_graphics_cfg.max_image_bytes,
            ),
            graphics_assembler: crate::terminal_graphics::GraphicsAssembler::new(),
            glass_cfg: crate::app_config::AppConfig::load_glass_effect(path),
            touch_bar_cfg: crate::app_config::AppConfig::load_touch_bar(path),
            tab_bar_cfg: crate::app_config::AppConfig::load_visual_tab_bar(path),
            window_border_cfg: crate::app_config::AppConfig::load_window_border(path),
            badge_cfg: crate::app_config::AppConfig::load_badge_overlay(path),
            file_preview_cfg: crate::app_config::AppConfig::load_file_preview(path),
            file_transfer_cfg: crate::app_config::AppConfig::load_file_transfer(path),
            file_transfer: crate::file_transfer::FileTransferState::default(),
            shared_session_cfg: crate::app_config::AppConfig::load_shared_session(path),
            auto_update_cfg: crate::app_config::AppConfig::load_auto_update(path),
            plugins_cfg: crate::app_config::AppConfig::load_plugins(path),
            cloud_sync_cfg: crate::app_config::AppConfig::load_cloud_sync(path),
            workflows_cfg: crate::app_config::AppConfig::load_workflows_enabled(path),
            notebooks_cfg: crate::app_config::AppConfig::load_notebooks(path),
            background_cfg: crate::app_config::AppConfig::load_background_image(path),
            window_effects_cfg: crate::app_config::AppConfig::load_window_effects(path),
            focus_follows_cfg: crate::app_config::AppConfig::load_focus_follows_mouse(path),
            focus_follows: crate::focus_follows_mouse::FocusFollowsMouseState::default(),
            haptic_cfg: crate::app_config::AppConfig::load_haptic(path),
            theme_cfg: crate::app_config::AppConfig::load_custom_theme(path),
            emoji_cfg: crate::app_config::AppConfig::load_emoji_rendering(path),
            pane_capture_cfg: crate::app_config::AppConfig::load_pane_capture(path),
            print_cfg: crate::app_config::AppConfig::load_print_support(path),
            accessibility_cfg: crate::app_config::AppConfig::load_accessibility(path),
            accessibility: crate::accessibility::AccessibilityState::default(),
            // T-core-graphics v3: 后台图像解码通道(Sender/Receiver)。
            image_decode_tx,
            image_decode_rx,
        }
    }

    pub fn reload_configs(&mut self, path: &std::path::Path) {
        *self = Self::load(path);
    }

    /// 配置加载后应用平台副作用(毛玻璃、模糊、触控条)。
    pub fn apply_platform_effects(&self) {
        crate::macos_glass_effect::apply_glass_effect(&self.glass_cfg);
        crate::window_effects::apply_blur(&self.window_effects_cfg);
        crate::macos_touch_bar::refresh_touch_bar(&self.touch_bar_cfg);
    }

    /// 由基础配置 + `[window.effects]` 综合得出的窗口不透明度。
    pub fn effective_window_opacity(&self, base: f32) -> f32 {
        self.window_effects_cfg.effective_opacity(base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_defaults_without_panic() {
        let p = std::path::Path::new("/nonexistent/pureterm.toml");
        let _f = P4Features::load(p);
    }
}
