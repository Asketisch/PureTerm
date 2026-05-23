use iced::widget::{column, container, scrollable, text};
use iced::{Element, Length, Padding};

use crate::app::Message;
use crate::theme::{self, PADDING, SPACING, TEXT_SECONDARY};

#[derive(Debug, Clone)]
pub struct KeybindingsState {
    pub bindings: Vec<BindingEntry>,
}

#[derive(Debug, Clone)]
pub struct BindingEntry {
    pub key: String,
    pub mods: String,
    pub action: String,
}

impl Default for KeybindingsState {
    fn default() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }
}

impl KeybindingsState {
    pub fn load(doc: &toml_edit::DocumentMut) -> Self {
        let mut bindings = Vec::new();
        if let Some(kb) = doc.get("keyboard").and_then(|i| i.as_table()) {
            if let Some(arr) = kb.get("bindings").and_then(|i| i.as_array_of_tables()) {
                for item in arr.iter() {
                    let key = item
                        .get("key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let mods = item
                        .get("mods")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let action = item
                        .get("action")
                        .map(|v| v.to_string().trim_matches('"').to_string())
                        .unwrap_or_default();
                    bindings.push(BindingEntry { key, mods, action });
                }
            }
        }
        Self { bindings }
    }
}

pub fn view(state: &KeybindingsState) -> Element<'static, Message> {
    let header = iced::widget::row![
        text("Key".to_string())
            .size(11.0)
            .color(TEXT_SECONDARY)
            .width(120),
        text("Modifiers".to_string())
            .size(11.0)
            .color(TEXT_SECONDARY)
            .width(200),
        text("Action".to_string())
            .size(11.0)
            .color(TEXT_SECONDARY)
            .width(Length::Fill),
    ]
    .spacing(SPACING)
    .padding(Padding::from([4, PADDING as u16]));

    let rows: Vec<Element<Message>> = state
        .bindings
        .iter()
        .map(|b| {
            iced::widget::row![
                text(b.key.clone()).size(theme::FONT_SIZE).width(120),
                text(b.mods.clone())
                    .size(theme::FONT_SIZE)
                    .color(TEXT_SECONDARY)
                    .width(200),
                text(b.action.clone())
                    .size(theme::FONT_SIZE)
                    .width(Length::Fill),
            ]
            .spacing(SPACING)
            .padding(Padding::from([4, PADDING as u16]))
            .into()
        })
        .collect();

    let list = if rows.is_empty() {
        column![
            text("No custom keyboard bindings configured.".to_string())
                .size(theme::FONT_SIZE)
                .color(TEXT_SECONDARY),
            text("Default bindings are built into PureTerm.".to_string())
                .size(12.0)
                .color(TEXT_SECONDARY),
        ]
        .spacing(SPACING)
        .padding(Padding::from(PADDING as u16))
    } else {
        column![
            header,
            iced::widget::rule::horizontal(1),
            scrollable(
                iced::widget::Column::with_children(rows)
                    .spacing(2)
                    .width(Length::Fill),
            )
            .height(Length::Fill),
        ]
        .spacing(0)
    };

    container(list).width(Length::Fill).height(Length::Fill).into()
}
