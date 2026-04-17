//! 欢迎页面 — 当没有打开标签页或首次启动时显示。

use egui::{Id, RichText, Vec2};
use pureterm_core::config::AppConfig;
use pureterm_core::profile::{Profile, ProfileOptions, ProfileType};

/// 欢迎页面的状态。
pub struct WelcomePage {
    /// 欢迎页面是否可见。
    pub visible: bool,
    /// 快速连接输入字段。
    pub quick_connect: String,
    /// 用于连接的被选中的配置索引。
    selected_profile: Option<usize>,
}

impl Default for WelcomePage {
    fn default() -> Self {
        Self {
            visible: true,
            quick_connect: String::new(),
            selected_profile: None,
        }
    }
}

impl WelcomePage {
    pub fn new() -> Self {
        Self::default()
    }

    /// 将十六进制颜色字符串解析为 egui 颜色。
    fn parse_color(hex: &str) -> egui::Color32 {
        let hex = hex.trim_start_matches('#');
        if hex.len() >= 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(128);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(128);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(128);
            egui::Color32::from_rgb(r, g, b)
        } else {
            egui::Color32::GRAY
        }
    }

    /// 显示欢迎页面并在选择时返回要连接的配置。
    pub fn show_page(&mut self, ctx: &egui::Context, config: &AppConfig) -> Option<Profile> {
        let mut result = None;

        if !self.visible {
            return None;
        }

        let screen_rect = ctx.content_rect();
        let panel_width = 500.0;
        let panel_height = 450.0;
        let x = (screen_rect.width() - panel_width) / 2.0;
        let y = (screen_rect.height() - panel_height) / 2.0;

        egui::Area::new(Id::new("welcome_page"))
            .fixed_pos(egui::Pos2::new(x, y))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_width(panel_width);

                egui::Frame::window(ui.style()).show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(20.0);

                        // Title
                        ui.heading(RichText::new("PureTerm").size(32.0).strong());

                        ui.add_space(10.0);

                        // Subtitle
                        ui.label(RichText::new("GPU-accelerated terminal emulator").small());

                        ui.add_space(30.0);

                        // Quick connect
                        ui.vertical(|ui| {
                            ui.set_max_width(400.0);

                            ui.label("Quick Connect:");
                            ui.add_space(5.0);

                            let quick_input = egui::TextEdit::singleline(&mut self.quick_connect)
                                .hint_text("user@host, host:port, /dev/ttyUSB0...")
                                .desired_width(400.0);

                            ui.add(quick_input);

                            ui.add_space(10.0);

                            if ui.button("Connect").clicked() && !self.quick_connect.is_empty() {
                                // Try to parse as quick connect and create a profile
                                if let Some(profile) = self.parse_quick_connect(&self.quick_connect) {
                                    result = Some(profile);
                                    self.visible = false;
                                    self.quick_connect.clear();
                                }
                            }
                        });

                        ui.add_space(20.0);

                        // New local terminal button
                        if ui.button("New Local Terminal").clicked() {
                            result = Some(self.create_local_profile());
                            self.visible = false;
                        }

                        ui.add_space(30.0);

                        // Profiles list
                        if !config.profiles.is_empty() {
                            ui.separator();
                            ui.add_space(10.0);
                            ui.label("Profiles:");

                            ui.add_space(5.0);

                            // Show profile cards
                            let profiles: Vec<_> = config.profiles.iter().enumerate().collect();
                            for (idx, profile) in profiles {
                                let is_selected = self.selected_profile == Some(idx);

                                ui.horizontal(|ui| {
                                    // Color indicator (small filled rectangle)
                                    let color = profile.color.as_ref()
                                        .map(|c| Self::parse_color(c))
                                        .unwrap_or_else(|| egui::Color32::GRAY);
                                    let rect = egui::Rect::from_min_size(ui.cursor().min, Vec2::splat(12.0));
                                    ui.painter().rect_filled(rect, 2.0, color);

                                    ui.add_space(8.0);

                                    // Profile name
                                    let name = profile.name.clone();
                                    let profile_type = format!("{:?}", profile.profile_type);

                                    if ui.selectable_label(is_selected, format!("{} ({})", name, profile_type)).clicked() {
                                        self.selected_profile = Some(idx);
                                    }
                                });
                            }

                            // Connect button for selected profile
                            if let Some(idx) = self.selected_profile {
                                ui.add_space(10.0);
                                if ui.button("Connect to Selected").clicked() {
                                    if let Some(profile) = config.profiles.get(idx) {
                                        result = Some(profile.clone());
                                        self.visible = false;
                                        self.selected_profile = None;
                                    }
                                }
                            }
                        }
                    });
                });
            });

        result
    }

    /// Parse quick connect string and create a profile.
    fn parse_quick_connect(&self, input: &str) -> Option<Profile> {
        let input = input.trim();
        if input.is_empty() {
            return None;
        }

        // user@host -> SSH
        if let Some(at) = input.find('@') {
            let user = input[..at].to_string();
            let host_part = input[at + 1..].to_string();

            let (host, port) = if let Some(colon) = host_part.find(':') {
                (host_part[..colon].to_string(), host_part[colon + 1..].parse().ok())
            } else {
                (host_part, None)
            };

            let port = port.unwrap_or(22);

            return Some(Profile {
                id: uuid::Uuid::new_v4().to_string(),
                name: format!("{}@{}", user, host),
                profile_type: ProfileType::Ssh,
                group: None,
                color: Some("#4A9EFF".to_string()),
                icon: None,
                options: ProfileOptions {
                    host: Some(host),
                    port: Some(port),
                    user: Some(user),
                    auth_method: Some("prompt".to_string()),
                    ..Default::default()
                },
            });
        }

        // /dev/tty* 或 COM* -> 串口
        if input.starts_with("/dev/tty") || input.starts_with("COM") {
            let path = input.to_string();
            return Some(Profile {
                id: uuid::Uuid::new_v4().to_string(),
                name: format!("Serial {}", path),
                profile_type: ProfileType::Serial,
                group: None,
                color: Some("#50C878".to_string()),
                icon: None,
                options: ProfileOptions {
                    path: Some(path),
                    baud_rate: Some(115200),
                    data_bits: Some(8),
                    parity: Some("N".to_string()),
                    stop_bits: Some(1),
                    ..Default::default()
                },
            });
        }

        // host:port -> TCP (Telnet 风格)
        if let Some(colon) = input.find(':') {
            let host = input[..colon].to_string();
            if let Ok(port) = input[colon + 1..].parse::<u16>() {
                return Some(Profile {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: format!("{}:{}", host, port),
                    profile_type: ProfileType::Telnet,
                    group: None,
                    color: Some("#FFB347".to_string()),
                    icon: None,
                    options: ProfileOptions {
                        telnet_host: Some(host),
                        telnet_port: Some(port),
                        ..Default::default()
                    },
                });
            }
        }

        // 只是一个 host -> 默认端口的 SSH
        let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
        Some(Profile {
            id: uuid::Uuid::new_v4().to_string(),
            name: input.to_string(),
            profile_type: ProfileType::Ssh,
            group: None,
            color: Some("#4A9EFF".to_string()),
            icon: None,
            options: ProfileOptions {
                host: Some(input.to_string()),
                port: Some(22),
                user: Some(user),
                auth_method: Some("prompt".to_string()),
                ..Default::default()
            },
        })
    }

    /// 创建默认的本地配置。
    fn create_local_profile(&self) -> Profile {
        Profile {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Local".to_string(),
            profile_type: ProfileType::Local,
            group: None,
            color: Some("#FFFFFF".to_string()),
            icon: None,
            options: ProfileOptions::default(),
        }
    }

    /// 隐藏欢迎页面。
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// 显示欢迎页面。
    pub fn show(&mut self) {
        self.visible = true;
    }
}
