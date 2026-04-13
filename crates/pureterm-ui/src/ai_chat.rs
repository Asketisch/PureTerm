//! AI chat panel for terminal assistance.

use egui::{RichText, ScrollArea, TextEdit, Ui};

/// AI chat panel state.
pub struct AiChatPanel {
    pub visible: bool,
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub loading: bool,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
}

impl Default for AiChatPanel {
    fn default() -> Self {
        Self {
            visible: false,
            messages: vec![ChatMessage {
                role: ChatRole::System,
                content: "AI assistant ready. Ask me to explain errors, suggest commands, or debug issues.".into(),
                timestamp: Self::current_time(),
            }],
            input: String::new(),
            loading: false,
        }
    }
}

impl AiChatPanel {
    pub fn new() -> Self {
        Self::default()
    }

    fn current_time() -> String {
        let now = std::time::SystemTime::now();
        let duration = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        let secs = duration.as_secs();
        format!("{:02}:{:02}", (secs / 60) % 60, secs % 60)
    }

    /// Show the AI chat panel.
    pub fn show(&mut self, ui: &mut Ui) -> Option<AiChatAction> {
        if !self.visible {
            return None;
        }

        let mut action = None;

        egui::SidePanel::right("ai_chat_panel")
            .min_width(280.0)
            .default_width(350.0)
            .show_inside(ui, |ui| {
                ui.heading("AI Assistant");
                ui.add_space(4.0);

                // Quick action buttons
                ui.horizontal(|ui| {
                    if ui.button("Explain Error").clicked() {
                        action = Some(AiChatAction::ExplainLastError);
                    }
                    if ui.button("Suggest").clicked() {
                        action = Some(AiChatAction::SuggestCommand);
                    }
                    if ui.button("Fix").clicked() {
                        action = Some(AiChatAction::FixCommand);
                    }
                });

                ui.separator();

                // Chat messages
                ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for msg in &self.messages {
                            let (color, prefix) = match msg.role {
                                ChatRole::User => (egui::Color32::from_rgb(100, 180, 255), "You"),
                                ChatRole::Assistant => (egui::Color32::from_rgb(100, 255, 150), "AI"),
                                ChatRole::System => (egui::Color32::from_rgb(200, 200, 200), "System"),
                            };
                            ui.colored_label(color, RichText::new(format!("[{}] {}", prefix, msg.content)).monospace());
                            ui.add_space(4.0);
                        }

                        if self.loading {
                            ui.label(RichText::new("Thinking...").italics());
                        }
                    });

                ui.separator();

                // Input
                ui.horizontal(|ui| {
                    let response = ui.add(
                        TextEdit::singleline(&mut self.input)
                            .hint_text("Ask anything...")
                            .desired_width(f32::INFINITY),
                    );

                    if (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        || ui.button("Send").clicked()
                    {
                        if !self.input.is_empty() {
                            self.messages.push(ChatMessage {
                                role: ChatRole::User,
                                content: self.input.clone(),
                                timestamp: Self::current_time(),
                            });
                            action = Some(AiChatAction::SendMessage(self.input.clone()));
                            self.input.clear();
                        }
                    }
                });
            });

        action
    }

    /// Add an assistant message.
    pub fn add_assistant_message(&mut self, content: &str) {
        self.messages.push(ChatMessage {
            role: ChatRole::Assistant,
            content: content.to_string(),
            timestamp: Self::current_time(),
        });
        self.loading = false;
    }

    /// Open the chat panel.
    pub fn open(&mut self) {
        self.visible = true;
    }

    /// Close the chat panel.
    pub fn close(&mut self) {
        self.visible = false;
    }
}

#[derive(Debug, Clone)]
pub enum AiChatAction {
    SendMessage(String),
    ExplainLastError,
    SuggestCommand,
    FixCommand,
}
