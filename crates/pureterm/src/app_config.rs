use std::path::{Path, PathBuf};

use pureterm_settings::config_io;

use crate::keymap::{CustomCommand, RawBinding};
use crate::ssh::config::SshProfile;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub font_family: String,
    pub font_size: f32,
    pub background_color: [f32; 4],
    pub foreground_color: [f32; 4],
    pub opacity: f32,
    pub locale: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            font_family: "monospace".to_string(),
            font_size: 14.0,
            background_color: [0.11, 0.11, 0.12, 1.0],
            foreground_color: [0.83, 0.84, 0.81, 1.0],
            opacity: 1.0,
            locale: "en".to_string(),
        }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Self {
        let mut config = Self::default();

        let doc = match config_io::load_document(path) {
            Ok(doc) => doc,
            Err(_) => return config,
        };

        if let Some(family) = config_io::get_string(&doc, &["font", "normal"], "family") {
            if !family.is_empty() {
                config.font_family = family;
            }
        }

        if let Some(size) = config_io::get_float(&doc, &["font"], "size") {
            if size > 0.0 {
                config.font_size = size as f32;
            }
        }

        if let Some(bg) = config_io::get_string(&doc, &["colors", "primary"], "background") {
            if let Some(color) = parse_hex_color(&bg) {
                config.background_color = color;
            }
        }

        if let Some(fg) = config_io::get_string(&doc, &["colors", "primary"], "foreground") {
            if let Some(color) = parse_hex_color(&fg) {
                config.foreground_color = color;
            }
        }

        if let Some(opacity) = config_io::get_float(&doc, &["window"], "opacity") {
            config.opacity = opacity.clamp(0.0, 1.0) as f32;
        }

        if let Some(locale) = config_io::get_string(&doc, &["pureterm"], "locale") {
            if !locale.is_empty() {
                config.locale = locale;
            }
        }

        config
    }

    pub fn config_path() -> PathBuf {
        config_io::find_config_path().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".config/pureterm/pureterm.toml")
        })
    }

    /// Load the user's `[[ssh.profiles]]` array from the toml config.
    ///
    /// The config is structured as:
    /// ```toml
    /// [[ssh.profiles]]
    /// name = "loopback"
    /// host = "127.0.0.1"
    /// user = "admin"
    /// port = 22
    /// key_path = "~/.ssh/id_ed25519"
    /// ```
    ///
    /// This is a separate pass from `AppConfig::load` because the
    /// `ssh` table is optional and we want to keep this function
    /// failure-tolerant: a missing or malformed `[[ssh.profiles]]`
    /// block returns an empty Vec, never propagates an error up.
    pub fn load_ssh_profiles(path: &Path) -> Vec<SshProfile> {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("load_ssh_profiles: cannot read {:?}: {}", path, e);
                return Vec::new();
            },
        };
        // Parse the whole file as a generic toml::Value first so we
        // can pick out the optional `[ssh]` table and its `profiles`
        // array. Using `toml::Value` (not toml_edit) is fine here:
        // we're not modifying the document, just reading.
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("load_ssh_profiles: parse {:?} failed: {}", path, e);
                return Vec::new();
            },
        };
        // Walk ssh.profiles. If the table is missing, that's normal
        // (user hasn't added any profiles yet).
        let Some(profiles_value) = value
            .get("ssh")
            .and_then(|s| s.get("profiles"))
        else {
            return Vec::new();
        };
        match profiles_value.clone().try_into::<Vec<SshProfile>>() {
            Ok(profiles) => {
                log::info!("load_ssh_profiles: loaded {} profiles", profiles.len());
                profiles
            },
            Err(e) => {
                log::warn!(
                    "load_ssh_profiles: failed to deserialize [[ssh.profiles]]: {}",
                    e,
                );
                Vec::new()
            },
        }
    }

    /// Load `[[keyboard.bindings]]` from the TOML config.
    ///
    /// Each entry has `key`, `mods` (optional), and `action` fields in
    /// Alacritty-compatible format. Returns raw strings for the keymap
    /// module to parse into typed `Binding` structs.
    ///
    /// Failure-tolerant: missing or malformed bindings are silently
    /// skipped; an empty Vec is returned if the table is absent.
    pub fn load_keybindings(path: &Path) -> Vec<RawBinding> {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("load_keybindings: cannot read {:?}: {}", path, e);
                return Vec::new();
            },
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("load_keybindings: parse {:?} failed: {}", path, e);
                return Vec::new();
            },
        };
        let Some(bindings_value) = value
            .get("keyboard")
            .and_then(|k| k.get("bindings"))
            .and_then(|b| b.as_array())
        else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for entry in bindings_value {
            let table = match entry.as_table() {
                Some(t) => t,
                None => continue,
            };
            let key = match table.get("key").and_then(|v| v.as_str()) {
                Some(k) => k.to_string(),
                None => continue,
            };
            let mods = table
                .get("mods")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let action = match table.get("action").and_then(|v| v.as_str()) {
                Some(a) => a.to_string(),
                None => continue,
            };
            result.push(RawBinding { key, mods, action });
        }
        if !result.is_empty() {
            log::info!("load_keybindings: loaded {} bindings", result.len());
        }
        result
    }

    /// GAP-6.4: Load the user's `[[commands]]` array from the toml config.
    ///
    /// Each entry is `{ id, title, command, key?, mods? }`:
    /// ```toml
    /// [[commands]]
    /// id = "deploy.staging"
    /// title = "Deploy to Staging"
    /// key = "Cmd+Shift+D"
    /// command = "ssh staging 'cd app && ./deploy.sh'"
    /// ```
    ///
    /// Failure-tolerant, mirroring `load_keybindings`: a missing file or
    /// `[[commands]]` table returns an empty Vec; entries missing required
    /// fields are skipped (with a `warn`). Duplicate `id`s collapse to the
    /// last definition (later wins).
    pub fn load_custom_commands(path: &Path) -> Vec<CustomCommand> {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("load_custom_commands: cannot read {:?}: {}", path, e);
                return Vec::new();
            },
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("load_custom_commands: parse {:?} failed: {}", path, e);
                return Vec::new();
            },
        };
        let Some(arr) = value.get("commands").and_then(|v| v.as_array()) else {
            return Vec::new();
        };
        let mut result: Vec<CustomCommand> = Vec::new();
        for entry in arr {
            let table = match entry.as_table() {
                Some(t) => t,
                None => continue,
            };
            let id = match table.get("id").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!("load_custom_commands: entry missing 'id', skipped");
                    continue;
                },
            };
            let title = match table.get("title").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_custom_commands: entry '{}' missing 'title', skipped",
                        id
                    );
                    continue;
                },
            };
            let command = match table.get("command").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_custom_commands: entry '{}' missing 'command', skipped",
                        id
                    );
                    continue;
                },
            };
            let key = table
                .get("key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let mods = table
                .get("mods")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let cmd = CustomCommand {
                id,
                title,
                command,
                key,
                mods,
            };
            // Dedup by id: later definition overrides an earlier one.
            if let Some(existing) = result.iter_mut().find(|e| e.id == cmd.id) {
                log::warn!(
                    "load_custom_commands: duplicate id '{}' — later entry overrides",
                    cmd.id
                );
                *existing = cmd;
            } else {
                result.push(cmd);
            }
        }
        if !result.is_empty() {
            log::info!("load_custom_commands: loaded {} commands", result.len());
        }
        result
    }
}

fn parse_hex_color(hex: &str) -> Option<[f32; 4]> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::CustomCommand;

    /// Write `contents` to a uniquely-named temp file and return its path.
    fn write_temp_toml(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn test_load_custom_commands_basic() {
        let toml = r#"
[[commands]]
id = "deploy"
title = "Deploy"
command = "make deploy"
key = "Cmd+Shift+D"

[[commands]]
id = "test"
title = "Run Tests"
command = "make test"
"#;
        let path = write_temp_toml("pureterm_test_cmds_basic.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].id, "deploy");
        assert_eq!(cmds[0].title, "Deploy");
        assert_eq!(cmds[0].command, "make deploy");
        assert_eq!(cmds[0].key.as_deref(), Some("Cmd+Shift+D"));
        assert!(cmds[0].mods.is_none());
        assert_eq!(cmds[1].id, "test");
        assert!(cmds[1].key.is_none(), "second command has no key");
    }

    #[test]
    fn test_load_custom_commands_dedup_later_wins() {
        let toml = r#"
[[commands]]
id = "dup"
title = "First"
command = "echo first"

[[commands]]
id = "dup"
title = "Second"
command = "echo second"
"#;
        let path = write_temp_toml("pureterm_test_cmds_dup.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cmds.len(), 1, "duplicate id should collapse to one");
        assert_eq!(cmds[0].title, "Second", "later entry should win");
        assert_eq!(cmds[0].command, "echo second");
    }

    #[test]
    fn test_load_custom_commands_missing_file() {
        let cmds = AppConfig::load_custom_commands(Path::new(
            "/nonexistent/pureterm_does_not_exist_3f9a.toml",
        ));
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_load_custom_commands_skips_incomplete_entry() {
        let toml = r#"
[[commands]]
id = "good"
title = "Good"
command = "echo ok"

[[commands]]
id = "bad"
title = "Missing Command"
"#;
        let path = write_temp_toml("pureterm_test_cmds_bad.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cmds.len(), 1, "incomplete entry should be skipped");
        assert_eq!(cmds[0].id, "good");
    }

    #[test]
    fn test_load_custom_commands_explicit_mods() {
        let toml = r#"
[[commands]]
id = "build"
title = "Build"
command = "make build"
key = "D"
mods = "Alt"
"#;
        let path = write_temp_toml("pureterm_test_cmds_mods.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].key.as_deref(), Some("D"));
        assert_eq!(cmds[0].mods.as_deref(), Some("Alt"));
    }

    /// Cross-module: a `[[commands]]` TOML entry must round-trip into a
    /// working keymap binding (config → load → register → resolve).
    #[test]
    fn test_custom_commands_config_to_keymap_binding() {
        use crate::keymap::{Action, Keymap, ResolveResult};
        use iced::keyboard::{Key, Modifiers};
        use std::time::Instant;

        let toml = r#"
[[commands]]
id = "deploy"
title = "Deploy"
command = "make deploy"
key = "Ctrl+B"
"#;
        let path = write_temp_toml("pureterm_test_cmds_integration.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);

        let mut km = Keymap::from_defaults();
        km.register_custom_commands(&cmds);
        let r = km.resolve(
            &Key::Character("b".into()),
            Modifiers::CTRL,
            Instant::now(),
        );
        assert_eq!(
            r,
            ResolveResult::Action(Action::CustomCommand("deploy".into()))
        );
    }
}
