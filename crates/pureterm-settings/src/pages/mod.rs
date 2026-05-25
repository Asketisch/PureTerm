pub mod appearance;
pub mod colors;
pub mod general;
pub mod keybindings;
pub mod ssh_profiles;
pub mod terminal;

use crate::i18n::I18n;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    General,
    Appearance,
    Colors,
    Terminal,
    SshProfiles,
    Keybindings,
}

impl Page {
    pub fn title(&self, i18n: &I18n) -> String {
        match self {
            Page::General => i18n.t("page.general"),
            Page::Appearance => i18n.t("page.appearance"),
            Page::Colors => i18n.t("page.colors"),
            Page::Terminal => i18n.t("page.terminal"),
            Page::SshProfiles => i18n.t("page.ssh_profiles"),
            Page::Keybindings => i18n.t("page.keybindings"),
        }
    }
}
