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

use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::io::{self, ErrorKind};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::tty;
use polling::{Event, PollMode, Poller};
use pureterm_ssh::{SshCommandBuilder, SshPty};

use crate::discovery;
use crate::frame::{self, frame_type};
use crate::ring_buffer::RingBuffer;

/// Interest keys for the per-session poller.
const LISTENER_KEY: usize = 0;
const MASTER_KEY: usize = 1;
const SUBSCRIBER_KEY: usize = 2;

/// Max bytes of a single OUTPUT/SCROLLBACK frame payload.
const OUTPUT_CHUNK: usize = 4096;

/// A PTY the daemon pumps, abstracted to its master `File` so the same pump
/// loop serves both local (`tty::Pty`) and SSH (`SshPty`) sessions. The daemon
/// works on the raw master fd (read/`ioctl` resize) and detects child exit via
/// master read EOF — it does not use the `EventedPty` trait impls.
trait PumpablePty: Send {
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
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
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
    sessions: Arc<Mutex<std::collections::HashMap<String, Arc<Mutex<SessionHandle>>>>>,
    /// GAP-7.3: last CPU sample per session id, used by `sample_cpu_pass` to
    /// detect CPU-time advancement (the dual idle condition for hibernation).
    cpu_samples:
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
    ) -> io::Result<String> {
        let id = new_session_id();
        let window_size =
            WindowSize { num_lines: rows, num_cols: cols, cell_width: 8, cell_height: 16 };
        tty::setup_env();
        let mut options = tty::Options::default();
        options.env.extend(crate::osc7::prepare_osc7_env(&id));
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
        let pump = spawn_pump(pty, listener, stop.clone(), last_output_at.clone());

        let handle = Arc::new(Mutex::new(SessionHandle {
            id: id.clone(),
            kind: "local".into(),
            child_pid,
            data_socket_path,
            agent_kind,
            last_output_at,
            last_cpu_active_at,
            hibernated,
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

        let mut builder = SshCommandBuilder::new(control_socket, host.to_string());
        if let Some(u) = user {
            builder = builder.with_user(u);
        }
        if let Some(p) = port {
            builder = builder.with_port(p);
        }
        if let Some(k) = identity_file {
            builder = builder.with_identity_file(k);
        }
        let argv = builder.build();

        let (ssh_pty, _stderr) = SshPty::spawn(&argv, Some((cols, rows)), password)?;
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
        let pump = spawn_pump(ssh_pty, listener, stop.clone(), last_output_at.clone());

        let handle = Arc::new(Mutex::new(SessionHandle {
            id: id.clone(),
            kind: "ssh".into(),
            child_pid,
            data_socket_path,
            agent_kind: None,
            last_output_at,
            last_cpu_active_at,
            hibernated,
            stop,
            join: Some(pump),
        }));
        self.sessions.lock().unwrap().insert(id.clone(), handle);
        Ok(id)
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

/// Generate a unique session id (pid + monotonic counter).
fn new_session_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}", std::process::id(), n)
}

fn spawn_pump<P: PumpablePty + Send + 'static>(
    pty: P,
    listener: UnixListener,
    stop: Arc<AtomicBool>,
    last_output_at: Arc<Mutex<Instant>>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("puretermd-session".into())
        .spawn(move || run_pump(pty, listener, stop, last_output_at))
        .expect("spawn session pump")
}

/// Tear down a session's poll registrations. Generic so it works for any
/// `PumpablePty` (local or SSH).
fn cleanup_pump<P: PumpablePty>(
    poll: &Poller,
    pty: &P,
    listener: &UnixListener,
    subscribers: &[Subscriber],
) {
    let _ = poll.delete(listener);
    let _ = poll.delete(pty.master());
    for sub in subscribers {
        let _ = poll.delete(&sub.stream);
    }
}

/// The pump loop. See module docs.
fn run_pump<P: PumpablePty + Send + 'static>(
    pty: P,
    listener: UnixListener,
    stop: Arc<AtomicBool>,
    last_output_at: Arc<Mutex<Instant>>,
) {
    let poll = match Poller::new() {
        Ok(p) => p,
        Err(e) => {
            log::error!("session pump: poller create failed: {e}");
            return;
        }
    };

    let master_fd = pty.master().as_raw_fd();

    if unsafe { poll.add_with_mode(&listener, Event::readable(LISTENER_KEY), PollMode::Level) }.is_err() {
        return;
    }
    if unsafe { poll.add_with_mode(pty.master(), Event::readable(MASTER_KEY), PollMode::Level) }.is_err() {
        let _ = poll.delete(&listener);
        return;
    }

    // GAP-5.1 M5: multi-attach fan-out. Every attached GUI client is a
    // `Subscriber` with its own outbound frame queue, so several tabs/windows
    // can share one session. The ring is the scrollback source replayed as
    // SCROLLBACK frames on each new attach; live output is broadcast as OUTPUT
    // frames to every subscriber's queue. A lagging subscriber whose queue
    // exceeds `SUB_BACKPRESSURE_CAP` is dropped rather than blocking the pump.
    let mut subscribers: Vec<Subscriber> = Vec::new();
    let mut ring = RingBuffer::default();
    let mut events = polling::Events::with_capacity(std::num::NonZeroUsize::new(64).unwrap());
    let mut buf = [0u8; 8192];
    let mut sub_scratch = [0u8; 8192];

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        events.clear();
        match poll.wait(&mut events, Some(std::time::Duration::from_millis(250))) {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => {
                log::error!("session pump: poll error: {e}");
                break;
            }
        }

        for ev in events.iter() {
            match ev.key {
                LISTENER_KEY => {
                    // Multi-attach: accept every pending connection.
                    while let Ok((stream, _)) = listener.accept() {
                        let _ = stream.set_nonblocking(true);
                        if unsafe {
                            poll.add_with_mode(
                                &stream,
                                Event::readable(SUBSCRIBER_KEY),
                                PollMode::Level,
                            )
                        }
                        .is_ok()
                        {
                            let mut sub = Subscriber { stream, queue: VecDeque::new(), off: 0 };
                            // M2: replay scrollback history as SCROLLBACK frames.
                            for wire in scrollback_frames(&ring) {
                                sub.queue.push_back(wire);
                            }
                            subscribers.push(sub);
                        }
                    }
                }
                MASTER_KEY => match read_master(master_fd, &mut buf) {
                    MasterRead::Data(n) => {
                        ring.push(&buf[..n]);
                        broadcast_output(&mut subscribers, &buf[..n]);
                        flush_subscribers(&mut subscribers);
                        // GAP-7.3: record PTY activity for idle detection.
                        if let Ok(mut t) = last_output_at.lock() {
                            *t = Instant::now();
                        }
                    }
                    MasterRead::Eof => {
                        enqueue_exit_all(&mut subscribers, 0);
                        flush_subscribers(&mut subscribers);
                        cleanup_pump(&poll, &pty, &listener, &subscribers);
                        return;
                    }
                    MasterRead::WouldBlock => {}
                },
                SUBSCRIBER_KEY => {
                    // Shared key: drain input from every subscriber, drop the
                    // closed/errored ones, then flush any newly-writable queues.
                    let mut to_drop: Vec<usize> = Vec::new();
                    for (i, sub) in subscribers.iter_mut().enumerate() {
                        loop {
                            match handle_subscriber_read(
                                sub.stream.as_raw_fd(),
                                master_fd,
                                &mut sub_scratch,
                            ) {
                                ReadOutcome::Closed => {
                                    to_drop.push(i);
                                    break;
                                }
                                ReadOutcome::WouldBlock => break,
                                ReadOutcome::Ok => continue,
                            }
                        }
                    }
                    drop_subscribers(&poll, &mut subscribers, &to_drop);
                    flush_subscribers(&mut subscribers);
                }
                _ => {}
            }
        }

        enforce_backpressure(&poll, &mut subscribers);
        reconcile_writable(&poll, &subscribers);
    }

    cleanup_pump(&poll, &pty, &listener, &subscribers);
}

/// An attached GUI client (GAP-5.1 M5 multi-attach).
struct Subscriber {
    stream: UnixStream,
    /// Outbound frame queue — each entry is one fully-serialized wire frame.
    queue: VecDeque<Vec<u8>>,
    /// Bytes of `queue.front()` already written to the kernel.
    off: usize,
}

impl Subscriber {
    /// Bytes not yet accepted by the kernel (for backpressure accounting).
    fn pending_bytes(&self) -> usize {
        let mut total = 0usize;
        for (i, f) in self.queue.iter().enumerate() {
            total += if i == 0 { f.len().saturating_sub(self.off) } else { f.len() };
        }
        total
    }
}

/// Drop a subscriber whose outbound queue is hopelessly behind (bounded
/// backpressure): the pump never blocks on a stalled client — it disconnects it.
const SUB_BACKPRESSURE_CAP: usize = 2 * 1024 * 1024;

/// Serialize the ring's contents as `SCROLLBACK` wire frames (chunked).
fn scrollback_frames(ring: &RingBuffer) -> Vec<Vec<u8>> {
    ring.snapshot()
        .chunks(OUTPUT_CHUNK)
        .map(|c| frame::encode(frame_type::SCROLLBACK, c))
        .collect()
}

/// Broadcast fresh PTY output to every subscriber as `OUTPUT` frames.
fn broadcast_output(subscribers: &mut [Subscriber], bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let frames: Vec<Vec<u8>> = bytes
        .chunks(OUTPUT_CHUNK)
        .map(|c| frame::encode(frame_type::OUTPUT, c))
        .collect();
    for sub in subscribers.iter_mut() {
        for f in &frames {
            sub.queue.push_back(f.clone());
        }
    }
}

/// Non-blocking flush of every subscriber's queue (partial writes advance the
/// per-subscriber cursor; `WouldBlock` defers to the next writable event).
fn flush_subscribers(subscribers: &mut [Subscriber]) {
    for sub in subscribers.iter_mut() {
        while let Some(front) = sub.queue.front() {
            let remaining = &front[sub.off..];
            let n = match write_fd_nonblock(sub.stream.as_raw_fd(), remaining) {
                Ok(n) => n,
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => break, // peer gone; reaped on the next read pass
            };
            if n == 0 {
                break;
            }
            sub.off += n;
            if sub.off >= front.len() {
                sub.queue.pop_front();
                sub.off = 0;
            }
        }
    }
}

/// Disconnect subscribers whose queued backlog exceeds the cap.
fn enforce_backpressure(poll: &Poller, subscribers: &mut Vec<Subscriber>) {
    let to_drop: Vec<usize> = subscribers
        .iter()
        .enumerate()
        .filter(|(_, s)| s.pending_bytes() > SUB_BACKPRESSURE_CAP)
        .map(|(i, _)| i)
        .collect();
    drop_subscribers(poll, subscribers, &to_drop);
}

/// Remove the given subscriber indices (deregistering each from the poller).
fn drop_subscribers(poll: &Poller, subscribers: &mut Vec<Subscriber>, indices: &[usize]) {
    if indices.is_empty() {
        return;
    }
    let drop_set: HashSet<usize> = indices.iter().copied().collect();
    let mut kept = Vec::with_capacity(subscribers.len());
    for (i, sub) in subscribers.drain(..).enumerate() {
        if drop_set.contains(&i) {
            let _ = poll.delete(&sub.stream);
        } else {
            kept.push(sub);
        }
    }
    *subscribers = kept;
}

/// Keep each subscriber's writable interest in sync with its queue.
fn reconcile_writable(poll: &Poller, subscribers: &[Subscriber]) {
    for sub in subscribers {
        let needs = !sub.queue.is_empty();
        let mut ev = Event::readable(SUBSCRIBER_KEY);
        ev.writable = needs;
        let _ = poll.modify_with_mode(&sub.stream, ev, PollMode::Level);
    }
}

/// Enqueue an `EXIT` frame to every subscriber (sent on master EOF).
fn enqueue_exit_all(subscribers: &mut [Subscriber], code: i32) {
    let wire = frame::encode(frame_type::EXIT, &frame::encode_exit(code));
    for sub in subscribers {
        sub.queue.push_back(wire.clone());
    }
}

/// Single non-blocking `write`; returns bytes written or `WouldBlock`.
fn write_fd_nonblock(fd: RawFd, data: &[u8]) -> io::Result<usize> {
    let n = unsafe { libc::write(fd, data.as_ptr() as *const _, data.len()) };
    if n >= 0 {
        Ok(n as usize)
    } else {
        Err(io::Error::last_os_error())
    }
}

enum MasterRead {
    Data(usize),
    Eof,
    WouldBlock,
}

fn read_master(fd: std::os::fd::RawFd, buf: &mut [u8]) -> MasterRead {
    let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len()) };
    if n >= 0 {
        if n == 0 {
            MasterRead::Eof
        } else {
            MasterRead::Data(n as usize)
        }
    } else {
        let err = io::Error::last_os_error();
        match err.kind() {
            ErrorKind::WouldBlock => MasterRead::WouldBlock,
            _ => {
                #[cfg(target_os = "linux")]
                if err.raw_os_error() == Some(libc::EIO) {
                    return MasterRead::Eof;
                }
                log::warn!("session pump: master read error: {err}");
                MasterRead::Eof
            }
        }
    }
}

/// Outcome of one non-blocking read from a subscriber.
enum ReadOutcome {
    /// Got and processed a chunk; caller may read again.
    Ok,
    /// No data available right now.
    WouldBlock,
    /// Subscriber closed / errored — drop it.
    Closed,
}

/// Read one chunk from a subscriber and dispatch any INPUT/RESIZE/PING frames
/// it carries. `Ok` means there may be more to drain.
fn handle_subscriber_read(
    sub_fd: std::os::fd::RawFd,
    master_fd: std::os::fd::RawFd,
    buf: &mut [u8],
) -> ReadOutcome {
    let n = unsafe { libc::read(sub_fd, buf.as_mut_ptr() as *mut _, buf.len()) };
    if n == 0 {
        return ReadOutcome::Closed;
    }
    if n < 0 {
        let err = io::Error::last_os_error();
        return if err.kind() == ErrorKind::WouldBlock {
            ReadOutcome::WouldBlock
        } else {
            ReadOutcome::Closed
        };
    }
    // Parse all complete frames in this chunk. Incomplete tails wait for more.
    let data = &buf[..n as usize];
    let mut pos = 0;
    while let Ok(Some((frame, consumed))) = frame::decode(&data[pos..]) {
        match frame.ftype {
            frame_type::INPUT => {
                let _ = write_all_fd(master_fd, frame.payload);
            }
            frame_type::RESIZE => {
                if let Ok((cols, rows)) = frame::decode_resize(frame.payload) {
                    resize_pty(master_fd, cols, rows);
                }
            }
            frame_type::PING => {
                let _ = write_all_fd(sub_fd, &frame::encode(frame_type::PONG, &[]));
            }
            _ => {}
        }
        pos += consumed;
    }
    ReadOutcome::Ok
}

fn write_all_fd(fd: std::os::fd::RawFd, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        let n = unsafe { libc::write(fd, data.as_ptr() as *const _, data.len()) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == ErrorKind::WouldBlock {
                std::thread::sleep(std::time::Duration::from_micros(200));
                continue;
            }
            return Err(err);
        }
        data = &data[n as usize..];
    }
    Ok(())
}

fn resize_pty(master_fd: std::os::fd::RawFd, cols: u16, rows: u16) {
    let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    unsafe {
        let _ = libc::ioctl(master_fd, libc::TIOCSWINSZ, &ws);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_unique() {
        let a = new_session_id();
        let b = new_session_id();
        assert_ne!(a, b);
    }

    /// GAP-5.1 M2/M5: `scrollback_frames` serializes the ring as `SCROLLBACK`
    /// wire frames — what a freshly-attached subscriber receives to rebuild
    /// scrollback. (Eviction itself is covered by `ring_buffer` tests.)
    #[test]
    fn scrollback_frames_encode_ring_as_scrollback() {
        let mut ring = RingBuffer::new(1024);
        ring.push(b"the quick brown fox");

        // Concatenate the payloads of every produced frame; assert all SCROLLBACK.
        let mut got = Vec::new();
        for wire in scrollback_frames(&ring) {
            let (frame, _consumed) = next_frame(&wire, 0).expect("one frame per entry");
            assert_eq!(frame.0, frame_type::SCROLLBACK);
            got.extend_from_slice(frame.1);
        }
        assert_eq!(got, b"the quick brown fox");
    }

    /// Minimal frame reader for tests: returns `((ftype, payload), consumed)`.
    fn next_frame(data: &[u8], pos: usize) -> Option<((u8, &[u8]), usize)> {
        if pos + 4 > data.len() {
            return None;
        }
        let ln = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        if pos + 4 + ln > data.len() {
            return None;
        }
        Some(((data[pos + 4], &data[pos + 5..pos + 4 + ln]), 4 + ln))
    }

    #[test]
    fn manager_list_reflects_create_and_kill() {
        let mgr = SessionManager::default();
        assert!(mgr.list().is_empty());
        // create_local spawns a real shell; skip shape-assertions if it fails
        // (e.g. minimal CI without a usable login shell).
        if let Ok(id) = mgr.create_local(80, 24, None, None, None) {
            let infos = mgr.list();
            assert_eq!(infos.len(), 1);
            assert_eq!(infos[0].id, id);
            assert_eq!(infos[0].kind, "local");
            assert!(mgr.kill(&id));
            assert!(mgr.list().is_empty());
        }
    }

    /// GAP-7.3: hibernate SIGSTOPs the session child (flag → true), resume
    /// SIGCONTs it (flag → false), both idempotent, and the child stays alive
    /// throughout (SIGSTOP/SIGCONT never close fds or kill the process).
    #[test]
    fn hibernate_then_resume_round_trip() {
        let mgr = SessionManager::default();
        let id = match mgr.create_local(80, 24, None, None, None) {
            Ok(id) => id,
            Err(_) => return, // no usable login shell in this environment
        };
        let child_pid = mgr.list().into_iter().find(|i| i.id == id).unwrap().child_pid;
        assert!(pid_alive(child_pid), "child alive before hibernate");

        // hibernate → flag true, child still alive (stopped, not killed).
        assert!(mgr.hibernate(&id));
        assert!(mgr.list().iter().find(|i| i.id == id).unwrap().hibernated);
        assert!(pid_alive(child_pid), "child still alive after SIGSTOP");

        // resume → flag false, child still alive.
        assert!(mgr.resume(&id));
        assert!(!mgr.list().iter().find(|i| i.id == id).unwrap().hibernated);
        assert!(pid_alive(child_pid), "child still alive after SIGCONT");

        // idempotent: hibernate/resume on the same state don't fail.
        assert!(mgr.hibernate(&id));
        assert!(mgr.hibernate(&id));
        assert!(mgr.resume(&id));
        assert!(mgr.resume(&id));

        let _ = mgr.kill(&id);
    }

    /// GAP-5.1 M1 core proof: a session's child shell survives the GUI
    /// (subscriber) disconnecting, because the master fd is owned by the pump
    /// thread inside the `SessionHandle` — not by the subscriber. Dropping the
    /// subscriber must NOT close the master, so the child keeps running and a
    /// fresh subscriber can re-attach.
    #[test]
    fn local_session_child_survives_subscriber_drop() {
        let mgr = SessionManager::default();
        let id = match mgr.create_local(80, 24, None, None, None) {
            Ok(id) => id,
            Err(_) => return, // no usable login shell in this environment
        };
        let (child_pid, data_path) = {
            let info = mgr.list().into_iter().find(|i| i.id == id).unwrap();
            (info.child_pid, PathBuf::from(&info.data_path))
        };

        // Sanity: child alive right after creation.
        assert!(pid_alive(child_pid), "child not alive after create");

        // Attach a subscriber (mimics the GUI's DaemonPty) and drain the banner.
        {
            let mut sub = UnixStream::connect(&data_path).expect("connect data socket");
            sub.set_read_timeout(Some(std::time::Duration::from_secs(1))).unwrap();
            let mut buf = [0u8; 8192];
            let _ = std::io::Read::read(&mut sub, &mut buf);
            // `sub` drops here → the subscriber connection closes (GUI exits).
        }

        // Let the pump notice the closed subscriber (poll wait ≤ 250ms).
        std::thread::sleep(std::time::Duration::from_millis(600));

        // CORE ASSERTION: the child shell is still alive after the subscriber
        // (GUI) dropped. If this fails, the master fd was closed on detach.
        assert!(
            pid_alive(child_pid),
            "child died after subscriber dropped — master fd closed prematurely"
        );

        // A fresh subscriber can re-attach to the still-live session.
        {
            let sub = UnixStream::connect(&data_path).expect("re-attach data socket");
            drop(sub);
        }

        assert!(mgr.kill(&id));
    }

    /// Authoritative liveness check via `ps` (kill(pid,0) can't distinguish a
    /// live child from a not-yet-reaped zombie; `ps` reads kernel state).
    fn pid_alive(pid: u32) -> bool {
        let out = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "stat="])
            .stdout(std::process::Stdio::piped())
            .output();
        match out {
            Ok(o) => {
                let stat = String::from_utf8_lossy(&o.stdout).trim().to_string();
                !stat.is_empty() && !stat.starts_with('Z')
            }
            Err(_) => false,
        }
    }
}
