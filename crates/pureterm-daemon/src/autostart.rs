//! OS-level autostart for `puretermd` (GAP-5.1 M4).
//!
//! Installs/uninstalls a per-user background service so `puretermd` is running
//! at login — before the PureTerm GUI opens. This makes `pureterm ssh connect`
//! work from the CLI immediately and lets the GUI re-use the existing daemon
//! instead of spawn-on-demand.
//!
//! - macOS:  a LaunchAgent (`~/Library/LaunchAgents/com.cnb.puretermd.plist`)
//!           with `RunAtLoad` + `KeepAlive{SuccessfulExit:false}`. The latter
//!           keeps the daemon alive across crashes (non-zero exit) but does NOT
//!           respawn it when it exits 0 on a single-instance-lock collision
//!           (`runtime::run` returns `Ok` in that case) — so there's no respawn
//!           war with the GUI's spawn-on-demand.
//! - Linux:  a systemd user service (`~/.config/systemd/user/puretermd.service`)
//!           enabled with `systemctl --user enable --now`.
//!
//! The `puretermd` binary path is resolved beside the calling executable (covers
//! `cargo build` and the `.app` bundle), falling back to `puretermd` on `PATH`.

use std::io;
use std::path::PathBuf;

use crate::discovery;

/// LaunchAgent / systemd unit label.
pub const LABEL: &str = "com.cnb.puretermd";

/// Resolve the `puretermd` binary to embed in the service definition.
pub fn puretermd_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("puretermd");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from("puretermd")
}

// ── macOS (launchd) ────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::process::Command;

    fn plist_path() -> io::Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no HOME"))?;
        Ok(home.join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
    }

    fn domain_target() -> String {
        let uid = unsafe { libc::getuid() };
        format!("gui/{uid}")
    }

    pub fn is_installed() -> bool {
        matches!(plist_path(), Ok(p) if p.exists())
    }

    pub fn install() -> io::Result<()> {
        let puretermd = super::puretermd_path();
        let plist = plist_path()?;
        let log_path = discovery::data_dir().join("puretermd.log");
        if let Some(parent) = plist.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let plist_xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>StandardErrorPath</key>
    <string>{log}</string>
</dict>
</plist>
"#,
            exe = puretermd.display(),
            log = log_path.display()
        );
        std::fs::write(&plist, plist_xml)?;

        let domain = domain_target();
        // Reload: bootout (ignore "not loaded"), then bootstrap.
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("{domain}/{LABEL}")])
            .output();
        let out = Command::new("launchctl")
            .args(["bootstrap", &domain, &plist.to_string_lossy()])
            .output()?;
        if !out.status.success() {
            return Err(io::Error::other(format!(
                "launchctl bootstrap failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    }

    pub fn uninstall() -> io::Result<()> {
        let domain = domain_target();
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("{domain}/{LABEL}")])
            .output();
        if let Ok(plist) = plist_path() {
            let _ = std::fs::remove_file(&plist);
        }
        Ok(())
    }

    /// True if launchd currently has the job loaded + running.
    pub fn is_loaded() -> bool {
        let domain = domain_target();
        Command::new("launchctl")
            .args(["print", &format!("{domain}/{LABEL}")])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

// ── Linux (systemd user service) ───────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::process::Command;

    fn unit_path() -> io::Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no HOME"))?;
        Ok(home.join(".config/systemd/user/puretermd.service"))
    }

    pub fn is_installed() -> bool {
        matches!(unit_path(), Ok(p) if p.exists())
    }

    pub fn install() -> io::Result<()> {
        let puretermd = super::puretermd_path();
        let unit = unit_path()?;
        if let Some(parent) = unit.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let unit_text = format!(
            "[Unit]\nDescription=PureTerm persistent PTY daemon\n\n\
             [Service]\nExecStart={exe}\nRestart=on-failure\n\n\
             [Install]\nWantedBy=default.target\n",
            exe = puretermd.display()
        );
        std::fs::write(&unit, unit_text)?;
        Command::new("systemctl").args(["--user", "daemon-reload"]).status()?;
        let out = Command::new("systemctl")
            .args(["--user", "enable", "--now", "puretermd"])
            .output()?;
        if !out.status.success() {
            return Err(io::Error::other(format!(
                "systemctl enable failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    }

    pub fn uninstall() -> io::Result<()> {
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", "puretermd"])
            .output();
        if let Ok(unit) = unit_path() {
            let _ = std::fs::remove_file(&unit);
        }
        let _ = Command::new("systemctl").args(["--user", "daemon-reload"]).status();
        Ok(())
    }

    pub fn is_loaded() -> bool {
        Command::new("systemctl")
            .args(["--user", "is-active", "puretermd"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

#[cfg(target_os = "macos")]
pub use self::macos::{install, is_installed, is_loaded, uninstall};

#[cfg(target_os = "linux")]
pub use self::linux::{install, is_installed, is_loaded, uninstall};

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("puretermd autostart is only implemented for macOS and Linux");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puretermd_path_is_nonempty() {
        let p = puretermd_path();
        assert!(!p.as_os_str().is_empty());
    }
}
