//! `SessionManager` 实现:会话生命周期 (create_local / create_ssh / kill /
//! hibernate / resume)、文件浏览 (list_dir / read_file / write_file /
//! find_files)、pipe-pane 状态、CPU 采样 (GAP-7.3)。仅依赖根模块的 struct
//! 定义与 io_helpers / pump 子模块的入口函数。

use std::io;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::tty;
use pureterm_ssh::agent_sock;
use pureterm_ssh::{SshCommandBuilder, SshForwards, SshPty};

use crate::discovery;
use crate::remote_fs::{self, SshSessionMeta};

use super::io_helpers::new_session_id;
use super::pump::spawn_pump;
use super::{SessionHandle, SessionInfo, SessionManager};

impl SessionManager {
    /// 拉起一个新的本地 PTY 会话。返回其 id。
    ///
    /// `cwd` 为 `Some` 时作为 shell 的启动目录(经
    /// `tty::Options::working_directory`)。OSC 7 cwd 上报钩子
    /// 总是注入,使 GUI 拉起新标签页/窗格时能继承该
    /// shell 的工作目录。以会话 id 作标签,使并发
    /// 会话得到互不相同的临时 zdotdir。
    pub fn create_local(
        &self,
        cols: u16,
        rows: u16,
        cwd: Option<PathBuf>,
        argv: Option<Vec<String>>,
        agent_kind: Option<String>,
        smallest_screen_wins: bool,
    ) -> io::Result<String> {
        let id = new_session_id();
        let window_size =
            WindowSize { num_lines: rows, num_cols: cols, cell_width: 8, cell_height: 16 };
        tty::setup_env();
        let mut options = tty::Options::default();
        // GAP-15:OSC 7 cwd + OSC 133 shell 集成标记。守护进程
        // 无法读配置,因此集成在此永远开启(镜像 OSC 7
        // 此前也永远开启的做法);GUI 的 `[shell_integration]` 开关
        // 只门控直连(非守护)PTY 回退路径。
        options
            .env
            .extend(crate::shell_integration::prepare_shell_env(
                &id,
                crate::shell_integration::ShellIntegrationOpts::all_on(),
            ));
        options.working_directory = cwd;
        // GAP-7.5:显式给出 argv 时(经如 `claude --resume <id>`
        // 冷恢复 agent),拉起该命令而非默认
        // shell。`tty::Options.shell` 接受任意程序 + 参数,因此
        // 无需单独的 fork-and-exec 路径。
        if let Some(argv) = &argv {
            if !argv.is_empty() {
                let program = argv[0].clone();
                let args = argv[1..].to_vec();
                options.shell = Some(tty::Shell::new(program, args));
            }
        }
        let pty = tty::new(&options, window_size, 0)?;
        let child_pid = pty.child().id();

        discovery::ensure_dirs()?;
        let data_socket_path = discovery::sessions_dir().join(format!("s-{id}.sock"));
        if data_socket_path.exists() {
            let _ = std::fs::remove_file(&data_socket_path);
        }
        let listener = UnixListener::bind(&data_socket_path)?;
        listener.set_nonblocking(true)?;
        discovery::restrict_socket(&data_socket_path); // GAP-5.1 M5: owner-only

        let stop = Arc::new(AtomicBool::new(false));
        let last_output_at = Arc::new(Mutex::new(Instant::now()));
        let last_cpu_active_at = Arc::new(Mutex::new(Instant::now()));
        let hibernated = Arc::new(AtomicBool::new(false));
        let pipe_feeder: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>> =
            Arc::new(Mutex::new(None));
        let pump = spawn_pump(
            pty,
            listener,
            stop.clone(),
            last_output_at.clone(),
            pipe_feeder.clone(),
            cols,
            rows,
            smallest_screen_wins,
        );

        let handle = Arc::new(Mutex::new(SessionHandle {
            id: id.clone(),
            kind: "local".into(),
            child_pid,
            data_socket_path,
            agent_kind,
            last_output_at,
            last_cpu_active_at,
            hibernated,
            pipe_feeder,
            ssh_meta: None,
            stop,
            join: Some(pump),
        }));
        self.sessions.lock().unwrap().insert(id.clone(), handle);
        Ok(id)
    }

    /// 拉起新的 SSH PTY 会话(GAP-5.1 M3)。守护进程持有 `ssh -tt`
    /// 子进程 + PTY master,远端 shell 因此能在 GUI 重启后存活。
    /// `password`(由 GUI 解锁)驱动 `SSH_ASKPASS`;`None` 让 ssh
    /// 经 PTY 提示(密钥/agent 认证)。
    pub fn create_ssh(
        &self,
        host: &str,
        user: Option<&str>,
        port: Option<u16>,
        identity_file: Option<&str>,
        password: Option<&str>,
        cols: u16,
        rows: u16,
        forward_agent: bool,
        agent_sock_override: Option<&str>,
        forwards: SshForwards,
        smallest_screen_wins: bool,
    ) -> io::Result<String> {
        let id = new_session_id();

        discovery::ensure_dirs()?;
        // 每会话独立的 ControlMaster socket 目录,避免并发 SSH 会话
        // 在多路复用 socket 上冲突。
        let ssh_socket_dir = discovery::sessions_dir().join(format!("ssh-{id}"));
        std::fs::create_dir_all(&ssh_socket_dir)?;
        let user_seg = user.unwrap_or("");
        let control_socket =
            ssh_socket_dir.join(format!("{user_seg}@{host}:{}", port.unwrap_or(22)));

        let mut builder = SshCommandBuilder::new(control_socket.clone(), host.to_string());
        if let Some(u) = user {
            builder = builder.with_user(u);
        }
        if let Some(p) = port {
            builder = builder.with_port(p);
        }
        if let Some(k) = identity_file {
            builder = builder.with_identity_file(k);
        }
        builder = builder.with_forwards(forwards);
        // GAP-5.4:按需把本地 ssh-agent 转发到远端主机。
        let agent_sock = if forward_agent {
            builder = builder.with_forward_agent(true);
            agent_sock::resolve_agent_sock(agent_sock_override.map(Path::new))
        } else {
            None
        };
        let argv = builder.build();

        let (ssh_pty, _stderr) = SshPty::spawn(&argv, Some((cols, rows)), password, agent_sock.as_deref())?;
        let child_pid = ssh_pty.child_pid();

        let data_socket_path = discovery::sessions_dir().join(format!("s-{id}.sock"));
        if data_socket_path.exists() {
            let _ = std::fs::remove_file(&data_socket_path);
        }
        let listener = UnixListener::bind(&data_socket_path)?;
        listener.set_nonblocking(true)?;
        discovery::restrict_socket(&data_socket_path); // GAP-5.1 M5: owner-only

        let stop = Arc::new(AtomicBool::new(false));
        let last_output_at = Arc::new(Mutex::new(Instant::now()));
        let last_cpu_active_at = Arc::new(Mutex::new(Instant::now()));
        let hibernated = Arc::new(AtomicBool::new(false));
        let pipe_feeder: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>> =
            Arc::new(Mutex::new(None));
        let pump = spawn_pump(
            ssh_pty,
            listener,
            stop.clone(),
            last_output_at.clone(),
            pipe_feeder.clone(),
            cols,
            rows,
            smallest_screen_wins,
        );

        let handle = Arc::new(Mutex::new(SessionHandle {
            id: id.clone(),
            kind: "ssh".into(),
            child_pid,
            data_socket_path,
            agent_kind: None,
            last_output_at,
            last_cpu_active_at,
            hibernated,
            pipe_feeder,
            ssh_meta: Some(SshSessionMeta {
                control_socket: control_socket.clone(),
                host: host.to_string(),
            }),
            stop,
            join: Some(pump),
        }));
        self.sessions.lock().unwrap().insert(id.clone(), handle);
        Ok(id)
    }

    /// GAP-43:为存活会话写一个文本文件。
    pub fn write_file(&self, sid: &str, path: &str, content: &str) -> io::Result<()> {
        let sessions = self.sessions.lock().unwrap();
        let handle = sessions
            .get(sid)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown session_id"))?;
        let h = handle.lock().unwrap();
        match h.kind.as_str() {
            "local" => remote_fs::write_local_file(path, content),
            "ssh" => {
                let meta = h.ssh_meta.as_ref().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::Other, "ssh session missing control socket metadata")
                })?;
                remote_fs::write_remote_file(&meta.control_socket, &meta.host, path, content)
            }
            other => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("write_file unsupported for session kind {other}"),
            )),
        }
    }

    /// GAP-43:在目录下按名称查找路径。
    pub fn find_files(
        &self,
        sid: &str,
        base: &str,
        query: &str,
        max: usize,
    ) -> io::Result<Vec<remote_fs::FindMatch>> {
        let sessions = self.sessions.lock().unwrap();
        let handle = sessions
            .get(sid)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown session_id"))?;
        let h = handle.lock().unwrap();
        match h.kind.as_str() {
            "local" => remote_fs::find_local(base, query, max),
            "ssh" => {
                let meta = h.ssh_meta.as_ref().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::Other, "ssh session missing control socket metadata")
                })?;
                remote_fs::find_remote(&meta.control_socket, &meta.host, base, query, max)
            }
            other => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("find_files unsupported for session kind {other}"),
            )),
        }
    }

    /// GAP-43:为存活会话读文件(本地 `read` 或远端 `ssh -S head`)。
    pub fn read_file(
        &self,
        sid: &str,
        path: &str,
        max_bytes: u64,
    ) -> io::Result<remote_fs::FileReadResult> {
        let sessions = self.sessions.lock().unwrap();
        let handle = sessions
            .get(sid)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown session_id"))?;
        let h = handle.lock().unwrap();
        match h.kind.as_str() {
            "local" => remote_fs::read_local_file(path, max_bytes),
            "ssh" => {
                let meta = h.ssh_meta.as_ref().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::Other, "ssh session missing control socket metadata")
                })?;
                remote_fs::read_remote_file(&meta.control_socket, &meta.host, path, max_bytes)
            }
            other => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("read_file unsupported for session kind {other}"),
            )),
        }
    }

    /// GAP-43:为存活会话列举目录(本地 `read_dir` 或经会话
    /// ControlMaster socket 的远端 `ssh -S`)。
    pub fn list_dir(&self, sid: &str, path: &str) -> io::Result<Vec<remote_fs::FileEntry>> {
        let sessions = self.sessions.lock().unwrap();
        let handle = sessions
            .get(sid)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown session_id"))?;
        let h = handle.lock().unwrap();
        match h.kind.as_str() {
            "local" => {
                let p = if path.is_empty() { "." } else { path };
                remote_fs::list_local_dir(Path::new(p))
            }
            "ssh" => {
                let meta = h.ssh_meta.as_ref().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::Other, "ssh session missing control socket metadata")
                })?;
                remote_fs::list_remote_dir(&meta.control_socket, &meta.host, path)
            }
            other => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("list_dir unsupported for session kind {other}"),
            )),
        }
    }

    /// GAP-6.6:按 id 取会话句柄(供 pipe-pane 控制)。
    pub fn session_handle(&self, sid: &str) -> Option<Arc<Mutex<SessionHandle>>> {
        self.sessions.lock().unwrap().get(sid).cloned()
    }

    /// GAP-6.6:当前存在活动 pipe-pane 管道的会话 id。
    pub fn pipe_status(&self) -> Vec<String> {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(sid, h)| {
                if h.lock().unwrap().pipe_feeder.lock().unwrap().is_some() {
                    Some(sid.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// 把存活会话枚举为可序列化描述符。
    pub fn list(&self) -> Vec<SessionInfo> {
        let now = Instant::now();
        self.sessions
            .lock()
            .unwrap()
            .values()
            .map(|h| {
                let h = h.lock().unwrap();
                // GAP-7.3:从泵/CPU tick 时间戳计算空闲时长。
                let pty_idle_secs = h
                    .last_output_at
                    .lock()
                    .ok()
                    .map(|t| now.duration_since(*t).as_secs());
                let cpu_idle_secs = h
                    .last_cpu_active_at
                    .lock()
                    .ok()
                    .map(|t| now.duration_since(*t).as_secs());
                SessionInfo {
                    id: h.id.clone(),
                    kind: h.kind.clone(),
                    child_pid: h.child_pid,
                    data_path: h.data_socket_path.to_string_lossy().into_owned(),
                    agent_kind: h.agent_kind.clone(),
                    pty_idle_secs,
                    cpu_idle_secs,
                    hibernated: h.hibernated.load(Ordering::SeqCst),
                }
            })
            .collect()
    }

    /// 会话数据 socket 的路径(GUI 在 `attach` 之后连接此处)。
    pub fn data_path(&self, id: &str) -> Option<PathBuf> {
        self.sessions.lock().unwrap().get(id).map(|h| h.lock().unwrap().data_socket_path.clone())
    }

    /// GAP-7.3:采样所有未休眠会话的子进程 CPU 时间,若自上次
    /// 采样以来有前进则刷新 `last_cpu_active_at`(使
    /// `cpu_idle_secs` 反映真实 CPU 不活动)。由守护进程的
    /// 周期 CPU tick(`runtime::run`)调用。先快照会话列表,
    /// 再在会话锁之外采样,使(阻塞的)ps/proc 读取不
    /// 阻塞 `list`/`attach`/控制流量。
    pub fn sample_cpu_pass(&self) {
        let now = Instant::now();
        let targets: Vec<(String, u32, Arc<Mutex<Instant>>, Arc<AtomicBool>)> = {
            let map = self.sessions.lock().unwrap();
            map.values()
                .map(|h| {
                    let h = h.lock().unwrap();
                    (
                        h.id.clone(),
                        h.child_pid,
                        h.last_cpu_active_at.clone(),
                        h.hibernated.clone(),
                    )
                })
                .collect()
        };
        for (id, pid, last_cpu_active_at, hibernated) in targets {
            if hibernated.load(Ordering::SeqCst) {
                continue;
            }
            let Some(sample) = crate::idle_probe::sample_cpu_times(pid) else {
                continue;
            };
            let advanced = {
                let mut samples = self.cpu_samples.lock().unwrap();
                let prev = samples.insert(id, sample);
                // 首次采样计为“活动”(别让新建
                // 会话在尚未运行时就被读作 CPU 空闲)。
                prev.map_or(true, |p| p.total() != sample.total())
            };
            if advanced {
                if let Ok(mut t) = last_cpu_active_at.lock() {
                    *t = now;
                }
            }
        }
    }

    /// 杀掉会话的子进程并从注册表移除。
    pub fn kill(&self, id: &str) -> bool {
        let handle = self.sessions.lock().unwrap().remove(id);
        if let Some(handle) = handle {
            unsafe {
                libc::kill(handle.lock().unwrap().child_pid as i32, libc::SIGHUP);
            }
            // `handle` 在此丢弃 → join 泵 + 移除数据 socket。
            // 会话没了,它注入 shell 的引导脚本目录也没用了 —— 删掉,
            // 否则系统临时目录里会越堆越多 `.pureterm-*` 垃圾目录。
            crate::shell_integration::cleanup_temp_dirs(id);
            true
        } else {
            false
        }
    }

    /// GAP-7.3:对会话子进程 SIGSTOP(休眠)。PTY 保持打开——
    /// SIGSTOP 冻结调度但不关闭 fd——因此 `resume` 可以
    /// SIGCONT 唤醒它。幂等(已休眠则空操作)。会话未知
    /// 返回 `false`。
    pub fn hibernate(&self, id: &str) -> bool {
        let map = self.sessions.lock().unwrap();
        let Some(handle) = map.get(id) else { return false };
        let h = handle.lock().unwrap();
        if h.hibernated.load(Ordering::SeqCst) {
            return true;
        }
        // SAFETY:SIGSTOP 是 async-signal-safe 的;child_pid 是我们自己
        // fork 的子进程(该句柄生命周期内绝不是被回收的外来 pid)。
        unsafe { libc::kill(h.child_pid as i32, libc::SIGSTOP) };
        h.hibernated.store(true, Ordering::SeqCst);
        drop(h);
        drop(map);
        // 丢弃缓存的 CPU 采样,使恢复后的首次采样计为
        // “活动”(过期采样会让恢复后立刻的 cpu_idle_secs 偏高)。
        self.cpu_samples.lock().unwrap().remove(id);
        true
    }

    /// GAP-7.3:对已休眠会话的子进程 SIGCONT(恢复)。幂等。
    /// 刷新空闲时间戳,使刚恢复的会话不会被
    /// 立即重新判定为空闲。会话未知返回 `false`。
    pub fn resume(&self, id: &str) -> bool {
        let map = self.sessions.lock().unwrap();
        let Some(handle) = map.get(id) else { return false };
        let h = handle.lock().unwrap();
        unsafe { libc::kill(h.child_pid as i32, libc::SIGCONT) };
        h.hibernated.store(false, Ordering::SeqCst);
        let now = Instant::now();
        if let Ok(mut t) = h.last_output_at.lock() {
            *t = now;
        }
        if let Ok(mut t) = h.last_cpu_active_at.lock() {
            *t = now;
        }
        true
    }
}
