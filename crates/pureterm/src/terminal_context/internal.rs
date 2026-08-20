//! 内部载体类型:`LocalCtx` / `SshCtx` / `SshRuntime` / `ThreadJoiner`。
//!
//! 这些 struct 是 [`super::TerminalContext`] 枚举两个变体的内部状态,字段
//! 都标记为 `pub(super)`,以便同模块下的构造(`construct.rs`)、接线
//! (`wiring.rs`)、操作(`ops.rs`)实现块按变体访问字段。文件外部不可见。

use std::cell::RefCell;
use std::fs::File;
use std::sync::Arc;

#[cfg(unix)]
use std::sync::atomic::AtomicBool;
#[cfg(unix)]
use std::sync::Mutex;

use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::Term;
use crossbeam_channel::Receiver;

#[cfg(unix)]
use pureterm_ssh::SshForwards;

use super::ChannelEventListener;
use alacritty_terminal::event_loop::Notifier;
#[cfg(unix)]
use crate::ssh::reconnect::SshState;

/// 本地 PTY 上下文:持有用户的 shell PTY。
pub(super) struct LocalCtx {
    pub(super) terminal: Arc<FairMutex<Term<ChannelEventListener>>>,
    pub(super) notifier: Notifier,
    pub(super) event_rx: Receiver<super::TerminalEvent>,
    pub(super) size: TermSize,
    pub(super) child_pid: u32,
    /// GAP-5.6:当此上下文是 `puretermd` 所持有 PTY 的薄镜像时,记录守护进程会话 id
    /// (由守护进程路径设置)。直接本地 PTY(`new_local` 回退)为 `None`,
    /// 因为直接本地 PTY 无法在 GUI 重启后存活,
    /// 因此不会被持久化/恢复。
    pub(super) session_id: Option<String>,
}

/// SSH PTY 上下文:持有一个远程 ssh -tt 子进程。
#[cfg(unix)]
pub(super) struct SshCtx {
    pub(super) terminal: Arc<FairMutex<Term<ChannelEventListener>>>,
    /// 可替换的每连接运行时(notifier 通道 + IO 线程 +
    /// stderr)。用 `RefCell` 包裹是因为 `TerminalContext` 位于
    /// 与渲染路径共享的 `Arc` 之后,`reattach` 需通过
    /// 内部可变性来修改它。
    pub(super) runtime: RefCell<SshRuntime>,
    pub(super) size: TermSize,
    /// 人类可读的会话档案(用于标签页标题)。
    pub(super) profile_display: String,
    /// 连接关键字段:用于关闭时释放连接池、重连时复用。
    pub(super) ssh_host: String,
    pub(super) ssh_user: Option<String>,
    pub(super) ssh_port: Option<u16>,
    /// 用于认证的身份文件(-i);重连时重放。
    pub(super) ssh_key_path: Option<String>,
    /// 已解析的密码(CredentialStore > TOML 明文)在重连时重放。
    /// 对务实子集采用裸 `String` 持有;
    /// 未来的加固可将其包进 `Zeroizing`。
    pub(super) ssh_password: Option<String>,
    /// GAP-5.6 重连状态机。采用 `Arc<Mutex<..>>`,使 iced 循环
    /// (UI 读取)与重连工作线程(写入 Progress/Failed)都能
    /// 访问它。
    pub(super) reconnect_state: Arc<Mutex<SshState>>,
    /// 在任一用户主动关闭路径(手动断开连接 / 关闭标签页)
    /// 写入 `exit\r` 之前置为 `true`。退出处理器据此决定
    /// 是否自动重连:用户主动退出绝不重连。
    pub(super) user_initiated_close: Arc<AtomicBool>,
    /// GAP-5.6:当此上下文镜像 `puretermd` 持有的 `ssh -tt` 子进程时,
    /// 记录守护进程会话 id(由守护进程路径设置)。
    /// 直接(非守护进程)SSH 上下文为 `None`,无法在 GUI 重启后存活。
    pub(super) session_id: Option<String>,
    /// GAP-40:状态栏用的简洁转发摘要(`L2 R1 X`)。
    pub(super) forward_summary: Option<String>,
    /// GAP-40:完整转发包(重连 + 显示用)。
    pub(super) ssh_forwards: SshForwards,
    /// GAP-41:重连用的 ProxyJump 跳板链。
    pub(super) ssh_jump_hosts: Vec<String>,
}

/// [`SshCtx`] 内部可替换的每连接运行时。
#[cfg(unix)]
pub(super) struct SshRuntime {
    pub(super) notifier: Notifier,
    pub(super) event_rx: Receiver<super::TerminalEvent>,
    /// `PtyEventLoop` 线程的句柄,类型擦除后无需引用
    /// 终端 crate 的私有 `State` 类型。在 `reattach` 时 join。
    pub(super) io_thread: Option<ThreadJoiner>,
    /// ssh 子进程的非阻塞 stderr 管道;
    /// 用于读取快速连接失败后的错误消息。
    pub(super) stderr_file: Option<File>,
}

/// 持有类型参数已擦除的 `JoinHandle`,使 [`SshCtx`] 不必
/// 指名终端 crate 的私有 `State` 类型。[`Self::join`] 等待线程结束;
/// drop 时将其分离(与之前
/// `let _io_thread = pty_loop.spawn()` 的行为一致)。
#[cfg(unix)]
pub(super) struct ThreadJoiner(pub(super) Option<Box<dyn FnOnce() + Send + 'static>>);

#[cfg(unix)]
impl ThreadJoiner {
    pub(super) fn new<T: Send + 'static>(handle: std::thread::JoinHandle<T>) -> Self {
        Self(Some(Box::new(move || {
            let _ = handle.join();
        })))
    }

    /// 等待包裹的线程结束(首次调用之后为空操作)。
    pub(super) fn join(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}
