//! Stable discovery paths + single-instance lock for `puretermd`.
//!
//! Unlike the GUI IPC socket (pid-suffixed, dies with the GUI), the daemon
//! socket path is **fixed** so a restarted GUI can find the same daemon and
//! re-attach to surviving sessions.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

/// Per-user runtime dir for the daemon socket. Stable across GUI restarts.
fn runtime_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        // TMPDIR is per-user on macOS (set by launchd).
        std::env::temp_dir()
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir())
    }
}

/// Fixed (non-pid-suffixed) daemon control socket path.
pub fn socket_path() -> PathBuf {
    runtime_dir().join("puretermd.sock")
}

/// Persistent data dir for the daemon: manifests, session rings, etc.
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("pureterm")
        .join("daemon")
}

/// Per-session data socket directory.
pub fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

/// GAP-5.2: persisted tunnel configs (config-only restore across daemon
/// restarts).
pub fn tunnels_file() -> PathBuf {
    data_dir().join("tunnels.json")
}

/// GAP-5.2: TOFU known_hosts store (isolated from the user's `~/.ssh/known_hosts`).
pub fn known_hosts_file() -> PathBuf {
    data_dir().join("known_hosts")
}

/// Path of the single-instance lock file (flock'd).
pub fn lock_path() -> PathBuf {
    data_dir().join("puretermd.lock")
}

/// Ensure the data + sessions directories exist.
pub fn ensure_dirs() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(data_dir())?;
    std::fs::create_dir_all(sessions_dir())?;
    // GAP-5.1 M5: owner-only — other local users must not traverse into the
    // session data sockets (which carry raw PTY bytes / typed input).
    let _ = std::fs::set_permissions(data_dir(), std::fs::Permissions::from_mode(0o700));
    let _ = std::fs::set_permissions(sessions_dir(), std::fs::Permissions::from_mode(0o700));
    Ok(())
}

/// Restrict a socket file to owner-only (0600). Best-effort (the listener is
/// already bound; this tightens the on-disk inode mode).
pub fn restrict_socket(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

/// Try to acquire an exclusive non-blocking `flock` on the daemon lock file.
///
/// Returns `Ok(Some(file))` if this process is the unique daemon instance —
/// the caller must hold the `File` for the daemon's lifetime. Returns
/// `Ok(None)` if another daemon already holds the lock.
pub fn acquire_instance_lock() -> io::Result<Option<File>> {
    ensure_dirs()?;
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(lock_path())?;
    // LOCK_EX | LOCK_NB: fails immediately if another instance holds it.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_is_stable_and_has_no_pid() {
        let p = socket_path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        assert_eq!(name, "puretermd.sock", "daemon socket must be a fixed name");
        // Calling twice yields the same path (stability across restarts).
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
        // Use the real (per-user) dir; creating it is idempotent and safe.
        ensure_dirs().expect("ensure_dirs");
        assert!(data_dir().is_dir());
        assert!(sessions_dir().is_dir());
    }

    #[test]
    fn instance_lock_is_exclusive() {
        let first = acquire_instance_lock().expect("first lock");
        assert!(first.is_some(), "first acquirer should get the lock");
        let second = acquire_instance_lock().expect("second lock attempt");
        assert!(second.is_none(), "second acquirer must be told another instance runs");
        // Dropping `first` releases the flock; a third attempt re-acquires.
        drop(first);
        let third = acquire_instance_lock().expect("third lock attempt");
        assert!(third.is_some(), "lock must be reusable after release");
    }
}
