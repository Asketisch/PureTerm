use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorScheme {
    pub name: String,
    pub foreground: String,
    pub background: String,
    pub cursor: String,
    pub selection_background: String,
    pub selection_foreground: String,
    /// 16 ANSI colors [black, red, green, yellow, blue, magenta, cyan, white,
    /// bright black, bright red, bright green, bright yellow, bright blue,
    /// bright magenta, bright cyan, bright white]
    pub colors: [String; 16],
}

impl ColorScheme {
    pub fn parse_color(hex: &str) -> [u8; 3] {
        let hex = hex.trim_start_matches('#');
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            [r, g, b]
        } else {
            [0, 0, 0]
        }
    }

    pub fn foreground_rgb(&self) -> [u8; 3] {
        Self::parse_color(&self.foreground)
    }

    pub fn background_rgb(&self) -> [u8; 3] {
        Self::parse_color(&self.background)
    }

    pub fn cursor_rgb(&self) -> [u8; 3] {
        Self::parse_color(&self.cursor)
    }

    pub fn color_rgb(&self, index: usize) -> [u8; 3] {
        if index < 16 {
            Self::parse_color(&self.colors[index])
        } else {
            [0, 0, 0]
        }
    }
}

// ── Built-in color schemes ──

macro_rules! color_scheme {
    ($name:expr, $fg:expr, $bg:expr, $cursor:expr, $sel_bg:expr, $sel_fg:expr,
     $($c:expr),*) => {
        ColorScheme {
            name: $name.to_string(),
            foreground: $fg.to_string(),
            background: $bg.to_string(),
            cursor: $cursor.to_string(),
            selection_background: $sel_bg.to_string(),
            selection_foreground: $sel_fg.to_string(),
            colors: [$($c.to_string()),*],
        }
    };
}

pub fn builtin_color_schemes() -> Vec<ColorScheme> {
    vec![
        color_scheme!(
            "Monokai Pro", "#cfcfc2", "#1e1e1e", "#f8f8f2", "#49483e", "#f8f8f2",
            "#1e1e1e", "#f92672", "#a6e22e", "#f4bf75",
            "#66d9ef", "#ae81ff", "#a1efe4", "#cfcfc2",
            "#75715e", "#f92672", "#a6e22e", "#f4bf75",
            "#66d9ef", "#ae81ff", "#a1efe4", "#f9f5f1"
        ),
        color_scheme!(
            "Monokai", "#f8f8f2", "#272822", "#f8f8f0", "#49483e", "#f8f8f2",
            "#272822", "#f92672", "#a6e22e", "#f4bf75",
            "#66d9ef", "#ae81ff", "#a1efe4", "#f8f8f2",
            "#75715e", "#fd971f", "#a6e22e", "#e6db74",
            "#66d9ef", "#ae81ff", "#a1efe4", "#f9f8f5"
        ),
        color_scheme!(
            "Solarized Dark", "#839496", "#002b36", "#93a1a1", "#073642", "#93a1a1",
            "#073642", "#dc322f", "#859900", "#b58900",
            "#268bd2", "#d33682", "#2aa198", "#eee8d5",
            "#002b36", "#cb4b16", "#586e75", "#657b83",
            "#839496", "#6c71c4", "#93a1a1", "#fdf6e3"
        ),
        color_scheme!(
            "Solarized Light", "#657b83", "#fdf6e3", "#586e75", "#eee8d5", "#586e75",
            "#eee8d5", "#dc322f", "#859900", "#b58900",
            "#268bd2", "#d33682", "#2aa198", "#073642",
            "#fdf6e3", "#cb4b16", "#93a1a1", "#839496",
            "#657b83", "#6c71c4", "#586e75", "#002b36"
        ),
        color_scheme!(
            "Dracula", "#f8f8f2", "#282a36", "#f8f8f2", "#44475a", "#f8f8f2",
            "#21222c", "#ff5555", "#50fa7b", "#f1fa8c",
            "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
            "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5",
            "#d6acff", "#ff92df", "#a4ffff", "#ffffff"
        ),
        color_scheme!(
            "One Dark", "#abb2bf", "#282c34", "#528bff", "#3e4451", "#abb2bf",
            "#282c34", "#e06c75", "#98c379", "#e5c07b",
            "#61afef", "#c678dd", "#56b6c2", "#abb2bf",
            "#5c6370", "#e06c75", "#98c379", "#e5c07b",
            "#61afef", "#c678dd", "#56b6c2", "#ffffff"
        ),
        color_scheme!(
            "GitHub Dark", "#e6edf3", "#0d1117", "#58a6ff", "#264f78", "#e6edf3",
            "#0d1117", "#ff7b72", "#3fb950", "#d29922",
            "#58a6ff", "#bc8cff", "#39d353", "#e6edf3",
            "#484f58", "#ffa657", "#3fb950", "#e3b341",
            "#58a6ff", "#bc8cff", "#56d364", "#ffffff"
        ),
        color_scheme!(
            "Nord", "#d8dee9", "#2e3440", "#d8dee9", "#434c5e", "#d8dee9",
            "#3b4252", "#bf616a", "#a3be8c", "#ebcb8b",
            "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
            "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b",
            "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4"
        ),
        color_scheme!(
            "Catppuccin Mocha", "#cdd6f4", "#1e1e2e", "#f5e0dc", "#45475a", "#cdd6f4",
            "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af",
            "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
            "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af",
            "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8"
        ),
        color_scheme!(
            "Tokyo Night", "#c0caf5", "#1a1b26", "#c0caf5", "#24283b", "#c0caf5",
            "#1a1b26", "#f7768e", "#9ece6a", "#e0af68",
            "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5",
            "#414868", "#f7768e", "#9ece6a", "#e0af68",
            "#7aa2f7", "#bb9af7", "#7dcfff", "#ffffff"
        ),
        color_scheme!(
            "Night Owl", "#d6deeb", "#011627", "#d6deeb", "#2c2e3e", "#d6deeb",
            "#011627", "#ff5874", "#2d9f6f", "#adad2b",
            "#637bbe", "#9984d9", "#21c7d9", "#d6deeb",
            "#323045", "#ff5874", "#2d9f6f", "#adad2b",
            "#637bbe", "#9984d9", "#21c7d9", "#ffffff"
        ),
        color_scheme!(
            "Gruvbox Dark", "#ebdbb2", "#282828", "#ebdbb2", "#3c3836", "#ebdbb2",
            "#282828", "#fb4934", "#98971a", "#d79921",
            "#83a598", "#d3869b", "#8ec07c", "#ebdbb2",
            "#928374", "#fb4934", "#98971a", "#d79921",
            "#83a598", "#d3869b", "#b8bb26", "#ffffff"
        ),
        color_scheme!(
            "Gruvbox Light", "#3c3836", "#fbf1c7", "#3c3836", "#d5c4a1", "#3c3836",
            "#fbf1c7", "#fb4934", "#98971a", "#d79921",
            "#83a598", "#d3869b", "#8ec07c", "#ebdbb2",
            "#928374", "#fb4934", "#98971a", "#d79921",
            "#83a598", "#d3869b", "#b8bb26", "#ffffff"
        ),
        color_scheme!(
            "One Light", "#383a42", "#fafafa", "#383a42", "#e5e5e6", "#383a42",
            "#fafafa", "#e45649", "#50a14f", "#c18401",
            "#4078f2", "#a626a4", "#0184bc", "#383a42",
            "#a0a1a7", "#e45649", "#50a14f", "#c18401",
            "#4078f2", "#a626a4", "#0184bc", "#ffffff"
        ),
        color_scheme!(
            "Rosé Pine", "#e4e1e9", "#232136", "#e4e1e9", "#44415a", "#e4e1e9",
            "#232136", "#eb6f92", "#9ccfd8", "#f6c177",
            "#9d7cd8", "#c4a7e7", "#31748f", "#e4e1e9",
            "#6e6a86", "#eb6f92", "#9ccfd8", "#f6c177",
            "#9d7cd8", "#c4a7e7", "#31748f", "#ffffff"
        ),
        color_scheme!(
            "Catppuccin Latte", "#4c4f69", "#eff1f5", "#4c4f69", "#ca9ee6", "#4c4f69",
            "#eff1f5", "#d20f39", "#40a02b", "#df8e1d",
            "#1e66f5", "#ea76cb", "#179299", "#acb0be",
            "#7c7f93", "#d20f39", "#40a02b", "#df8e1d",
            "#1e66f5", "#ea76cb", "#179299", "#bcc0cc"
        ),
        color_scheme!(
            "Everforest Dark", "#d3c6aa", "#2d383b", "#d3c6aa", "#3c4947", "#d3c6aa",
            "#2d383b", "#e67e80", "#a7c080", "#dbab79",
            "#7fbbb3", "#d699b6", "#94d8d0", "#d3c6aa",
            "#5c6b64", "#e67e80", "#a7c080", "#dbab79",
            "#7fbbb3", "#d699b6", "#94d8d0", "#ffffff"
        ),
        color_scheme!(
            "Kanagawa Dragon", "#c5c9c5", "#1f1f1f", "#c5c9c5", "#3a3a3a", "#c5c9c5",
            "#1f1f1f", "#f26e79", "#8bab87", "#d29066",
            "#7aa2f7", "#bb79c8", "#7cc9cf", "#c5c9c5",
            "#4a4a4a", "#f26e79", "#8bab87", "#d29066",
            "#7aa2f7", "#bb79c8", "#7cc9cf", "#ffffff"
        ),
        color_scheme!(
            "VS Code Dark+", "#d4d4d4", "#1e1e1e", "#d4d4d4", "#264f78", "#d4d4d4",
            "#1e1e1e", "#cd3131", "#4ec9b0", "#dcdcaa",
            "#569cd6", "#c586c0", "#4fc1ff", "#d4d4d4",
            "#5a5a5a", "#cd3131", "#4ec9b0", "#dcdcaa",
            "#569cd6", "#c586c0", "#4fc1ff", "#ffffff"
        ),
        color_scheme!(
            "VS Code Light+", "#333333", "#ffffff", "#333333", "#add6ff", "#333333",
            "#ffffff", "#16825d", "#4ec9b0", "#dcdcaa",
            "#0000ff", "#c586c0", "#4fc1ff", "#333333",
            "#6c6c6c", "#16825d", "#4ec9b0", "#dcdcaa",
            "#0000ff", "#c586c0", "#4fc1ff", "#000000"
        ),
        color_scheme!(
            "Iceberg", "#e6e6e6", "#1e1e2e", "#e6e6e6", "#262b44", "#e6e6e6",
            "#1e1e2e", "#d46142", "#a5c3ca", "#caac73",
            "#6891c3", "#c589ba", "#87abd5", "#e6e6e6",
            "#6e75a3", "#d46142", "#a5c3ca", "#caac73",
            "#6891c3", "#c589ba", "#87abd5", "#ffffff"
        ),
        color_scheme!(
            "Argonaut", "#f8f8f2", "#262b3c", "#f8f8f2", "#2e3340", "#f8f8f2",
            "#262b3c", "#f45a4c", "#7cc8a8", "#ffd866",
            "#5eb5e8", "#c688f0", "#76e1e1", "#f8f8f2",
            "#515767", "#f45a4c", "#7cc8a8", "#ffd866",
            "#5eb5e8", "#c688f0", "#76e1e1", "#ffffff"
        ),
        color_scheme!(
            "Dracula Plus", "#f8f8f2", "#282a36", "#f8f8f2", "#44475a", "#f8f8f2",
            "#282a36", "#ff5555", "#50fa7b", "#f1fa8c",
            "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
            "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5",
            "#d6acff", "#ff92df", "#a4ffff", "#ffffff"
        ),
        color_scheme!(
            "Hop Light", "#383a42", "#fafafa", "#383a42", "#e5e5e6", "#383a42",
            "#fafafa", "#e45649", "#50a14f", "#c18401",
            "#4078f2", "#a626a4", "#0184bc", "#383a42",
            "#a0a1a7", "#e45649", "#50a14f", "#c18401",
            "#4078f2", "#a626a4", "#0184bc", "#ffffff"
        ),
        color_scheme!(
            "Pencil Dark", "#f1f1f1", "#2d2d2d", "#f1f1f1", "#474747", "#f1f1f1",
            "#2d2d2d", "#ff6054", "#68c47a", "#ffc44a",
            "#5a80e0", "#e07fbd", "#3ab8d8", "#f1f1f1",
            "#6d6d6d", "#ff6054", "#68c47a", "#ffc44a",
            "#5a80e0", "#e07fbd", "#3ab8d8", "#ffffff"
        ),
        color_scheme!(
            "Pencil Light", "#383a42", "#fafafa", "#383a42", "#e5e5e6", "#383a42",
            "#fafafa", "#e45649", "#50a14f", "#c18401",
            "#4078f2", "#a626a4", "#0184bc", "#383a42",
            "#a0a1a7", "#e45649", "#50a14f", "#c18401",
            "#4078f2", "#a626a4", "#0184bc", "#ffffff"
        ),
        color_scheme!(
            "Ayu Dark", "#f8f8f2", "#0e1419", "#f8f8f2", "#1e1e1e", "#f8f8f2",
            "#0e1419", "#ff3333", "#a6cc70", "#ffcc66",
            "#36a3d9", "#f07178", "#95e6cb", "#f8f8f2",
            "#626a7c", "#ff3333", "#a6cc70", "#ffcc66",
            "#36a3d9", "#f07178", "#95e6cb", "#ffffff"
        ),
        color_scheme!(
            "Ayu Light", "#5c6670", "#fafafa", "#5c6670", "#e8e8e8", "#5c6670",
            "#fafafa", "#ff3333", "#86b72a", "#dda438",
            "#2a70b0", "#d960a0", "#39c0c3", "#5c6670",
            "#7a808a", "#ff3333", "#86b72a", "#dda438",
            "#2a70b0", "#d960a0", "#39c0c3", "#ffffff"
        ),
        color_scheme!(
            "Half Life", "#9a9a9a", "#1e1e1e", "#ffb000", "#3c3c3c", "#9a9a9a",
            "#1e1e1e", "#ff4d4d", "#7cfc00", "#ffff00",
            "#00a2ff", "#ff00ff", "#00ffff", "#9a9a9a",
            "#4a4a4a", "#ff4d4d", "#7cfc00", "#ffff00",
            "#00a2ff", "#ff00ff", "#00ffff", "#ffffff"
        ),
        color_scheme!(
            "C64", "#ffffff", "#4040e0", "#ffffff", "#8080c0", "#ffffff",
            "#4040e0", "#602020", "#60e060", "#ffff40",
            "#4080ff", "#ff80ff", "#80ffff", "#ffffff",
            "#808080", "#ff6060", "#60ff60", "#ffff80",
            "#4080ff", "#ff80ff", "#80ffff", "#ffffff"
        ),
        color_scheme!(
            "Tango Dark", "#ffffff", "#2e2e2e", "#ffffff", "#4a4a4a", "#ffffff",
            "#2e2e2e", "#cc0000", "#4e9a06", "#c8b800",
            "#3465a2", "#75507b", "#06989a", "#ffffff",
            "#888888", "#ef2929", "#8ae234", "#fce94f",
            "#729fc8", "#ad7fa8", "#34e2e2", "#ffffff"
        ),
        color_scheme!(
            "Tango Light", "#000000", "#ffffff", "#000000", "#babdb6", "#000000",
            "#ffffff", "#cc0000", "#4e9a06", "#c8b800",
            "#3465a2", "#75507b", "#06989a", "#000000",
            "#888888", "#ef2929", "#8ae234", "#fce94f",
            "#729fc8", "#ad7fa8", "#34e2e2", "#000000"
        ),
        color_scheme!(
            "Xterm", "#ffffff", "#000000", "#ffffff", "#000000", "#ffffff",
            "#000000", "#cd0000", "#00cd00", "#cdcd00",
            "#0000ee", "#cd00cd", "#00cdcd", "#ffffff",
            "#7f7f7f", "#ff0000", "#00ff00", "#ffff00",
            "#0000ff", "#ff00ff", "#00ffff", "#ffffff"
        ),
        color_scheme!(
            "Linux", "#cccccc", "#000000", "#ffffff", "#a3a3a3", "#cccccc",
            "#000000", "#cc0000", "#00cc00", "#cccc00",
            "#0000cc", "#cc00cc", "#00cccc", "#cccccc",
            "#808080", "#ff0000", "#00ff00", "#ffff00",
            "#0000ff", "#ff00ff", "#00ffff", "#ffffff"
        ),
    ]
}

pub struct ThemeService {
    schemes: HashMap<String, ColorScheme>,
}

impl ThemeService {
    pub fn new() -> Self {
        let mut schemes = HashMap::new();
        for scheme in builtin_color_schemes() {
            schemes.insert(scheme.name.clone(), scheme);
        }
        Self { schemes }
    }

    /// Load user themes from the given directory.
    /// Reads all .toml files and registers them as color schemes.
    pub fn load_user_themes(&mut self, themes_dir: &std::path::Path) {
        if !themes_dir.exists() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(themes_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "toml") {
                    match std::fs::read_to_string(&path) {
                        Ok(content) => {
                            match toml::from_str::<ColorScheme>(&content) {
                                Ok(scheme) => {
                                    log::info!("Loaded user theme: {}", scheme.name);
                                    self.register_scheme(scheme);
                                }
                                Err(e) => {
                                    log::warn!("Failed to parse theme {:?}: {}", path, e);
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to read theme {:?}: {}", path, e);
                        }
                    }
                }
            }
        }
    }

    pub fn get_scheme(&self, name: &str) -> Option<&ColorScheme> {
        self.schemes.get(name)
    }

    pub fn list_schemes(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.schemes.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    pub fn register_scheme(&mut self, scheme: ColorScheme) {
        self.schemes.insert(scheme.name.clone(), scheme);
    }

    pub fn default_scheme(&self) -> &ColorScheme {
        self.get_scheme("Catppuccin Mocha").unwrap()
    }
}

impl Default for ThemeService {
    fn default() -> Self {
        Self::new()
    }
}
