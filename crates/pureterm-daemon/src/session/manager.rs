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
    /// Spawn a new local-PTY session. Returns its id.
    ///
    /// `cwd`, when `Some`, becomes the shell's startup directory (via
    /// `tty::Options::working_directory`). The OSC 7 cwd-reporting hook is
    /// always injected so the GUI can inherit this shell's working directory
    /// when spawning new tabs/panes. Tagged with the session id so concurrent
    /// sessions get distinct temp zdotdirs.
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
        // GAP-15: OSC 7 cwd + OSC 133 shell-integration marks. The daemon has
        // no config access, so integration is always-on here (mirrors how OSC 7
        // was already always-on); the GUI's `[shell_integration]` knobs gate
        // only the direct (non-daemon) PTY fallback path.
        options
            .env
            .extend(crate::shell_integration::prepare_shell_env(
                &id,
                crate::shell_integration::ShellIntegrationOpts::all_on(),
            ));
        options.working_directory = cwd;
        // GAP-7.5: when an explicit argv is given (cold resume of an agent via
        // e.g. `claude --resume <id>`), spawn that command instead of the
        // default shell. `tty::Options.shell` accepts any program + args, so
        // no separate fork-and-exec path is needed.
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

    /// Spawn a new SSH-PTY session (GAP-5.1 M3). The daemon owns the `ssh -tt`
    /// child + PTY master, so the remote shell survives GUI restart. `password`
    /// (resolved + unlocked by the GUI) drives `SSH_ASKPASS`; `None` lets ssh
    /// prompt via the PTY (key/agent auth).
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
        // Per-session ControlMaster socket dir so concurrent SSH sessions don't
        // collide on the multiplex socket.
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
        // GAP-5.4: forward the local ssh-agent to the remote host when asked.
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

    /// GAP-43: write a text file for a live session.
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

    /// GAP-43: find paths by name under a directory.
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

    /// GAP-43: read a file for a live session (local `read` or remote `ssh -S head`).
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

    /// GAP-43: list a directory for a live session (local `read_dir` or remote
    /// `ssh -S` over the session's ControlMaster socket).
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

    /// GAP-6.6: fetch a session handle by id (for pipe-pane control).
    pub fn session_handle(&self, sid: &str) -> Option<Arc<Mutex<SessionHandle>>> {
        self.sessions.lock().unwrap().get(sid).cloned()
    }

    /// GAP-6.6: session ids that currently have an active pipe-pane tap.
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

    /// Enumerate live sessions as serializable descriptors.
    pub fn list(&self) -> Vec<SessionInfo> {
        let now = Instant::now();
        self.sessions
            .lock()
            .unwrap()
            .values()
            .map(|h| {
                let h = h.lock().unwrap();
                // GAP-7.3: compute idle durations from the pump/CPU-tick timestamps.
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

    /// Path of a session's data socket (the GUI connects here after `attach`).
    pub fn data_path(&self, id: &str) -> Option<PathBuf> {
        self.sessions.lock().unwrap().get(id).map(|h| h.lock().unwrap().data_socket_path.clone())
    }

    /// GAP-7.3: sample every non-hibernated session's child CPU time and, if it
    /// advanced since the last sample, refresh `last_cpu_active_at` (so
    /// `cpu_idle_secs` reflects real CPU inactivity). Called by the daemon's
    /// periodic CPU tick (`runtime::run`). Snapshots the session list, then
    /// samples outside the session lock so the (blocking) ps/proc read doesn't
    /// block `list`/`attach`/control traffic.
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
                // First-ever sample counts as "active" (don't let a freshly
                // created session read as CPU-idle before it has run).
                prev.map_or(true, |p| p.total() != sample.total())
            };
            if advanced {
                if let Ok(mut t) = last_cpu_active_at.lock() {
                    *t = now;
                }
            }
        }
    }

    /// Kill a session's child and remove it from the registry.
    pub fn kill(&self, id: &str) -> bool {
        let handle = self.sessions.lock().unwrap().remove(id);
        if let Some(handle) = handle {
            unsafe {
                libc::kill(handle.lock().unwrap().child_pid as i32, libc::SIGHUP);
            }
            // `handle` drops here -> joins pump + removes the data socket.
            true
        } else {
            false
        }
    }

    /// GAP-7.3: SIGSTOP the session's child (hibernate). The PTY stays open —
    /// SIGSTOP freezes scheduling but doesn't close fds — so `resume` can
    /// SIGCONT it back. Idempotent (already-hibernated is a no-op). Returns
    /// `false` if the session is unknown.
    pub fn hibernate(&self, id: &str) -> bool {
        let map = self.sessions.lock().unwrap();
        let Some(handle) = map.get(id) else { return false };
        let h = handle.lock().unwrap();
        if h.hibernated.load(Ordering::SeqCst) {
            return true;
        }
        // SAFETY: SIGSTOP is async-signal-safe; child_pid is our own forked
        // child (never a recycled foreign pid during this handle's lifetime).
        unsafe { libc::kill(h.child_pid as i32, libc::SIGSTOP) };
        h.hibernated.store(true, Ordering::SeqCst);
        drop(h);
        drop(map);
        // Drop the cached CPU sample so the first post-resume sample counts as
        // "active" (a stale sample would keep cpu_idle_secs high right after resume).
        self.cpu_samples.lock().unwrap().remove(id);
        true
    }

    /// GAP-7.3: SIGCONT a hibernated session's child (resume). Idempotent.
    /// Refreshes the idle timestamps so a freshly-resumed session isn't
    /// immediately re-judged idle. Returns `false` if the session is unknown.
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
