pub mod appearance;
pub mod colors;
pub mod general;
pub mod keybindings;
pub mod ssh_profiles;
pub mod terminal;

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
    pub fn title(&self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Appearance => "Appearance",
            Page::Colors => "Colors",
            Page::Terminal => "Terminal",
            Page::SshProfiles => "SSH Profiles",
            Page::Keybindings => "Keyboard Shortcuts",
        }
    }
}
