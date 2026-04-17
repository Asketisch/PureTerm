//! PureTerm 的配置文件选择面板。
//!
//! 提供可搜索的连接配置列表,用于快速创建会话。
//! 支持快速连接 via user@host, host:port 等。

use egui::{Id, RichText, TextEdit};
use pureterm_core::config::AppConfig;
use pureterm_core::profile::{Profile, ProfileOptions, ProfileType};

/// 配置文件选择器状态。
pub struct ProfileSelector {
    pub visible: bool,
    pub query: String,
    pub selected_index: usize,
}

impl Default for ProfileSelector {
    fn default() -> Self {
        Self {
            visible: false,
            query: String::new(),
            selected_index: 0,
        }
    }
}

impl ProfileSelector {
    /// 显示配置文件选择器。如用户选择一个则返回 Profile。
    pub fn show(&mut self, ctx: &egui::Context, config: &AppConfig) -> Option<Profile> {
        let mut result = None;

        // 按 Escape 关闭
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.visible = false;
            self.query.clear();
            return None;
        }

        let query_lower = self.query.to_lowercase();

        // 根据查询过滤配置
        let filtered: Vec<&Profile> = config.profiles
            .iter()
            .filter(|p| {
                query_lower.is_empty()
                    || p.name.to_lowercase().contains(&query_lower)
                    || p.options.host.as_ref().map_or(false, |h| h.to_lowercase().contains(&query_lower))
                    || p.options.telnet_host.as_ref().map_or(false, |h| h.to_lowercase().contains(&query_lower))
                    || p.options.path.as_ref().map_or(false, |h| h.to_lowercase().contains(&query_lower))
            })
            .collect();

        // 方向键导航
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if !filtered.is_empty() {
                self.selected_index = (self.selected_index + 1).min(filtered.len());
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if self.selected_index > 0 {
                self.selected_index -= 1;
            }
        }

        // 输入时自动选择第一个匹配项
        if !query_lower.is_empty() && !filtered.is_empty() && self.selected_index >= filtered.len() {
            self.selected_index = 0;
        }

        let screen_rect = ctx.content_rect();
        let selector_width = 420.0;
        let selector_height = 360.0;
        let x = (screen_rect.width() - selector_width) / 2.0;
        let y = screen_rect.height() * 0.15;

        egui::Area::new(Id::new("profile_selector"))
            .fixed_pos(egui::Pos2::new(x, y))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_width(selector_width);

                egui::Frame::window(ui.style()).show(ui, |ui| {
                    ui.vertical(|ui| {
                        // 搜索输入框
                        let response = ui.add(
                            TextEdit::singleline(&mut self.query)
                                .hint_text("Search profiles or enter user@host for quick connect...")
                                .font(egui::TextStyle::Body)
                                .desired_width(f32::INFINITY),
                        );
                        response.request_focus();

                        ui.separator();

                        // 快速连接部分(如果查询看起来像连接字符串)
                        let mut quick_connect_idx = 0;
                        if !self.query.is_empty() && !self.query.contains(' ') {
                            let quick_profiles = self.quick_connect_profiles(&self.query);
                            quick_connect_idx = quick_profiles.len();
                            for (i, qp) in quick_profiles.iter().enumerate() {
                                let is_selected = self.selected_index == i;
                                let icon = profile_type_icon(qp.profile_type);
                                let label = format!("{} {}  {}", icon, qp.name, quick_connect_description(qp));

                                let response = ui.selectable_label(
                                    is_selected,
                                    RichText::new(&label),
                                );
                                if response.clicked() || (is_selected && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                                    result = Some(qp.clone());
                                    self.visible = false;
                                    self.query.clear();
                                }
                            }
                            if !quick_profiles.is_empty() {
                                ui.separator();
                            }
                        }

                        // Profile list
                        egui::ScrollArea::vertical()
                            .max_height(selector_height - 120.0)
                            .show(ui, |ui| {
                                // 始终可用的"新建本地终端"
                                {
                                    let local_idx = quick_connect_idx;
                                    let is_selected = self.selected_index == local_idx;
                                    let response = ui.selectable_label(
                                        is_selected,
                                        RichText::new(">> New Local Terminal"),
                                    );
                                    if response.clicked() || (is_selected && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                                        result = Some(Profile {
                                            id: "local-default".into(),
                                            name: "Local Terminal".into(),
                                            profile_type: ProfileType::Local,
                                            group: None,
                                            options: ProfileOptions::default(),
                                            color: None,
                                            icon: None,
                                        });
                                        self.visible = false;
                                        self.query.clear();
                                    }
                                }

                                for (i, profile) in filtered.iter().enumerate() {
                                    let idx = quick_connect_idx + 1 + i;
                                    let is_selected = self.selected_index == idx;
                                    let icon = profile_type_icon(profile.profile_type);
                                    let desc = profile_description(profile);
                                    let label = format!("{} {}  {}", icon, profile.name, desc);

                                    // Color indicator
                                    ui.horizontal(|ui| {
                                        if let Some(ref color) = profile.color {
                                            let cv = parse_hex_color(color);
                                            ui.painter().rect_filled(
                                                egui::Rect::from_min_size(ui.cursor().min, egui::vec2(4.0, 16.0)),
                                                0.0,
                                                egui::Color32::from_rgb(cv[0], cv[1], cv[2]),
                                            );
                                            ui.add_space(6.0);
                                        }

                                        let response = ui.selectable_label(
                                            is_selected,
                                            RichText::new(&label),
                                        );
                                        if response.clicked() || (is_selected && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                                            result = Some((*profile).clone());
                                            self.visible = false;
                                            self.query.clear();
                                        }
                                    });
                                }

                                // 限制选中索引范围
                                let max_idx = quick_connect_idx + filtered.len();
                                if max_idx > 0 {
                                    self.selected_index = self.selected_index.min(max_idx);
                                }
                            });

                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Ctrl+Shift+P to toggle").small().weak());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Close").clicked() {
                                    self.visible = false;
                                    self.query.clear();
                                }
                            });
                        });
                    });
                });
            });

        if result.is_some() {
            self.visible = false;
            self.query.clear();
        }

        result
    }

    /// 从查询字符串生成快速连接配置。
    fn quick_connect_profiles(&self, query: &str) -> Vec<Profile> {
        let mut profiles = Vec::new();
        let trimmed = query.trim();

        // SSH: user@host[:port]
        if trimmed.contains('@') && !trimmed.starts_with('/') {
            let parts: Vec<&str> = trimmed.splitn(2, '@').collect();
            if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                let (host, port) = if let Some((h, p)) = parts[1].rsplit_once(':') {
                    if let Ok(p) = p.parse::<u16>() {
                        (h.to_string(), p)
                    } else {
                        (parts[1].to_string(), 22)
                    }
                } else {
                    (parts[1].to_string(), 22)
                };
                profiles.push(Profile {
                    id: format!("qc-ssh-{}", trimmed),
                    name: format!("{}@{}", parts[0], host),
                    profile_type: ProfileType::Ssh,
                    group: None,
                    options: ProfileOptions {
                        host: Some(host),
                        port: Some(port),
                        user: Some(parts[0].to_string()),
                        auth_method: Some("prompt".to_string()),
                        ..Default::default()
                    },
                    color: None,
                    icon: None,
                });
            }
        }

        // Telnet: host[:port] (if looks like a hostname or IP)
        if !trimmed.contains('@') && !trimmed.contains('/') && !trimmed.is_empty() {
            let (host, port) = if let Some((h, p)) = trimmed.rsplit_once(':') {
                if let Ok(p) = p.parse::<u16>() {
                    (h.to_string(), p)
                } else {
                    (trimmed.to_string(), 23)
                }
            } else {
                (trimmed.to_string(), 23)
            };

            // Only suggest telnet if it looks like a hostname (has dots or is an IP)
            if host.contains('.') || host.chars().all(|c| c.is_ascii_digit() || c == '.') {
                profiles.push(Profile {
                    id: format!("qc-telnet-{}", trimmed),
                    name: format!("Telnet {}", host),
                    profile_type: ProfileType::Telnet,
                    group: None,
                    options: ProfileOptions {
                        telnet_host: Some(host),
                        telnet_port: Some(port),
                        ..Default::default()
                    },
                    color: None,
                    icon: None,
                });
            }
        }

        // Serial: /dev/ttyXXX[:baud]
        if trimmed.starts_with("/dev/") || trimmed.starts_with("COM") {
            let (path, baud) = if let Some((p, b)) = trimmed.rsplit_once(':') {
                if let Ok(b) = b.parse::<u32>() {
                    (p.to_string(), b)
                } else {
                    (trimmed.to_string(), 9600)
                }
            } else {
                (trimmed.to_string(), 9600)
            };
            profiles.push(Profile {
                id: format!("qc-serial-{}", trimmed),
                name: format!("Serial {}", path),
                profile_type: ProfileType::Serial,
                group: None,
                options: ProfileOptions {
                    path: Some(path),
                    baud_rate: Some(baud),
                    data_bits: Some(8),
                    stop_bits: Some(1),
                    parity: Some("none".to_string()),
                    flow_control: Some("none".to_string()),
                    ..Default::default()
                },
                color: None,
                icon: None,
            });
        }

        profiles
    }
}

fn profile_type_icon(t: ProfileType) -> &'static str {
    match t {
        ProfileType::Local => "[Local]",
        ProfileType::Ssh => "[SSH]",
        ProfileType::Serial => "[Serial]",
        ProfileType::Telnet => "[Telnet]",
    }
}

fn profile_description(p: &Profile) -> String {
    match p.profile_type {
        ProfileType::Ssh => {
            let host = p.options.host.as_deref().unwrap_or("?");
            let port = p.options.port.unwrap_or(22);
            if port == 22 { host.to_string() } else { format!("{}:{}", host, port) }
        }
        ProfileType::Serial => {
            let path = p.options.path.as_deref().unwrap_or("?");
            let baud = p.options.baud_rate.unwrap_or(9600);
            format!("{} @ {} baud", path, baud)
        }
        ProfileType::Telnet => {
            let host = p.options.telnet_host.as_deref().unwrap_or("?");
            let port = p.options.telnet_port.unwrap_or(23);
            if port == 23 { host.to_string() } else { format!("{}:{}", host, port) }
        }
        ProfileType::Local => {
            let shell = p.options.shell.as_deref().unwrap_or("default");
            shell.to_string()
        }
    }
}

fn quick_connect_description(p: &Profile) -> String {
    match p.profile_type {
        ProfileType::Ssh => "Quick SSH".into(),
        ProfileType::Serial => "Quick Serial".into(),
        ProfileType::Telnet => "Quick Telnet".into(),
        ProfileType::Local => "Quick Local".into(),
    }
}

fn parse_hex_color(s: &str) -> [u8; 3] {
    let s = s.trim_start_matches('#');
    if s.len() >= 6 {
        let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(128);
        let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(128);
        let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(128);
        [r, g, b]
    } else {
        [128, 128, 128]
    }
}
