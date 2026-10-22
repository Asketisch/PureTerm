//! 会话管理 + 每会话的 PTY 镜像泵。
//!
//! 每个会话持有一个真实 PTY(经 `alacritty_terminal::tty` 拉起)与
//! 一个每会话的 Unix 数据监听器。单个轮询泵线程:
//!   - 读 PTY 输出 → 打 `OUTPUT` 帧给已挂接的 GUI,
//!   - 读 GUI `INPUT` 帧 → 写入 PTY,
//!   - 读 GUI `RESIZE` 帧 → 对 PTY 做真正的 `TIOCSWINSZ`,
//!   - 检测子进程退出(master 读到 EOF / EIO)→ 打 `EXIT` 帧。
//!
//! 因为守护进程持有真实 PTY,子进程的父进程是守护进程,
//! the master fd survives GUI close — the core "关 GUI 不丢会话" win. The GUI
//! 侧(`DaemonPty`)只是瘦镜像;M0 spike 已证明
//! `EventedPty` 契约在 socketpair 上依然成立。
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

/// 每会话轮询器的兴趣键。
pub(super) const LISTENER_KEY: usize = 0;
pub(super) const MASTER_KEY: usize = 1;
pub(super) const SUBSCRIBER_KEY: usize = 2;

/// 单个 OUTPUT/SCROLLBACK 帧载荷的字节上限。
pub(super) const OUTPUT_CHUNK: usize = 4096;

/// 守护进程泵送的 PTY,抽象为其 master `File`,使同一泵
/// 循环既服务本地(`tty::Pty`)也服务 SSH(`SshPty`)会话。守护进程
/// 工作在裸 master fd 上(read/`ioctl` resize),并经
/// master 读 EOF 检测子进程退出——不使用 `EventedPty` trait 实现。
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

/// 守护进程持有的一个存活会话。
pub struct SessionHandle {
    pub id: String,
    pub kind: String,
    pub child_pid: u32,
    pub data_socket_path: PathBuf,
    /// GAP-7.5:该会话支撑 AI agent(如 `claude-code`、
    /// `codex`)时的 agent 类型。普通 shell / SSH 为 `None`。驱动
    /// 休眠资格(GAP-7.3),并让 GUI 在冷恢复后重新持久化
    /// 会话 id。
    pub agent_kind: Option<String>,
    /// GAP-7.3:PTY 最近一次产出输出的时间。泵线程在每次
    /// `MasterRead::Data` 时写入;`list` 读取以计算 `pty_idle_secs`。
    pub last_output_at: Arc<Mutex<Instant>>,
    /// GAP-7.3:最近一次观察到子进程消耗 CPU 的时间。由 CPU
    /// 采样 tick(`SessionManager::sample_cpu_pass`)更新;`list` 读取
    /// 以计算 `cpu_idle_secs`。
    pub last_cpu_active_at: Arc<Mutex<Instant>>,
    /// GAP-7.3:子进程被 SIGSTOP(休眠)期间为真。由
    /// `hibernate`/`resume` 置位/清除(M3);空闲检测与 CPU 采样跳过这些。
    pub hibernated: Arc<AtomicBool>,
    /// GAP-6.6:pipe-pane 管道。`Some(tx)` ⇒ 泵把 PTY 输出喂给一个
    /// 写入拉起命令 stdin 的 tokio 任务;None ⇒ 无管道。
    pub pipe_feeder: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>>,
    /// GAP-43:SSH 文件浏览(`remote.listdir`)用的 ControlMaster socket 与主机。
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

/// 存活会话的中心注册表,由控制面与
/// 泵线程共享。
#[derive(Clone, Default)]
pub struct SessionManager {
    pub(super) sessions: Arc<Mutex<std::collections::HashMap<String, Arc<Mutex<SessionHandle>>>>>,
    /// GAP-7.3:每会话 id 的最近 CPU 采样,供 `sample_cpu_pass` 检测
    /// CPU 时间前进(休眠的双重空闲条件)。
    pub(super) cpu_samples:
        Arc<Mutex<std::collections::HashMap<String, crate::idle_probe::SampledCpuTimes>>>,
}

/// 可序列化的会话描述符(供 `session.list`)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    /// 会话 id。字段名与其余守护进程 RPC 一致(`session.create` /
    /// `session.attach` / `session.kill` 都用 `session_id`);早前这里
    /// 叫 `id`,同一套 API 里出现了两种叫法,调用方得同时认两个键。
    #[serde(rename = "session_id")]
    pub id: String,
    pub kind: String,
    pub child_pid: u32,
    pub data_path: String,
    /// GAP-7.5:该会话支撑 agent(`claude-code`/`codex`)时的
    /// agent 类型;普通 shell 为 `None`。驱动休眠资格。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_kind: Option<String>,
    /// GAP-7.3:PTY 最近一次产出输出以来的秒数。未知为 `None`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pty_idle_secs: Option<u64>,
    /// GAP-7.3:子进程最近一次消耗 CPU 以来的秒数。未知为 `None`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_idle_secs: Option<u64>,
    /// GAP-7.3:子进程被 SIGSTOP(休眠)期间为真。
    pub hibernated: bool,
}
