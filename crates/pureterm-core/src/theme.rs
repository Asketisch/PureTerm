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
