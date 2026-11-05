//! `puretermd` 的 OS 级自启(GAP-5.1 M4)。
//!
//! 安装/卸载一个每用户后台服务,使 `puretermd` 在登录时——
//! PureTerm GUI 打开之前——就已运行。这让 `pureterm ssh connect`
//! 从 CLI 立即可用,GUI 也能复用既有守护进程而非按需拉起。
//! 
//!
//! - macOS:用 LaunchAgent(`~/Library/LaunchAgents/com.cnb.puretermd.plist`)
//!          带 `RunAtLoad` + `KeepAlive{SuccessfulExit:false}`。后者
//!          使守护进程跨崩溃(非零退出)存活,但在单实例锁冲突
//!          导致的 0 退出时不重启(此时 `runtime::run` 返回 `Ok`)
//!          ——因此不会与 GUI 的按需拉起打重启战。
//!          
//! - Linux:systemd 用户服务(`~/.config/systemd/user/puretermd.service`)
//!          以 `systemctl --user enable --now` 启用。
//!
//! `puretermd` 二进制路径解析为调用方可执行文件的同目录(覆盖
//! `cargo build` 与 `.app` 捆绑),回退到 `PATH` 上的 `puretermd`。

use std::io;
use std::path::PathBuf;

use crate::discovery;

/// LaunchAgent / systemd 单元标签。
pub const LABEL: &str = "com.cnb.puretermd";

/// 解析要写入服务定义的 `puretermd` 二进制路径。
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

// ── macOS 的 launchd ────────────────────────────────────────────────────────

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
        // 重载:先 bootout(忽略“未加载”),再 bootstrap。
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

    /// launchd 当前已加载并运行该任务时为真。
    pub fn is_loaded() -> bool {
        let domain = domain_target();
        Command::new("launchctl")
            .args(["print", &format!("{domain}/{LABEL}")])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

// ── Linux(systemd 用户服务) ────────────────────────────────────────────────────

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

/// Windows 上的自启动桩。
///
/// 守护进程本身在 Windows 上是 Unsupported(见 `runtime_windows.rs`),
/// 因此没有可注册的自启动项。这里给出完整签名而非
/// `compile_error!`,使 `cargo build --workspace` 能在 Windows 通过;
/// `install` / `uninstall` 明确返回 Unsupported,`is_installed` /
/// `is_loaded` 恒为 false。
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod windows_stub {
    use std::io;

    pub fn is_installed() -> bool {
        false
    }

    pub fn is_loaded() -> bool {
        false
    }

    pub fn install() -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "puretermd autostart is not supported on Windows",
        ))
    }

    pub fn uninstall() -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "puretermd autostart is not supported on Windows",
        ))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub use self::windows_stub::{install, is_installed, is_loaded, uninstall};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puretermd_path_is_nonempty() {
        let p = puretermd_path();
        assert!(!p.as_os_str().is_empty());
    }
}
