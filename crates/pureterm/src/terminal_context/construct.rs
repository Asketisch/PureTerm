//! `impl TerminalContext` 的构造器集合(`new_local*` / `new_ssh*` /
//! `attach_*` / `new_local_auto`)。
//!
//! 这些方法构造上下文对象:本机直接 PTY、本机 daemon-owned PTY、
//! SSH 直连 PTY、SSH daemon-owned PTY,以及跨重启恢复用的 attach 路径。
//! 方法的对外语义保持与 P2 之前一致,只是物理上独立成文件。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(unix)]
use std::sync::Mutex;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, Notifier};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty::{self, Options as PtyOptions};
use crossbeam_channel::bounded;
// GAP-15：shell 集成注入选项（OSC 7 + OSC 133）。
use pureterm_daemon::shell_integration::ShellIntegrationOpts;

#[cfg(unix)]
use std::cell::RefCell;
#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::sync::atomic::AtomicBool;

#[cfg(unix)]
use super::internal::{SshCtx, SshRuntime, ThreadJoiner};
use super::internal::LocalCtx;
#[cfg(unix)]
use super::wiring::{wire_local_daemon, wire_ssh_daemon};
use super::ChannelEventListener;
use super::TerminalContext;
#[cfg(unix)]
use crate::ssh::config::SshProfile;
#[cfg(unix)]
use crate::ssh::pty::SshPty;
#[cfg(unix)]
use crate::ssh::reconnect::SshState;

impl TerminalContext {
    /// T-core-tmux-cc:创建一个 `tmux -CC` 控制模式的终端上下文。
    ///
    /// `session` 为 `Some(name)` 时附加到已有会话,否则新建一个名为
    /// `pureterm-<pid>` 的会话。返回一个**已经连上**的上下文 ——
    /// `tmux -CC` 子进程在构造时就已拉起,`%output` 信封由
    /// [`crate::tmux_cc::pump`] 泵进网格。
    ///
    /// 失败(tmux 不存在 / 会话不存在)时返回带 tmux stderr 的错误,
    /// 让调用方能把真实原因告诉用户,而不是静默失败。
    #[cfg(unix)]
    pub fn new_tmux(
        session: Option<&str>,
        cols: u16,
        rows: u16,
        new_window: bool,
        socket_label: Option<&str>,
    ) -> io::Result<Self> {
        use crate::tmux_cc::pty::{TmuxChild, TmuxSpawnArgs};

        let mut args = match session {
            Some(name) if new_window => TmuxSpawnArgs::attach_new_window(name.to_string()),
            Some(name) => TmuxSpawnArgs::attach(name.to_string()),
            None => TmuxSpawnArgs::new_session(&format!("pureterm-{}", std::process::id()), cols, rows),
        };
        // tmux socket 命名空间(`tmux -L <name>`)。指定它即连接到**独立**
        // 的 tmux 服务器 —— 测试用它来隔离,用户也可以用它连接并行的
        // tmux 实例。
        args.socket_label = socket_label.map(str::to_string);
        let mut child = TmuxChild::spawn(&args).map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("failed to spawn `tmux -CC`: {e}"))
        })?;

        let (event_tx, event_rx) = bounded(64);
        let event_listener = ChannelEventListener::new(event_tx);
        let size = TermSize::new(cols.max(1) as usize, rows.max(1) as usize);
        let terminal = Term::new(TermConfig::default(), &size, event_listener.clone());
        let terminal = Arc::new(FairMutex::new(terminal));

        // 信封接收端的克隆交给泵线程 —— 它阻塞在这里,不持有 child 的锁,
        // 因此用户按键(`%input`)永远不会被堵住。
        let envelopes = child.envelope_rx();
        let writer = child.writer();
        let pane_id = child.pane_id_handle();
        let session_name = session.map(|s| s.to_string());
        crate::tmux_cc::pump::spawn_pump(
            Arc::clone(&terminal),
            envelopes,
            event_listener,
            writer,
            pane_id,
        );

        Ok(TerminalContext::Tmux(super::internal::TmuxCtx {
            terminal,
            event_rx,
            size,
            child: Arc::new(Mutex::new(child)),
            session_id: None,
            session_name,
        }))
    }

    /// 创建新的本地 PTY 终端上下文。等价于 P2 之前的 `TerminalContext::new()`。
    ///
    /// `cwd` 为 `Some` 时成为 shell 的启动目录，使新 Tab/面板能继承
    /// 当前工作目录。`shell_opts`（GAP-15）选择哪些 shell 在 OSC 7 之外
    /// 注入 OSC 133 集成。
    pub fn new_local(cwd: Option<&Path>, shell_opts: ShellIntegrationOpts) -> io::Result<Self> {
        let (event_tx, event_rx) = bounded(64);
        let event_listener = ChannelEventListener::new(event_tx);

        let num_cols: u16 = 80;
        let num_lines: u16 = 24;
        let cell_width: u16 = 8;
        let cell_height: u16 = 16;
        let size = TermSize::new(num_cols as usize, num_lines as usize);

        let term_config = TermConfig::default();
        let terminal = Term::new(term_config, &size, event_listener.clone());
        let terminal = Arc::new(FairMutex::new(terminal));

        let window_size = WindowSize {
            num_lines,
            num_cols,
            cell_width,
            cell_height,
        };
        tty::setup_env();

        // 构建 PTY 选项：把启动目录设为继承的 cwd，并把 OSC 7 目录上报钩子
        // 注入子进程环境（与 daemon 路径共用），使该 shell 上报其工作
        // 目录——这正是新 Tab/面板能继承它的原因。
        //
        // 用 `Options::env` 而不是本进程的 `std::env::set_var`，避免了旧做法
        // 的两个问题：(1) 并发 PTY 生成在同一个临时 ZDOTDIR 上竞争；(2) 把
        // 临时 ZDOTDIR 泄漏到无关的生成中。同时使钩子注入不进入 `Msg::Input`，
        // 以免负载超过约 80 字节时阻塞 macOS .app 包上的 PTY IO 线程。
        let mut pty_options = PtyOptions::default();
        pty_options.working_directory = cwd.map(PathBuf::from);
        // GAP-15：OSC 7 目录上报 + OSC 133 标记。`shell_opts` 来自已加载的
        // `[shell_integration]` 配置（daemon 路径无法读取配置，始终使用
        // all_on——这些开关只在直接 PTY 兜底路径上生效）。
        pty_options.env.extend(
            pureterm_daemon::shell_integration::prepare_shell_env(
                &std::process::id().to_string(),
                shell_opts,
            ),
        );

        let pty = tty::new(&pty_options, window_size, 0)?;

        let child_pid = pty.child().id();

        let pty_loop = PtyEventLoop::new(
            Arc::clone(&terminal),
            event_listener,
            pty,
            false,
            false,
        )?;

        let loop_tx = pty_loop.channel();
        let _io_thread = pty_loop.spawn();

        Ok(TerminalContext::Local(LocalCtx {
            terminal,
            notifier: Notifier(loop_tx),
            event_rx,
            size,
            child_pid,
            session_id: None,
        }))
    }

    /// 创建经由持久化 daemon（`puretermd`）路由的本地 PTY 终端上下文。
    /// daemon 拥有真正的 PTY（因此子进程在 GUI 关闭后仍存活）；本侧持有
    /// 一个 `DaemonPty`，通过 socketpair 镜像字节。
    ///
    /// GAP-5.1 M1：对调用方而言与 `new_local()` 契约一致——返回带可用
    /// 终端、notifier 和事件通道的 `TerminalContext::Local`。
    #[cfg(unix)]
    pub fn new_local_via_daemon(cwd: Option<&Path>) -> io::Result<Self> {
        use crate::daemon_client;

        let daemon_socket = daemon_client::ensure_daemon()?;

        let num_cols: u16 = 80;
        let num_lines: u16 = 24;
        let size = TermSize::new(num_cols as usize, num_lines as usize);

        let (session_id, data_path) = daemon_client::create_local(
            &daemon_socket,
            num_cols,
            num_lines,
            cwd.and_then(|p| p.to_str()),
            None,
            None,
            crate::ssh::config::load_ssh_settings(&crate::app_config::AppConfig::config_path())
                .smallest_screen_wins,
        )?;

        let ctx = wire_local_daemon(&data_path, size, session_id)?;
        Ok(TerminalContext::Local(ctx))
    }

    /// GAP-5.6 跨重启恢复：重新附加到上一个 GUI 运行期间创建且仍存活的
    /// daemon 会话。daemon 以 `SCROLLBACK` 帧重放会话的回滚，使新构建的
    /// `Term` + `vte::ansi::Processor` 重建可见网格与回滚。shell 继续
    /// 不间断运行（无需重新生成）。
    #[cfg(unix)]
    pub fn attach_local_via_daemon(session_id: &str, size: TermSize) -> io::Result<Self> {
        use crate::daemon_client;

        let daemon_socket = daemon_client::ensure_daemon()?;
        let data_path = daemon_client::attach_existing(&daemon_socket, session_id)?;
        let ctx = wire_local_daemon(&data_path, size, session_id.to_string())?;
        Ok(TerminalContext::Local(ctx))
    }

    /// GAP-5.1 M1：让新本地终端走持久化 daemon，使 PTY 在 GUI 重启后
    /// 仍存活。当 daemon 不可达时（例如 `puretermd` 未与运行中的二进制
    /// 一同打包），回退到直接本地 PTY，保证终端始终可用——代价是 daemon
    /// 可用前会话无法跨重启存活。这是 GUI 创建 Tab/拆分应使用的入口。
    ///
    /// `cwd` 为 `Some` 时转发给生成 shell 的任意路径，使新 Tab/面板能
    /// 继承当前工作目录。`shell_opts`（GAP-15）只在直接 PTY 兜底路径上
    /// 生效（daemon 路径始终为完整集成）。
    pub fn new_local_auto(cwd: Option<&Path>, shell_opts: ShellIntegrationOpts) -> Self {
        #[cfg(unix)]
        {
            match Self::new_local_via_daemon(cwd) {
                Ok(ctx) => return ctx,
                Err(e) => log::warn!(
                    "puretermd unavailable ({e}); using direct local PTY \
                     (session will not survive GUI restart)"
                ),
            }
        }
        #[cfg(not(unix))]
        {
            log::debug!("daemon PTY path is unix-only; using direct local PTY");
        }
        Self::new_local(cwd, shell_opts).expect("Failed to create terminal context")
    }

    /// 创建新的 SSH 支撑终端上下文。SSH 子进程经 `SshSessionManager::spawn_pty`
    /// 生成，然后接入与本地路径相同的 alacritty `PtyEventLoop`。
    ///
    /// 这是 GAP-5.7 P3 阶段将调用的入口。仅 Unix；Windows 上调用方
    /// 必须改用桩实现。
    #[cfg(unix)]
    pub fn new_ssh(
        profile: &SshProfile,
        ssh_pty: SshPty,
        socket_path: PathBuf,
        password: Option<String>,
        stderr: Option<File>,
    ) -> io::Result<Self> {
        

        let (event_tx, event_rx) = bounded(64);
        let event_listener = ChannelEventListener::new(event_tx);

        let num_cols: u16 = 80;
        let num_lines: u16 = 24;
        let _cell_width: u16 = 8;
        let _cell_height: u16 = 16;
        let size = TermSize::new(num_cols as usize, num_lines as usize);

        let term_config = TermConfig::default();
        let terminal = Term::new(term_config, &size, event_listener.clone());
        let terminal = Arc::new(FairMutex::new(terminal));

        // 把具体的 SshPty 交给 PtyEventLoop。循环线程在其整个生命周期内按值
        // 拥有 pty；`ThreadJoiner` 让后续的 `reattach` 先等待该线程结束，
        // 再启动替换循环。
        let pty_loop = PtyEventLoop::new(
            Arc::clone(&terminal),
            event_listener,
            ssh_pty,
            false,
            false,
        )?;

        let loop_tx = pty_loop.channel();
        let io_thread = ThreadJoiner::new(pty_loop.spawn());

        // socket_path 已记录在 SshSessionManager 池中。
        let _ = socket_path;

        Ok(TerminalContext::Ssh(SshCtx {
            terminal,
            runtime: RefCell::new(SshRuntime {
                notifier: Notifier(loop_tx),
                event_rx,
                io_thread: Some(io_thread),
                stderr_file: stderr,
            }),
            size,
            profile_display: profile.name.clone(),
            ssh_host: profile.host.clone(),
            ssh_user: profile.user.clone(),
            ssh_port: profile.port,
            ssh_key_path: profile.key_path.clone(),
            ssh_password: password,
            reconnect_state: Arc::new(Mutex::new(SshState::Connecting)),
            user_initiated_close: Arc::new(AtomicBool::new(false)),
            session_id: None,
            forward_summary: profile.forwards().summary(),
            ssh_forwards: profile.forwards(),
        ssh_jump_hosts: profile.jump_hosts.clone(),
        }))
    }

    /// GAP-5.1 M3：打开由持久化 daemon 拥有的 SSH 会话，使远程 shell 在
    /// GUI 重启后仍存活。GUI 侧与本地路径使用的是同一个 `DaemonPty` 镜像——
    /// 只是 daemon 生成 `ssh` 而不是本地 shell。`password` 必须已由调用方
    /// 解析（CredentialStore / profile）；它会转发给 daemon 用于 `SSH_ASKPASS`。
    ///
    /// 此处有意不接入自动重连（GAP-5.6）：daemon 让现有 ssh 子进程跨 GUI
    /// 重启存活，重连附加会重建回滚。若网络本身断开，用户通过 SSH 对话框
    /// 重连（daemon 侧重连是后续细化项）。
    #[cfg(unix)]
    pub fn new_ssh_via_daemon(profile: &SshProfile, password: Option<String>) -> io::Result<Self> {
        use crate::daemon_client;

        let daemon_socket = daemon_client::ensure_daemon()?;
        let num_cols: u16 = 80;
        let num_lines: u16 = 24;
        let size = TermSize::new(num_cols as usize, num_lines as usize);

        let settings =
            crate::ssh::config::load_ssh_settings(&crate::app_config::AppConfig::config_path());

        let (session_id, data_path) = daemon_client::create_ssh(
            &daemon_socket,
            &profile.host,
            profile.user.as_deref(),
            profile.port,
            profile.key_path.as_deref(),
            password.as_deref(),
            num_cols,
            num_lines,
            profile.forward_agent,
            // ssh.agent_sock 配置覆盖接线属于后续工作；解析器已支持它（override
            // > $SSH_AUTH_SOCK > launchctl）——传 None 以使用继承的环境变量
            // (来自 launchctl 或 shell 环境)。
            None,
            profile.forwards(),
            settings.smallest_screen_wins,
        )?;

        let ctx = wire_ssh_daemon(&data_path, size, profile, password, session_id)?;
        Ok(TerminalContext::Ssh(ctx))
    }

    /// GAP-5.6 跨重启恢复：重新附加到仍存活的 daemon 拥有的 SSH 会话
    /// （`ssh -tt` 子进程在 daemon 中经受住了 GUI 重启）。无需重新认证——
    /// 远程 shell 继续运行。`profile` 提供显示名 / 连接元数据（恢复时
    /// 按名称从 GUI 持久化的配置文件列表查找；若配置文件已被删除，调用方
    /// 传入合成占位符，之后掉线时的自动重连仅为尽力而为）。
    #[cfg(unix)]
    pub fn attach_ssh_via_daemon(
        session_id: &str,
        profile: &SshProfile,
        size: TermSize,
    ) -> io::Result<Self> {
        use crate::daemon_client;

        let daemon_socket = daemon_client::ensure_daemon()?;
        let data_path = daemon_client::attach_existing(&daemon_socket, session_id)?;
        let ctx = wire_ssh_daemon(&data_path, size, profile, None, session_id.to_string())?;
        Ok(TerminalContext::Ssh(ctx))
    }
}
