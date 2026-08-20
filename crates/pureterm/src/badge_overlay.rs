//! GAP-29:环境徽标覆盖层(PROD/DEV/STAGING)。

/// 徽标角落位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BadgePosition {
    TopLeft,
    TopRight,
    #[default]
    BottomRight,
    BottomLeft,
}

impl BadgePosition {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "top-left" | "topleft" => Self::TopLeft,
            "top-right" | "topright" => Self::TopRight,
            "bottom-left" | "bottomleft" => Self::BottomLeft,
            _ => Self::BottomRight,
        }
    }
}

/// `[badge]` 配置。
#[derive(Clone, Debug, PartialEq)]
pub struct BadgeOverlayConfig {
    pub enabled: bool,
    pub text: String,
    pub color: String,
    pub position: BadgePosition,
    /// 0.0–1.0
    pub opacity: f32,
}

impl Default for BadgeOverlayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            text: String::new(),
            color: "#FF3B30".into(),
            position: BadgePosition::BottomRight,
            opacity: 0.75,
        }
    }
}

impl BadgeOverlayConfig {
    /// 按配置档覆盖:非空配置档徽标优先于全局文本。
    pub fn effective_text<'a>(&'a self, profile_badge: Option<&'a str>) -> Option<&'a str> {
        if !self.enabled {
            return None;
        }
        if let Some(p) = profile_badge.filter(|s| !s.is_empty()) {
            return Some(p);
        }
        let t = self.text.trim();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_overrides_global() {
        let cfg = BadgeOverlayConfig {
            enabled: true,
            text: "DEV".into(),
            ..Default::default()
        };
        assert_eq!(cfg.effective_text(Some("PROD")), Some("PROD"));
        assert_eq!(cfg.effective_text(None), Some("DEV"));
    }
}
