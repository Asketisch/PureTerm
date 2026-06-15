use std::path::PathBuf;
use std::time::SystemTime;

use iced::widget::{
    button, column, container, image, row, rule, scrollable, space, stack, text,
};
use iced::{Color, window, ContentFit, Element, Length, Padding, Task};

use crate::config_io;
use crate::i18n::I18n;
use crate::pages::appearance::{AppearanceMsg, AppearanceState};
use crate::pages::colors::{ColorPickerTarget, ColorsMsg, ColorsState};
use crate::pages::general::{GeneralMsg, GeneralState};
use crate::pages::keybindings::KeybindingsState;
use crate::pages::ssh_profiles::{SshMsg, SshState};
use crate::pages::terminal::{TerminalMsg, TerminalState};
use crate::pages::Page;
use crate::theme;
use crate::widgets::color_picker::hex_to_color;
use crate::widgets::sidebar;

enum StatusMessage {
    Saved,
    Reverted,
    SaveFailed(String),
    RevertFailed(String),
}

/// Reusable settings state — can be embedded in any Iced daemon.
pub struct SettingsState {
    config_path: PathBuf,
    document: toml_edit::DocumentMut,
    current_page: Page,
    dirty: bool,
    status_msg: Option<StatusMessage>,
    i18n: I18n,
    general: GeneralState,
    appearance: AppearanceState,
    colors: ColorsState,
    terminal: TerminalState,
    ssh: SshState,
    keybindings: KeybindingsState,
    pub main_window: Option<window::Id>,
    color_picker_window: Option<window::Id>,
    pending_color_target: Option<ColorPickerTarget>,
    /// Color value when the picker was opened (used to restore on cancel).
    picker_original_hex: Option<String>,
    /// Live preview value while user is adjusting the picker.
    /// Only written to state/TOML on confirm.
    picker_preview_hex: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    NavigateTo(Page),
    Save,
    /// Emitted by `Message::Save` after a successful write through
    /// `config_io::mutate_config`. Carries the new file mtime so the
    /// embedding application (PureTermApp) can record a "self just
    /// wrote at T" window and suppress the notify-watcher self-reload
    /// triggered by its own save. Standalone settings binary ignores it.
    SavedAt(SystemTime),
    Revert,
    General(GeneralMsg),
    Appearance(AppearanceMsg),
    Colors(ColorsMsg),
    Terminal(TerminalMsg),
    Ssh(SshMsg),
    MainWindowOpened(window::Id),
    ColorPickerOpened(window::Id),
    ColorPickerClosed(window::Id),
    /// PureTerm: 触发原生文件对话框选择图片作为设置窗口背景。
    PickBackgroundImage,
}

impl SettingsState {
    pub fn owns_window(&self, id: window::Id) -> bool {
        self.main_window == Some(id) || self.color_picker_window == Some(id)
    }

    pub fn new(config_path: PathBuf) -> Self {
        let document = config_io::load_document(&config_path).unwrap_or_else(|e| {
            log::warn!("Failed to load config: {e}, using empty document");
            "".parse().unwrap()
        });

        let mut i18n = I18n::new();
        let locale = config_io::get_string(&document, &["pureterm"], "locale")
            .unwrap_or_else(|| "en".to_string());
        i18n.set_locale(&locale);

        let general = GeneralState::load(&document);
        let appearance = AppearanceState::load(&document);
        let colors = ColorsState::load(&document);
        let terminal = TerminalState::load(&document);
        let ssh = SshState::load(&document);
        let keybindings = KeybindingsState::load(&document);

        Self {
            config_path,
            document,
            current_page: Page::Appearance,
            dirty: false,
            status_msg: None,
            i18n,
            general,
            appearance,
            colors,
            terminal,
            ssh,
            keybindings,
            main_window: None,
            color_picker_window: None,
            pending_color_target: None,
            picker_original_hex: None,
            picker_preview_hex: None,
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NavigateTo(page) => {
                self.current_page = page;
                Task::none()
            }
            Message::Save => {
                // Section-scoped save: reload the latest on-disk document
                // under an advisory file lock, then ask each page state
                // to replay its in-memory values onto that fresh doc.
                // This prevents the historical "open settings → main
                // process writes ssh.profiles → close settings → save
                // → just-added profile gets rolled back" scenario, and
                // also prevents the SSH-field-loss bug (sync_to_doc now
                // merges rather than replaces — see ssh_profiles.rs).
                let path = self.config_path.clone();
                let general = self.general.clone();
                let appearance = self.appearance.clone();
                let colors = self.colors.clone();
                let terminal = self.terminal.clone();
                let ssh = self.ssh.clone();
                let keybindings = self.keybindings.clone();
                let locale = self.i18n.current_locale().to_string();

                let result = config_io::mutate_config(&path, |doc| {
                    // The locale lives in [pureterm] and is normally
                    // written by the LocaleChanged handler in this file.
                    // Re-apply here so a stale on-disk locale doesn't
                    // shadow the in-effect one.
                    config_io::set_string(doc, &["pureterm"], "locale", &locale);
                    general.apply(doc);
                    appearance.apply(doc);
                    colors.apply(doc);
                    terminal.apply(doc);
                    ssh.apply(doc);
                    keybindings.apply(doc);
                    Ok(())
                });

                match result {
                    Ok(mtime) => {
                        self.dirty = false;
                        self.status_msg = Some(StatusMessage::Saved);
                        // Refresh in-memory doc snapshot so a later
                        // Revert (which reads `self.document` indirectly
                        // by re-reading disk) and any UI inspection sees
                        // the same bytes we just wrote.
                        if let Ok(fresh) = config_io::load_document(&path) {
                            self.document = fresh;
                        }
                        log::info!("Config saved to {}", path.display());
                        Task::done(Message::SavedAt(mtime))
                    }
                    Err(e) => {
                        self.status_msg = Some(StatusMessage::SaveFailed(e.to_string()));
                        log::error!("Failed to save config: {e}");
                        Task::none()
                    }
                }
            }
            Message::SavedAt(_) => {
                // No-op for the standalone settings daemon. The embedded
                // settings window (PureTermApp) intercepts this BEFORE
                // routing into SettingsState::update so it can record
                // `last_self_write` for the notify-watcher debounce.
                Task::none()
            }
            Message::Revert => {
                match config_io::load_document(&self.config_path) {
                    Ok(doc) => {
                        self.document = doc;
                        self.general = GeneralState::load(&self.document);
                        self.appearance = AppearanceState::load(&self.document);
                        self.colors = ColorsState::load(&self.document);
                        self.terminal = TerminalState::load(&self.document);
                        self.ssh = SshState::load(&self.document);
                        self.keybindings = KeybindingsState::load(&self.document);
                        self.dirty = false;
                        self.status_msg = Some(StatusMessage::Reverted);
                    }
                    Err(e) => {
                        self.status_msg = Some(StatusMessage::RevertFailed(e.to_string()));
                    }
                }
                Task::none()
            }
            Message::General(msg) => {
                if let GeneralMsg::LocaleChanged(ref display) = msg {
                    if let Some(code) = self.i18n.locale_code_for_display(display).map(|s| s.to_string()) {
                        self.i18n.set_locale(&code);
                        config_io::set_string(&mut self.document, &["pureterm"], "locale", &code);
                        // Mirror the new locale into the General page state so
                        // `GeneralState::apply` writes the in-effect locale
                        // back, not the one loaded when settings opened.
                        self.general.locale = code;
                    }
                }
                self.general.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
                Task::none()
            }
            Message::Appearance(msg) => {
                self.appearance.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
                Task::none()
            }
            Message::Colors(msg) => {
                // Handle color picker lifecycle messages specially so that
                // the picker uses a preview/confirm flow:
                // - OpenColorPicker: snapshot the current value
                // - HueSatChanged/ValueChanged: only update the preview, never commit
                // - ColorPickerResult: commit the preview and close the window
                if let ColorsMsg::OpenColorPicker(target) = &msg {
                    let current_hex = match target {
                        ColorPickerTarget::Bell => self.terminal.bell_color.clone(),
                        other => self.colors.hex_for_target(other),
                    };
                    self.pending_color_target = Some(target.clone());
                    self.picker_original_hex = Some(current_hex.clone());
                    self.picker_preview_hex = Some(current_hex);
                    let settings = window::Settings {
                        size: iced::Size::new(320.0, 450.0),
                        decorations: true,
                        resizable: false,
                        closeable: true,
                        transparent: true,
                        ..Default::default()
                    };
                    return window::open(settings).1.map(|id| Message::ColorPickerOpened(id));
                }

                if let ColorsMsg::ColorPickerHexChanged(s) = &msg {
                    // 弹窗内 hex 直接输入：仅更新预览原始文本，确认时统一提交。
                    // 非法 hex 由弹窗 view 的 unwrap_or(original_color) 兜底，色轮不跳红。
                    self.picker_preview_hex = Some(s.clone());
                    return Task::none();
                }

                if matches!(msg, ColorsMsg::HueSatChanged(..) | ColorsMsg::ValueChanged(..)) {
                    if let Some(target) = &self.pending_color_target {
                        // Compute new hex from current preview, don't touch state/TOML
                        let current = self
                            .picker_preview_hex
                            .clone()
                            .unwrap_or_else(|| self.colors.hex_for_target(target));
                        let new_hex = match &msg {
                            ColorsMsg::HueSatChanged(_, hue, sat) => {
                                let (_, _, v) = crate::widgets::color_picker::hex_to_color(&current)
                                    .map(crate::widgets::color_picker::color_to_hsv)
                                    .unwrap_or((0.0, 0.0, 1.0));
                                crate::widgets::color_picker::hsv_to_hex(*hue, *sat, v)
                            }
                            ColorsMsg::ValueChanged(_, val) => {
                                let (h, s, _) = crate::widgets::color_picker::hex_to_color(&current)
                                    .map(crate::widgets::color_picker::color_to_hsv)
                                    .unwrap_or((0.0, 0.0, 1.0));
                                crate::widgets::color_picker::hsv_to_hex(h, s, *val)
                            }
                            _ => unreachable!(),
                        };
                        self.picker_preview_hex = Some(new_hex);
                        return Task::none();
                    }
                }

                if let ColorsMsg::ColorPickerResult(target, hex) = &msg {
                    // 拒绝提交非法 hex（如弹窗里输入到一半点 OK）：不写、不关窗。
                    if hex_to_color(hex).is_none() {
                        return Task::none();
                    }
                    match target {
                        ColorPickerTarget::Bell => {
                            self.terminal.bell_color = hex.clone();
                            crate::config_io::set_string(&mut self.document, &["bell"], "color", hex);
                        }
                        other => {
                            self.colors.apply_hex(other, hex, &mut self.document);
                        }
                    }
                    self.dirty = true;
                    self.status_msg = None;
                    let window_id = self.color_picker_window;
                    self.color_picker_window = None;
                    self.pending_color_target = None;
                    self.picker_original_hex = None;
                    self.picker_preview_hex = None;
                    return match window_id {
                        Some(id) => window::close(id),
                        None => Task::none(),
                    };
                }

                self.colors.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
                Task::none()
            }
            Message::Terminal(msg) => {
                self.terminal.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
                Task::none()
            }
            Message::Ssh(msg) => {
                self.ssh.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
                Task::none()
            }
            Message::MainWindowOpened(id) => {
                self.main_window = Some(id);
                Task::none()
            }
            Message::ColorPickerOpened(id) => {
                self.color_picker_window = Some(id);
                Task::none()
            }
            Message::ColorPickerClosed(id) => {
                if self.color_picker_window == Some(id) {
                    self.color_picker_window = None;
                    self.pending_color_target = None;
                    self.picker_original_hex = None;
                    self.picker_preview_hex = None;
                    return window::close(id);
                }
                Task::none()
            }
            Message::PickBackgroundImage => {
                // PureTerm: rfd 跨平台异步文件选择器（macOS NSOpenPanel / Linux GTK /
                // Windows native dialog），通过 Task::perform 把结果转回主消息循环。
                let task = Task::perform(
                    rfd::AsyncFileDialog::new()
                        .add_filter("Image", &["png", "jpg", "jpeg", "bmp", "webp", "gif"])
                        .set_title("选择背景图片")
                        .pick_file(),
                    |maybe| {
                        Message::Appearance(AppearanceMsg::BackgroundImagePicked(
                            maybe.map(|h| h.path().to_path_buf()),
                        ))
                    },
                );
                task
            }
        }
    }

    pub fn view(&self, window: window::Id) -> Element<'_, Message> {
        if let Some(picker_id) = self.color_picker_window {
            if picker_id == window {
                if let Some(target) = &self.pending_color_target {
                    // Show live preview while the picker is open, not the committed value
                    let hex = self
                        .picker_preview_hex
                        .clone()
                        .unwrap_or_else(|| match target {
                            ColorPickerTarget::Bell => self.terminal.bell_color.clone(),
                            other => self.colors.hex_for_target(other),
                        });
                    // 弹窗里 hex 非法时色轮回退到的「上一有效色」。
                    let original_color = self
                        .picker_original_hex
                        .as_ref()
                        .and_then(|h| hex_to_color(h))
                        .unwrap_or(Color::BLACK);
                    return crate::pages::colors::color_picker_window_view(
                        target.clone(),
                        &hex,
                        original_color,
                        window,
                    );
                }
            }
        }

        if self.main_window != Some(window) {
            return container(text("Loading..."))
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        let page_content: Element<Message> = match self.current_page {
            Page::General => crate::pages::general::view(&self.general, &self.i18n),
            Page::Appearance => crate::pages::appearance::view(&self.appearance, &self.i18n),
            Page::Colors => crate::pages::colors::main_view(&self.colors, &self.i18n),
            Page::Terminal => crate::pages::terminal::view(&self.terminal, &self.i18n),
            Page::SshProfiles => crate::pages::ssh_profiles::view(&self.ssh, &self.i18n),
            Page::Keybindings => crate::pages::keybindings::view(&self.keybindings, &self.i18n),
        };

        let page_header = text(self.current_page.title(&self.i18n))
            .size(theme::FONT_SIZE + 4.0)
            .color(theme::TEXT_PRIMARY);

        let mut save_btn = button(text(self.i18n.t("footer.save")).size(theme::FONT_SIZE))
            .padding(Padding::from([6, 20]))
            .style(theme::button::primary);
        if self.dirty {
            save_btn = save_btn.on_press(Message::Save);
        }

        let revert_btn = button(text(self.i18n.t("footer.revert")).size(theme::FONT_SIZE))
            .padding(Padding::from([6, 20]))
            .style(theme::button::secondary)
            .on_press(Message::Revert);

        let status = match &self.status_msg {
            Some(StatusMessage::Saved) => text(self.i18n.t("footer.saved"))
                .size(12.0)
                .color(theme::SUCCESS),
            Some(StatusMessage::Reverted) => text(self.i18n.t("footer.reverted"))
                .size(12.0)
                .color(theme::SUCCESS),
            Some(StatusMessage::SaveFailed(e)) => {
                text(format!("{}{e}", self.i18n.t("footer.save_failed")))
                    .size(12.0)
                    .color(theme::DANGER)
            }
            Some(StatusMessage::RevertFailed(e)) => {
                text(format!("{}{e}", self.i18n.t("footer.revert_failed")))
                    .size(12.0)
                    .color(theme::DANGER)
            }
            None if self.dirty => text(self.i18n.t("footer.unsaved_changes"))
                .size(12.0)
                .color(theme::WARNING),
            None => text(String::new()).size(12.0),
        };

        let footer = row![
            text(self.config_path.display().to_string())
                .size(11.0)
                .color(theme::TEXT_SECONDARY),
            space::horizontal(),
            status,
            revert_btn,
            save_btn,
        ]
        .spacing(theme::SPACING)
        .align_y(iced::Alignment::Center)
        .padding(Padding::from([8, theme::PADDING as u16]));

        let content_area = column![
            container(page_header).padding(Padding::from([theme::PADDING as u16, theme::PADDING as u16])),
            rule::horizontal(1),
            scrollable(page_content).height(Length::Fill),
            rule::horizontal(1),
            footer,
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        let layout = row![
            sidebar::view(self.current_page, &self.i18n),
            rule::vertical(1),
            content_area,
        ]
        .height(Length::Fill);

        // PureTerm: 如果用户配置了背景图，把它放在 layout 下面作为底图。
        // 用 `stack!` 把图片与原 page 容器叠加（图片在底层）。无图时退回
        // 原来的 page 容器，保持现有 blur 效果不变。
        let main_container: Element<Message> = container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::container::page)
            .into();

        if let Some(path) = &self.appearance.background_image {
            let bg_image: Element<Message> = image(image::Handle::from_path(path.clone()))
                .opacity(self.appearance.background_image_opacity)
                .content_fit(ContentFit::Cover)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
            stack![bg_image, main_container]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            main_container
        }
    }

    pub fn title(&self, window: window::Id) -> String {
        if let Some(picker_id) = self.color_picker_window {
            if picker_id == window {
                return "Color Picker".to_string();
            }
        }
        "PureTerm Settings".to_string()
    }
}

/// Run settings as a standalone daemon (for the independent binary).
pub fn run(config_path: PathBuf) -> iced::Result {
    struct SettingsApp {
        state: SettingsState,
    }

    impl SettingsApp {
        fn new(config_path: PathBuf) -> (Self, Task<Message>) {
            let state = SettingsState::new(config_path);
            let settings = window::Settings {
                size: iced::Size::new(850.0, 620.0),
                decorations: true,
                resizable: true,
                closeable: true,
                // PureTerm: 设置窗口走 macOS 磨砂玻璃效果。
                // `transparent` 必须开启，`blur` 才会有视觉效果（背景色 alpha < 1.0
                // 才能透出磨砂层）。`rfd` 文件选择器在透明窗口上工作正常。
                // `blur` 由 Appearance 页的「Settings Window Blur」控件决定。
                transparent: true,
                blur: state.appearance.settings_window_blur,
                ..Default::default()
            };
            let app = Self { state };
            (app, window::open(settings).1.map(|id| Message::MainWindowOpened(id)))
        }

        fn update(&mut self, message: Message) -> Task<Message> {
            self.state.update(message)
        }

        fn view(&self, window: window::Id) -> Element<'_, Message> {
            self.state.view(window)
        }

        fn title(&self, window: window::Id) -> String {
            self.state.title(window)
        }

        fn subscription(&self) -> iced::Subscription<Message> {
            iced::Subscription::none()
        }
    }

    iced::daemon(
        {
            let path = config_path;
            move || SettingsApp::new(path.clone())
        },
        SettingsApp::update,
        SettingsApp::view,
    )
    .title(SettingsApp::title)
    // PureTerm: 用动态 opacity 构造调色板，使设置窗口背景随外观设置而变化。
    .theme(|app: &SettingsApp, _| {
        theme::pureterm_theme_with_opacity(app.state.appearance.opacity)
    })
    .subscription(SettingsApp::subscription)
    .run()
}
