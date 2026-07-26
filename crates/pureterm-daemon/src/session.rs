//! Session management + per-session PTY mirror pump.
//!
//! Each session owns a real PTY (spawned via `alacritty_terminal::tty`) and a
//! per-session Unix data listener. A single polling pump thread:
//!   - reads PTY output  -> frames `OUTPUT` to the attached GUI,
//!   - reads GUI `INPUT` frames  -> writes them to the PTY,
//!   - reads GUI `RESIZE` frames -> does a real `TIOCSWINSZ` on the PTY,
//!   - detects child exit (master read EOF / EIO) -> frames `EXIT`.
//!
//! Because the daemon owns the real PTY, the child's parent is the daemon and
//! the master fd survives GUI close — the core "关 GUI 不丢会话" win. The GUI
//! side (`DaemonPty`) is a thin mirror; see the M0 spike for proof the
//! `EventedPty` contract holds over a socketpair.
//!
//! 本模块按职责拆分为子模块 (见 `session/`):
//! - [`manager`]     — `SessionManager` 的实现 (生命周期 / 文件浏览 / CPU 采样)。
//! - [`pump`]        — pump 线程:轮询 master fd + data socket,帧化 IO。
//! - [`subscribers`] — 多 attach 订阅者管理 + 背压。
//! - [`io_helpers`]  — 会话 id 生成、master fd 非阻塞读写、PTY resize 等原语。
//! - [`tests`]       — 单测。
//!
//! 根文件只保留:模块文档、poller 兴趣键 / 帧大小常量、`PumpablePty` 抽象、
//! 三个 pub 类型 (`SessionHandle` / `SessionManager` / `SessionInfo`)。

use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use alacritty_terminal::tty;
use pureterm_ssh::SshPty;

use crate::remote_fs::SshSessionMeta;

mod io_helpers;
mod manager;
mod pump;
mod subscribers;
#[cfg(test)]
mod tests;

/// Interest keys for the per-session poller.
pub(super) const LISTENER_KEY: usize = 0;
pub(super) const MASTER_KEY: usize = 1;
pub(super) const SUBSCRIBER_KEY: usize = 2;

/// Max bytes of a single OUTPUT/SCROLLBACK frame payload.
pub(super) const OUTPUT_CHUNK: usize = 4096;

/// A PTY the daemon pumps, abstracted to its master `File` so the same pump
/// loop serves both local (`tty::Pty`) and SSH (`SshPty`) sessions. The daemon
/// works on the raw master fd (read/`ioctl` resize) and detects child exit via
/// master read EOF — it does not use the `EventedPty` trait impls.
pub(super) trait PumpablePty: Send {
    fn master(&self) -> &File;
}

impl PumpablePty for tty::Pty {
    fn master(&self) -> &File {
        self.file()
    }
}

impl PumpablePty for SshPty {
    fn master(&self) -> &File {
        self.master_file()
    }
}

/// A live session owned by the daemon.
pub struct SessionHandle {
    pub id: String,
    pub kind: String,
    pub child_pid: u32,
    pub data_socket_path: PathBuf,
    /// GAP-7.5: agent type when this session backs an AI agent (e.g.
    /// `claude-code`, `codex`). `None` for plain shells / SSH. Drives
    /// hibernation eligibility (GAP-7.3) and lets the GUI re-persist the
    /// session id after a cold resume.
    pub agent_kind: Option<String>,
    /// GAP-7.3: last time the PTY produced output. Written by the pump thread
    /// on every `MasterRead::Data`; read by `list` to compute `pty_idle_secs`.
    pub last_output_at: Arc<Mutex<Instant>>,
    /// GAP-7.3: last time the child was seen consuming CPU. Updated by the CPU
    /// sample tick (`SessionManager::sample_cpu_pass`); read by `list` to
    /// compute `cpu_idle_secs`.
    pub last_cpu_active_at: Arc<Mutex<Instant>>,
    /// GAP-7.3: true while the child is SIGSTOPped (hibernated). Set/cleared by
    /// `hibernate`/`resume` (M3); idle detection + CPU sampling skip these.
    pub hibernated: Arc<AtomicBool>,
    /// GAP-6.6: pipe-pane tap. `Some(tx)` ⇒ the pump feeds PTY output to a
    /// tokio task that writes it to a spawned command's stdin; None ⇒ no pipe.
    pub pipe_feeder: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>>,
    /// GAP-43: ControlMaster socket + host for SSH file browse (`remote.listdir`).
    pub ssh_meta: Option<SshSessionMeta>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) join: Option<JoinHandle<()>>,
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        let _ = std::fs::remove_file(&self.data_socket_path);
    }
}

/// Central registry of live sessions, shared between the control plane and
/// the pump threads.
#[derive(Clone, Default)]
pub struct SessionManager {
    pub(super) sessions: Arc<Mutex<std::collections::HashMap<String, Arc<Mutex<SessionHandle>>>>>,
    /// GAP-7.3: last CPU sample per session id, used by `sample_cpu_pass` to
    /// detect CPU-time advancement (the dual idle condition for hibernation).
    pub(super) cpu_samples:
        Arc<Mutex<std::collections::HashMap<String, crate::idle_probe::SampledCpuTimes>>>,
}

/// Serializable session descriptor (for `session.list`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub kind: String,
    pub child_pid: u32,
    pub data_path: String,
    /// GAP-7.5: agent type (`claude-code`/`codex`) when this session backs an
    /// agent; `None` for plain shells. Drives hibernation eligibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_kind: Option<String>,
    /// GAP-7.3: seconds since the PTY last produced output. `None` if unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pty_idle_secs: Option<u64>,
    /// GAP-7.3: seconds since the child last consumed CPU. `None` if unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_idle_secs: Option<u64>,
    /// GAP-7.3: true while the child is SIGSTOPped (hibernated).
    pub hibernated: bool,
}
