//! Lock screen — password-protect the terminal when away.

use egui::{TextEdit, Ui};

/// Lock screen state.
pub struct LockScreen {
    pub active: bool,
    password_hash: Option<String>,
    input: String,
    shake_offset: f32,
    failed_attempts: u32,
}

impl Default for LockScreen {
    fn default() -> Self {
        Self {
            active: false,
            password_hash: None,
            input: String::new(),
            shake_offset: 0.0,
            failed_attempts: 0,
        }
    }
}

impl LockScreen {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the lock password.
    pub fn set_password(&mut self, password: &str) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        password.hash(&mut hasher);
        self.password_hash = Some(format!("{:016x}", hasher.finish()));
    }

    /// Lock the screen.
    pub fn lock(&mut self) {
        if self.password_hash.is_some() {
            self.active = true;
            self.input.clear();
            self.shake_offset = 0.0;
            self.failed_attempts = 0;
        }
    }

    /// Unlock the screen (check password).
    pub fn try_unlock(&mut self) -> bool {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        self.input.hash(&mut hasher);
        let input_hash = format!("{:016x}", hasher.finish());

        if Some(&input_hash) == self.password_hash.as_ref() {
            self.active = false;
            self.input.clear();
            self.failed_attempts = 0;
            true
        } else {
            self.failed_attempts += 1;
            self.shake_offset = 10.0;
            self.input.clear();
            false
        }
    }

    /// Whether the lock screen has a password set.
    pub fn has_password(&self) -> bool {
        self.password_hash.is_some()
    }

    /// Remove the password.
    pub fn clear_password(&mut self) {
        self.password_hash = None;
        self.active = false;
    }

    /// Show the lock screen overlay. Returns true if unlocked.
    pub fn show(&mut self, ui: &mut Ui) -> bool {
        if !self.active {
            return true;
        }

        // Shake animation decay
        if self.shake_offset > 0.0 {
            self.shake_offset *= 0.9;
            if self.shake_offset < 0.1 {
                self.shake_offset = 0.0;
            }
        }

        let screen_rect = ui.ctx().screen_rect();
        let offset = self.shake_offset * (if self.failed_attempts % 2 == 0 { 1.0 } else { -1.0 });

        egui::Area::new(egui::Id::new("lock_screen"))
            .fixed_pos(screen_rect.min)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                ui.set_min_size(screen_rect.size());
                ui.painter().rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(230));

                ui.vertical_centered(|ui| {
                    ui.add_space(screen_rect.height() * 0.3);

                    ui.heading("PureTerm");
                    ui.add_space(8.0);
                    ui.label("Terminal is locked");
                    if self.failed_attempts > 0 {
                        ui.colored_label(
                            egui::Color32::RED,
                            format!("{} failed attempt(s)", self.failed_attempts),
                        );
                    }
                    ui.add_space(16.0);

                    let x = ui.cursor().min.x + offset;
                    ui.put(
                        egui::Rect::from_min_size(
                            egui::Pos2::new(x, ui.cursor().min.y),
                            egui::Vec2::new(300.0, 20.0),
                        ),
                        TextEdit::singleline(&mut self.input)
                            .hint_text("Enter password...")
                            .password(true)
                            .desired_width(300.0),
                    );

                    ui.add_space(8.0);
                    if ui.button("Unlock").clicked() {
                        self.try_unlock();
                    }

                    // Enter to unlock
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.try_unlock();
                    }
                });
            });

        !self.active
    }
}
