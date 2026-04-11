use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Modifiers {
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub super_key: bool,
}

impl Modifiers {
    pub const CTRL_SHIFT: Self = Self { ctrl: true, shift: true, alt: false, super_key: false };
    pub const CTRL: Self = Self { ctrl: true, shift: false, alt: false, super_key: false };
    pub const CTRL_ALT: Self = Self { ctrl: true, alt: true, shift: false, super_key: false };
    pub const NONE: Self = Self { ctrl: false, alt: false, shift: false, super_key: false };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPattern {
    pub key: String,
    #[serde(default)]
    pub modifiers: Modifiers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hotkey {
    pub id: String,
    pub name: String,
    pub key: KeyPattern,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeysConfig {
    #[serde(default = "default_hotkeys")]
    pub bindings: Vec<Hotkey>,
}

fn default_hotkeys() -> Vec<Hotkey> {
    builtin_hotkeys().to_vec()
}

impl Default for HotkeysConfig {
    fn default() -> Self {
        Self {
            bindings: default_hotkeys(),
        }
    }
}

static BUILTIN_HOTKEYS: LazyLock<Vec<Hotkey>> = LazyLock::new(|| {
    vec![
        Hotkey { id: "tab_new".into(), name: "New Tab".into(),
            key: KeyPattern { key: "t".into(), modifiers: Modifiers::CTRL_SHIFT } },
        Hotkey { id: "tab_close".into(), name: "Close Tab".into(),
            key: KeyPattern { key: "w".into(), modifiers: Modifiers::CTRL_SHIFT } },
        Hotkey { id: "copy".into(), name: "Copy".into(),
            key: KeyPattern { key: "c".into(), modifiers: Modifiers::CTRL_SHIFT } },
        Hotkey { id: "paste".into(), name: "Paste".into(),
            key: KeyPattern { key: "v".into(), modifiers: Modifiers::CTRL_SHIFT } },
        Hotkey { id: "search".into(), name: "Search".into(),
            key: KeyPattern { key: "f".into(), modifiers: Modifiers::CTRL } },
        Hotkey { id: "split_vertical".into(), name: "Split Vertically".into(),
            key: KeyPattern { key: "d".into(), modifiers: Modifiers::CTRL_SHIFT } },
        Hotkey { id: "split_horizontal".into(), name: "Split Horizontally".into(),
            key: KeyPattern { key: "d".into(), modifiers: Modifiers::CTRL } },
        Hotkey { id: "zoom_in".into(), name: "Zoom In".into(),
            key: KeyPattern { key: "=".into(), modifiers: Modifiers::CTRL } },
        Hotkey { id: "zoom_out".into(), name: "Zoom Out".into(),
            key: KeyPattern { key: "-".into(), modifiers: Modifiers::CTRL } },
        Hotkey { id: "zoom_reset".into(), name: "Reset Zoom".into(),
            key: KeyPattern { key: "0".into(), modifiers: Modifiers::CTRL } },
        Hotkey { id: "prev_tab".into(), name: "Previous Tab".into(),
            key: KeyPattern { key: "Left".into(), modifiers: Modifiers::CTRL_SHIFT } },
        Hotkey { id: "next_tab".into(), name: "Next Tab".into(),
            key: KeyPattern { key: "Right".into(), modifiers: Modifiers::CTRL_SHIFT } },
        Hotkey { id: "select_all".into(), name: "Select All".into(),
            key: KeyPattern { key: "a".into(), modifiers: Modifiers::CTRL_SHIFT } },
        Hotkey { id: "clear".into(), name: "Clear Terminal".into(),
            key: KeyPattern { key: "l".into(), modifiers: Modifiers::CTRL_SHIFT } },
    ]
});

pub fn builtin_hotkeys() -> &'static [Hotkey] {
    &BUILTIN_HOTKEYS
}

pub struct HotkeyService {
    bindings: HashMap<String, Hotkey>,
}

impl HotkeyService {
    pub fn new(config: &HotkeysConfig) -> Self {
        let bindings = config.bindings.iter()
            .map(|h| (h.id.clone(), h.clone()))
            .collect();
        Self { bindings }
    }

    pub fn get(&self, id: &str) -> Option<&Hotkey> {
        self.bindings.get(id)
    }

    pub fn matches(&self, id: &str, key: &str, modifiers: Modifiers) -> bool {
        if let Some(hotkey) = self.bindings.get(id) {
            hotkey.key.key == key && hotkey.key.modifiers == modifiers
        } else {
            false
        }
    }

    pub fn all(&self) -> Vec<&Hotkey> {
        self.bindings.values().collect()
    }

    pub fn set(&mut self, hotkey: Hotkey) {
        self.bindings.insert(hotkey.id.clone(), hotkey);
    }
}

impl Default for HotkeyService {
    fn default() -> Self {
        Self::new(&HotkeysConfig::default())
    }
}
