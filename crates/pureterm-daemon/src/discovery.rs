//! `puretermd` 的稳定发现路径 + 单实例锁。
//!
//! 与 GUI IPC socket(带 pid 后缀、随 GUI 消亡)不同,守护进程
//! socket 路径是**固定的**,因此重启后的 GUI 能找到同一守护进程,
//! 重新挂接存活的会话。

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

/// 守护进程 socket 的每用户运行时目录。跨 GUI 重启保持稳定。
fn runtime_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        // macOS 上 TMPDIR 是每用户的(由 launchd 设置)。
        std::env::temp_dir()
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir())
    }
}

/// 固定(不带 pid 后缀)的守护进程控制 socket 路径。
pub fn socket_path() -> PathBuf {
    runtime_dir().join("puretermd.sock")
}

/// 守护进程的持久数据目录:清单、会话环等。
///
/// 测试可经 `PURETERM_DAEMON_DATA_DIR` 覆盖,使 `cargo test` 与
/// 运行中的 `puretermd` 实例(锁文件 / 会话 socket)保持隔离。
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("PURETERM_DAEMON_DATA_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("pureterm")
        .join("daemon")
}

/// 每会话数据 socket 目录。
pub fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

/// GAP-5.2:持久化的隧道配置(守护进程重启后
/// 仅恢复配置)。
pub fn tunnels_file() -> PathBuf {
    data_dir().join("tunnels.json")
}

/// GAP-5.2:TOFU known_hosts 存储(与用户的 `~/.ssh/known_hosts` 隔离)。
pub fn known_hosts_file() -> PathBuf {
    data_dir().join("known_hosts")
}

/// 单实例锁文件(flock)的路径。
pub fn lock_path() -> PathBuf {
    data_dir().join("puretermd.lock")
}

/// 确保数据与会话目录存在。
pub fn ensure_dirs() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(data_dir())?;
    std::fs::create_dir_all(sessions_dir())?;
    // GAP-5.1 M5:仅属主可访问——其它本地用户绝不能遍历进入
    // 会话数据 socket(其中承载原始 PTY 字节 / 已录入的输入)。
    let _ = std::fs::set_permissions(data_dir(), std::fs::Permissions::from_mode(0o700));
    let _ = std::fs::set_permissions(sessions_dir(), std::fs::Permissions::from_mode(0o700));
    Ok(())
}

/// 把 socket 文件限制为仅属主(0600)。尽力而为(监听器已
/// 绑定;此处只是收紧磁盘 inode 权限位)。
pub fn restrict_socket(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

/// 尝试对守护进程锁文件做非阻塞排他 `flock`。
///
/// 本进程是唯一守护进程实例时返回 `Ok(Some(file))`——
/// 调用方必须在守护进程生命周期内持有该 `File`。
/// 已有其它守护进程持锁时返回 `Ok(None)`。
pub fn acquire_instance_lock() -> io::Result<Option<File>> {
    acquire_instance_lock_at(&lock_path())
}

/// 同 [`acquire_instance_lock`],但作用于任意路径(单元测试)。
pub fn acquire_instance_lock_at(path: &std::path::Path) -> io::Result<Option<File>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    // LOCK_EX | LOCK_NB:其它实例持锁时立即失败。
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        Ok(Some(file))
    } else {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
            Ok(None)
        } else {
            Err(err)
        }
    }
}

use std::os::unix::io::AsRawFd;

/// RAII 守卫:把守护进程数据路径导入隔离的临时目录(单元测试)。
#[cfg(test)]
pub(crate) struct IsolatedDataDir {
    _dir: tempfile::TempDir,
    prev: Option<String>,
}

#[cfg(test)]
impl IsolatedDataDir {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let prev = std::env::var("PURETERM_DAEMON_DATA_DIR").ok();
        std::env::set_var("PURETERM_DAEMON_DATA_DIR", dir.path());
        Self { _dir: dir, prev }
    }
}

#[cfg(test)]
impl Drop for IsolatedDataDir {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var("PURETERM_DAEMON_DATA_DIR", v),
            None => std::env::remove_var("PURETERM_DAEMON_DATA_DIR"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_is_stable_and_has_no_pid() {
        let p = socket_path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        assert_eq!(name, "puretermd.sock", "daemon socket must be a fixed name");
        // 两次调用得到同一路径(跨重启稳定)。
        assert_eq!(p, socket_path());
    }

    #[test]
    fn data_and_sessions_dirs_under_pureterm() {
        let d = data_dir();
        assert!(d.ends_with("pureterm") || d.to_string_lossy().contains("pureterm"));
        assert!(sessions_dir().starts_with(&d));
        assert!(lock_path().starts_with(&d));
    }

    #[test]
    fn ensure_dirs_creates_paths() {
        // 使用真实的(每用户)目录;创建幂等且安全。
        ensure_dirs().expect("ensure_dirs");
        assert!(data_dir().is_dir());
        assert!(sessions_dir().is_dir());
    }

    #[test]
    fn instance_lock_is_exclusive() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let lock = tmp.path().join("puretermd.lock");
        let first = acquire_instance_lock_at(&lock).expect("first lock");
        assert!(first.is_some(), "first acquirer should get the lock");
        let second = acquire_instance_lock_at(&lock).expect("second lock attempt");
        assert!(second.is_none(), "second acquirer must be told another instance runs");
        // 丢弃 `first` 释放 flock;第三次尝试可重新获取。
        drop(first);
        let third = acquire_instance_lock_at(&lock).expect("third lock attempt");
        assert!(third.is_some(), "lock must be reusable after release");
    }

    #[test]
    fn data_dir_respects_test_override() {
        let _iso = IsolatedDataDir::new();
        let d = data_dir();
        assert!(d.starts_with(std::env::temp_dir()));
        assert!(lock_path().starts_with(&d));
    }
}
