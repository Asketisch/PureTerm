//! GAP-61: Print terminal pane — NSPrintOperation scaffold.

/// `[print]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintSupportConfig {
    pub enabled: bool,
    pub include_header: bool,
    pub include_footer: bool,
}

impl Default for PrintSupportConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            include_header: true,
            include_footer: true,
        }
    }
}

/// Format pane text for printing with optional header/footer.
pub fn format_for_print(cfg: &PrintSupportConfig, title: &str, body: &str) -> String {
    if !cfg.enabled {
        return body.to_string();
    }
    let mut out = String::new();
    if cfg.include_header {
        out.push_str(&format!("PureTerm — {title}\n"));
        out.push_str(&"─".repeat(40));
        out.push('\n');
    }
    out.push_str(body);
    if cfg.include_footer {
        out.push_str("\n\n— printed by PureTerm —");
    }
    out
}

/// Trigger print dialog. MVP: write to temp file and `lpr` on Unix/macOS.
pub fn print_pane(cfg: &PrintSupportConfig, title: &str, body: &str) -> Result<(), String> {
    if !cfg.enabled {
        return Err("print disabled".into());
    }
    let formatted = format_for_print(cfg, title, body);
    let tmp = std::env::temp_dir().join(format!("pureterm-print-{}.txt", std::process::id()));
    std::fs::write(&tmp, &formatted).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        let status = std::process::Command::new("lpr")
            .arg(&tmp)
            .status()
            .map_err(|e| format!("lpr failed: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("lpr exited with {status}"))
        }
    }
    #[cfg(not(unix))]
    {
        Err("print requires Unix/macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_when_enabled() {
        let cfg = PrintSupportConfig {
            enabled: true,
            ..Default::default()
        };
        let s = format_for_print(&cfg, "Tab 1", "ls");
        assert!(s.contains("Tab 1"));
        assert!(s.contains("ls"));
    }
}
