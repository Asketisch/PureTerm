//! GAP-5.6：SSH 重连/断开状态机（进程内子集）。
//!
//! 当一条已建立的 SSH 连接断开（`ssh` 子进程在已连接超过 5s 后退出，
//! 且退出不是用户发起的），[`ReconnectHub`] 会启动一个后台工作线程，
//! 以指数退避重新生成 ssh PTY。状态转换通过 tokio 通道 + 持久的
//! `iced::Subscription` 反馈给 iced 主循环，镜像 IPC 监听器桥接模式
//! （见 `iced_app.rs` 的 `ReconnectKey` / `reconnect_stream_builder`）。
//!
//! 这是 GAP-5.6 的进程内子集：没有独立守护进程，没有持久化。
//! [`SshState`] 枚举与 `SshCtx::reattach` 机制
//!
//! 模块布局:
//! - 根:常量 + 类型定义(SpawnDecision/SshFailureKind/SshState/SshStateSnapshot/
//!   ReconnectParams/ReconnectUpdate/CancelSignal/PaneSlot/ReconnectHub 等类型) +
//!   纯函数(classify/on_first_remote_byte)+ Default
//! - `hub`:`impl ReconnectHub` + `reconnect_worker` 线程主体

use std::collections::HashMap;
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::pty::SshPty;
use pureterm_ssh::SshForwards;

use super::session::ConnectionKey;
use crate::split_tree::PaneId;

/// 重连尝试之间的指数退避时间表。
///
/// 3s → 6s → 12s → 24s → 30s → 40s → 50s → 60s。走完最后一项后，
/// 工作线程放弃并报告 [`SshState::Failed`]。
pub const BACKOFF: [Duration; 8] = [
    Duration::from_secs(3),
    Duration::from_secs(6),
    Duration::from_secs(12),
    Duration::from_secs(24),
    Duration::from_secs(30),
    Duration::from_secs(40),
    Duration::from_secs(50),
    Duration::from_secs(60),
];

/// 工作线程在之后放弃的 1 基尝试序号（BUG-3）。工作线程在该次尝试上
/// 就退出退避循环，而不是睡满最后的 [`BACKOFF`] 档位——否则死主机
/// 会让用户在误导性的“60 秒后重试” [`ReconnectUpdate::Progress`] 上
/// 干等一整分钟，然后（早已决定的）[`ReconnectUpdate::Failed`] 才落地。
/// 必须等于 `BACKOFF.len()`，这样编辑时间表不会复活被浪费的最后一觉。
pub fn final_attempt() -> u8 {
    BACKOFF.len() as u8
}

/// 反敲打滚动窗口（GAP-5.6 后续）。一个“连接即死”的主机可以绕过
/// 单工作线程的 [`BACKOFF`] 上限：每次成功生成都会结束工作线程，因此
/// 每次掉线都会再次调用 [`ReconnectHub::spawn_reconnect`]，从
/// `BACKOFF[0]` 重新开始一个全新工作线程——以约最小退避永久重连并
/// 敲打主机（在 CGNAT 中继上观察到：稳定的约 10s 连接→掉线→重连
/// 循环）。作为替代，我们在该窗口内按面板统计重连*触发*次数；超过
/// [`MAX_FLAPS`] 后拒绝生成，并把面板标记为 `Failed(flapping)`。
/// 窗口自衰减，因此偶尔掉线的健康主机永远不会累积——无需驻留计时器。
pub const FLAP_WINDOW: Duration = Duration::from_secs(120);

/// 在 [`FLAP_WINDOW`] 内停止前允许的最大重连触发次数。在约 10s 的
/// 抖动周期下，真正抖动的宿主会在约 `MAX_FLAPS × 10s` 内被停止；
/// 健康主机几乎不会在两分钟内掉线这么多次。
pub const MAX_FLAPS: usize = 6;

/// 抖动上限触发时对外呈现的原因（状态栏 / 对话框 / 事件）。
pub const FLAP_REASON: &str =
    "connection kept dropping (flapping); reconnect manually when the host is stable";

/// [`ReconnectHub::spawn_reconnect`] 是否真的启动了一个工作线程。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnDecision {
    /// 重连工作线程已启动。
    Spawned,
    /// 拒绝：面板在 [`FLAP_WINDOW`] 内抖动超过 [`MAX_FLAPS`]。
    /// 调用方应把面板标记为 `Failed(flapping)`，而不是等待工作线程更新。
    Flapping,
}

/// Classified reason an SSH connection attempt failed. GAP-5.6 收尾:因为
/// spawn 走 OS `ssh` 二进制,Rust 侧只能从 ssh 的 stderr 文本推断失败类别。
/// 用分类结果给 [`SshState::Failed`] 带类型,让状态栏 / 事件 / 对话框能显示
/// "认证失败" 而非笼统的 "连接失败"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshFailureKind {
    /// `Permission denied` — 凭证错误 / 公钥未授权。
    Auth,
    /// `Connection refused` / `Connection timed out` / `Could not resolve hostname`
    /// / `Network is unreachable` —— 网络层不可达。
    Network,
    /// `Host key verification failed` / `REMOTE HOST IDENTIFICATION HAS CHANGED`
    /// —— 主机密钥不匹配 / 首次连接 TOFU 拒绝。
    HostKey,
    /// 未匹配任何已知 ssh stderr 模式。
    Unknown,
}

/// 把 ssh 进程的 stderr 文本归类成 [`SshFailureKind`]。匹配大小写敏感(ssh
/// 实际输出即这些固定字符串),对未知内容返回 [`SshFailureKind::Unknown`]。
pub fn classify_ssh_failure(stderr: &str) -> SshFailureKind {
    if stderr.contains("Permission denied") {
        SshFailureKind::Auth
    } else if stderr.contains("Connection refused")
        || stderr.contains("Connection timed out")
        || stderr.contains("Could not resolve hostname")
        || stderr.contains("Network is unreachable")
    {
        SshFailureKind::Network
    } else if stderr.contains("Host key verification failed")
        || stderr.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
    {
        SshFailureKind::HostKey
    } else {
        SshFailureKind::Unknown
    }
}

impl SshFailureKind {
    /// 供事件载荷 / IPC 使用的稳定小写别名。
    pub fn as_str(self) -> &'static str {
        match self {
            SshFailureKind::Auth => "auth",
            SshFailureKind::Network => "network",
            SshFailureKind::HostKey => "host_key",
            SshFailureKind::Unknown => "unknown",
        }
    }
}

/// 单个 SSH 面板的连接生命周期。
#[derive(Debug, Clone)]
pub enum SshState {
    /// 用户手动断开；不重连。
    Disconnected,
    /// 首次连接进行中（`connect_ssh_tab` 中的同步生成阶段）。
    Connecting,
    /// GAP-5.6：`ssh` 子进程已被 fork（spawn 返回 Ok）但尚无远端字节
    /// 到达——ssh 进程正处于 TCP 连接 / 握手 / 认证中间。作为独立的
    /// UI 状态呈现，因为 spawn 边界本身无法观察到认证步骤（它全部发生
    /// 在 ssh 子进程内部）；iced 循环在首个远端字节到达时把它翻为
    /// [`SshState::Connected`]，若子进程在开口前退出则翻为
    /// [`SshState::Failed`]。
    Authenticating,
    /// 已连接且存活。
    Connected,
    /// 掉线后重连中；`attempt` 从 1 开始。
    Reconnecting {
        attempt: u8,
        next_retry: Instant,
    },
    /// 在 [`BACKOFF`] 次尝试后放弃，或是快速（<5s）的认证失败。
    /// `kind` 通过 [`classify_ssh_failure`] 从 ssh stderr 推导。
    Failed {
        kind: SshFailureKind,
        reason: String,
    },
}

impl SshState {
    /// 当此面板不应运行后台重连工作线程时为 true。
    /// 供 iced 退出处理器决定是否生成工作线程。
    pub fn is_terminal_for_reconnect(&self) -> bool {
        matches!(self, SshState::Disconnected | SshState::Failed { .. })
    }
}

/// 供 UI 渲染使用的 [`SshState`] 无锁快照——不含 `Instant`，`Clone`
/// 便宜，因此状态栏可以在不跨帧持有状态互斥锁的情况下读取它。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshStateSnapshot {
    Disconnected,
    Connecting,
    Authenticating,
    Connected,
    Reconnecting { attempt: u8 },
    Failed { kind: SshFailureKind },
}

impl From<&SshState> for SshStateSnapshot {
    fn from(s: &SshState) -> Self {
        match s {
            SshState::Disconnected => SshStateSnapshot::Disconnected,
            SshState::Connecting => SshStateSnapshot::Connecting,
            SshState::Authenticating => SshStateSnapshot::Authenticating,
            SshState::Connected => SshStateSnapshot::Connected,
            SshState::Reconnecting { attempt, .. } => SshStateSnapshot::Reconnecting { attempt: *attempt },
            SshState::Failed { kind, .. } => SshStateSnapshot::Failed { kind: *kind },
        }
    }
}

impl SshStateSnapshot {
    /// 供事件载荷 / IPC / 状态栏调试使用的稳定小写别名。
    pub fn as_str(self) -> &'static str {
        match self {
            SshStateSnapshot::Disconnected => "disconnected",
            SshStateSnapshot::Connecting => "connecting",
            SshStateSnapshot::Authenticating => "authenticating",
            SshStateSnapshot::Connected => "connected",
            SshStateSnapshot::Reconnecting { .. } => "reconnecting",
            SshStateSnapshot::Failed { .. } => "failed",
        }
    }

    /// 处于 `Reconnecting` 时的重连尝试序号，否则为 `None`。
    pub fn attempt(self) -> Option<u8> {
        match self {
            SshStateSnapshot::Reconnecting { attempt } => Some(attempt),
            _ => None,
        }
    }
}

/// GAP-5.6: 首个远端字节到达时的状态机决策(poll 循环 `TerminalEvent::Wakeup`
/// arm 调用)。返回 (是否翻 `Connected`, 是否注入重连分隔线)。仅
/// `Authenticating`(重连成功)盖分隔线;`Connecting`(初始连接)翻 `Connected`
/// 但**不**盖分隔线——以此区分"首次连接"与"重连",无需额外标志位。
pub fn on_first_remote_byte(prior: SshStateSnapshot) -> (bool, bool) {
    match prior {
        SshStateSnapshot::Authenticating => (true, true),
        SshStateSnapshot::Connecting => (true, false),
        _ => (false, false),
    }
}

/// 重连工作线程重新生成 ssh PTY 所需的全部参数。
/// 在重连触发时从 `SshCtx` 克隆出来，使工作线程自包含
/// （不借用 iced 持有的 `TerminalContext`）。
#[derive(Clone)]
pub struct ReconnectParams {
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub key_path: Option<String>,
    pub password: Option<String>,
    /// GAP-5.4：若原会话启用了 agent 转发，则重新生成时也带上。
    pub forward_agent: bool,
    /// GAP-40：重连时重放端口/X11 转发。
    pub forwards: SshForwards,
    /// GAP-41：跳板主机链。
    pub jump_hosts: Vec<String>,
}

impl ReconnectParams {
    pub fn connection_key(&self) -> ConnectionKey {
        ConnectionKey {
            host: self.host.clone(),
            port: self.port.unwrap_or(22),
            user: self.user.clone().unwrap_or_default(),
        }
    }
}

/// 工作线程发回 iced 主循环的轻量、可 `Clone` 的消息。生成的 PTY
/// 本身通过 hub 的 `completed` 交接映射传递（见
/// [`ReconnectHub::take_success`]）。
#[derive(Debug, Clone)]
pub enum ReconnectUpdate {
    /// 一次重连尝试失败；在下一次前先休眠。
    Progress {
        pane_id: PaneId,
        attempt: u8,
        next_retry_in: Duration,
    },
    /// 新的 ssh PTY 已生成（ssh 子进程已 fork）。GAP-5.6：这还不是
    /// “已连接”——ssh 子进程仍在连接/认证。iced 循环应调用
    /// [`ReconnectHub::take_success`] 认领 PTY，然后 `SshCtx::reattach`，
    /// 并设置 [`SshState::Authenticating`]；它会在首个远端字节到达时
    /// 翻为 [`SshState::Connected`]。
    Spawned { pane_id: PaneId },
    /// 所有退避尝试均已耗尽。`kind` 此处为 [`SshFailureKind::Unknown`]
    /// （工作线程的 `spawn_pty` `Err` 不携带 ssh stderr；已分类的失败
    /// 改由 GUI 侧的快速退出路径呈现）。
    Failed { pane_id: PaneId, kind: SshFailureKind, reason: String },
}

/// 按面板的取消句柄。工作线程睡在 `Condvar` 上，因此取消会立即唤醒它，
/// 而不是等完整个退避。
struct CancelSignal {
    cancelled: AtomicBool,
    cv: Condvar,
    lock: Mutex<()>,
}

impl CancelSignal {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            cancelled: AtomicBool::new(false),
            cv: Condvar::new(),
            lock: Mutex::new(()),
        })
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        // 唤醒睡在 condvar 上的工作线程，让它重新检查标志。
        let _guard = self.lock.lock();
        self.cv.notify_all();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// 休眠直到 `dur` 结束或被取消。若在等待期间被取消返回 `true`。
    fn sleep_or_cancel(&self, dur: Duration) -> bool {
        let guard = match self.lock.lock() {
            Ok(g) => g,
            // 毒化 → 视为已取消，使工作线程退出。
            Err(_) => return true,
        };
        let _ = self.cv.wait_timeout(guard, dur);
        self.is_cancelled()
    }
}

struct PaneSlot {
    signal: Arc<CancelSignal>,
}

/// 协调所有 SSH 面板的后台重连工作线程。
///
/// 单个 `ReconnectHub` 存在于 `PureTermApp` 上（放在 `Arc` 后面，因为
/// 稳定的 iced `Subscription` id——`ReconnectKey`——按指针包裹它）。
/// 工作线程是 `std::thread`（长时间退避休眠无法运行在 iced 的
/// 当前线程 tokio 运行时上）。更新通过 tokio 通道回流，其接收端通过
/// [`ReconnectHub::take_updates_receiver`] 一次性认领。
pub struct ReconnectHub {
    updates_tx: UnboundedSender<ReconnectUpdate>,
    updates_rx: Mutex<Option<UnboundedReceiver<ReconnectUpdate>>>,
    /// 成功生成的 PTY 由工作线程 → iced 交接。用 `Arc<Mutex<..>>`，
    /// 使工作线程持有便宜的克隆。
    completed: Arc<Mutex<HashMap<PaneId, (Box<SshPty>, Option<File>)>>>,
    slots: Mutex<HashMap<PaneId, PaneSlot>>,
    /// 按 `(host, port, user)` 串行化重连尝试。当 ControlMaster 主连接
    /// 死亡时，共享它的 N 个面板会同时掉线；没有这把锁，每个面板都会
    /// 竞争生成新的主连接（重连风暴）。每个键同一时间只有一个工作线程
    /// 生成；其余的廉价复用新的 ControlMaster socket。
    per_key_lock: Mutex<HashMap<ConnectionKey, Arc<Mutex<()>>>>,
    /// 反敲打：按面板记录的重连触发时间戳滚动窗口
    /// （见 [`FLAP_WINDOW`] / [`MAX_FLAPS`]）。按面板 id 索引，因此手动
    /// 重连（通过 `push_ssh_tab` 总是得到全新的面板 id）从干净状态开始。
    flap_history: Mutex<HashMap<PaneId, Vec<Instant>>>,
}

mod hub;

#[cfg(test)]
#[path = "reconnect/tests.rs"]
mod tests;
