//! Core RPC primitives + daemon spawn logic (ensure_daemon, spawn, staleness).

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::local_socket::{connect as local_connect, LocalStream};

use pureterm_jsonrpc::{JsonRpcRequest, JsonRpcResponse};

/// Send a one-shot JSON-RPC request to the daemon (5s read/write timeout) and
/// return the response.
pub fn rpc(
    socket: &Path,
    method: &str,
    params: Option<serde_json::Value>,
) -> io::Result<JsonRpcResponse> {
    rpc_with_timeout(socket, method, params, Duration::from_secs(5))
}

/// Same as `rpc` with a caller-chosen per-read/write timeout. The default 5s
/// suits real RPCs, but liveness probes (`ping_outcome`/`poll_daemon_ready`)
/// must bound well below it: a *wedged* daemon (listener still accepts, RPC
/// loop dead) otherwise blocks each probe for the full timeout and — in the
/// spawn-poll loop — wedged cold launch on "Starting PureTerm…" for minutes.
fn rpc_with_timeout(
    socket: &Path,
    method: &str,
    params: Option<serde_json::Value>,
    timeout: Duration,
) -> io::Result<JsonRpcResponse> {
    let stream = local_connect(socket)?;
    stream.set_write_timeout(Some(timeout))?;
    stream.set_read_timeout(Some(timeout))?;

    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let request = match params {
        Some(p) => JsonRpcRequest::with_params(id, method, p),
        None => JsonRpcRequest::new(id, method),
    };

    let mut json = serde_json::to_string(&request).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    json.push('\n');

    let mut stream = stream;
    stream.write_all(json.as_bytes())?;
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut buf = String::new();
    BufReader::new(&stream).read_line(&mut buf)?;
    if buf.trim().is_empty() {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "no response from daemon"));
    }
    serde_json::from_str::<JsonRpcResponse>(buf.trim())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Ping the daemon. Returns `true` if it is reachable.
pub fn ping(socket: &Path) -> bool {
    rpc(socket, "daemon.ping", None)
        .map(|r| r.error.is_none())
        .unwrap_or(false)
}

/// Outcome of a short-timeout liveness probe. Distinguishes *nothing there*
/// (`ConnectRefused` — normal cold start, keep polling) from *something there
/// but dead* (`Wedged` — listener accepts but the RPC loop never responds).
/// `Wedged` exists so callers can bail fast instead of looping for minutes:
/// the original `ping` collapsed both into `false`, so the spawn-poll loop
/// re-probed a wedged socket 100× with a 5s read timeout each (~8min hang).
#[derive(Debug, PartialEq, Eq)]
enum PingOutcome {
    /// Connected and got a timely JSON-RPC response — the daemon is alive.
    Responsive,
    /// Nothing listening at the socket path (connect refused / no entry).
    ConnectRefused,
    /// Socket accepts connections but never responds — the daemon RPC loop is
    /// wedged/dead.
    Wedged,
}

/// Probe the daemon with a short timeout, classifying the outcome. Used by
/// `ensure_daemon` and `poll_daemon_ready` where bounding wait time matters.
fn ping_outcome(socket: &Path, timeout: Duration) -> PingOutcome {
    let mut stream = match local_connect(socket) {
        Ok(s) => s,
        Err(_) => return PingOutcome::ConnectRefused,
    };
    let _ = stream.set_write_timeout(Some(timeout));
    let _ = stream.set_read_timeout(Some(timeout));

    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut json = match serde_json::to_string(&JsonRpcRequest::new(id, "daemon.ping")) {
        Ok(j) => j,
        Err(_) => return PingOutcome::Wedged, // serialization can't fail for this; defensive
    };
    json.push('\n');

    if stream.write_all(json.as_bytes()).is_err() {
        return PingOutcome::Wedged;
    }
    // half-close the write side so the daemon sees EOF and flushes its reply.
    let _ = stream.shutdown(std::net::Shutdown::Write);

    let mut buf = String::new();
    match BufReader::new(&stream).read_line(&mut buf) {
        // EOF / timeout / any I/O error → no timely response.
        Ok(0) | Err(_) => PingOutcome::Wedged,
        Ok(_) => {
            if buf.trim().is_empty() {
                PingOutcome::Wedged
            } else {
                // Any parseable response means the daemon answered = alive.
                match serde_json::from_str::<JsonRpcResponse>(buf.trim()) {
                    Ok(_) => PingOutcome::Responsive,
                    Err(_) => PingOutcome::Wedged,
                }
            }
        }
    }
}

/// Poll a daemon socket until it becomes responsive, bailing immediately if the
/// listener is wedged. Extracted from `spawn_daemon` so the fast-bail contract
/// is unit-testable without spawning the real binary.
fn poll_daemon_ready(socket: &Path, per_attempt: Duration, attempts: u32) -> io::Result<PathBuf> {
    for _ in 0..attempts {
        match ping_outcome(socket, per_attempt) {
            PingOutcome::Responsive => return Ok(socket.to_path_buf()),
            PingOutcome::Wedged => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "puretermd wedged: socket listening but not responding",
                ));
            }
            PingOutcome::ConnectRefused => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "puretermd did not become reachable",
    ))
}

/// Process-wide cache of the verified daemon socket path. `ensure_daemon` is
/// called once per restored pane (see `terminal_context`), so without this the
/// cold spawn-poll and the staleness RPC would repeat per pane. The cached path
/// is re-pinged on every call (cheap, one local-socket round-trip) and evicted
/// if it ever fails, so a daemon crash self-heals on the next call.
static LIVE_SOCKET: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Ensure the daemon is running (spawn-on-demand).
///
/// Returns the daemon socket path on success. If the daemon is already
/// reachable, first checks whether it is a *stale* binary (older than the
/// local `puretermd`, i.e. the bundle was repackaged) and restarts it so new
/// code actually runs. Otherwise spawns `puretermd` as a detached child and
/// polls until the socket becomes reachable (up to ~5s).
pub fn ensure_daemon() -> io::Result<PathBuf> {
    // Fast path: a previously verified socket that still responds. The lock is
    // held only to clone the path out — never across ping/spawn (which would
    // serialize callers). Short probe timeout so a daemon that later wedges is
    // detected in milliseconds, not the 5s `rpc` read timeout.
    let cached: Option<PathBuf> = LIVE_SOCKET.lock().unwrap().clone();
    if let Some(p) = &cached {
        if matches!(ping_outcome(p, Duration::from_millis(300)), PingOutcome::Responsive) {
            return Ok(p.clone());
        }
    }

    let socket = pureterm_daemon::discovery::socket_path();
    match ping_outcome(&socket, Duration::from_millis(300)) {
        PingOutcome::Responsive => {
            // Reachable, but possibly a stale binary from before a repackage.
            // Restart when safe so new code runs; a failure here is non-fatal.
            if let Err(e) = restart_if_stale(&socket) {
                log::warn!("daemon staleness check failed: {e}");
            }
        }
        PingOutcome::Wedged => {
            // The socket is held by a daemon that accepts connections but never
            // responds (dead RPC loop). Spawning a fresh `puretermd` cannot help
            // — it can't acquire the instance lock / rebind while the wedged one
            // lives — so surface an error and let callers (`new_local_auto`)
            // fall back to a direct local PTY. Full function (session-survival)
            // recovers via `pkill puretermd` + relaunch.
            log::warn!(
                "puretermd appears wedged (listening, not responding); using direct PTY \
                 fallback. Run `pkill puretermd` and relaunch to recover."
            );
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "puretermd wedged: socket listening but not responding",
            ));
        }
        PingOutcome::ConnectRefused => {
            spawn_daemon(&socket)?;
        }
    }
    // Here the daemon is up — either it was already warm, restarted, or freshly
    // spawned (spawn_daemon only returns Ok after its poll confirms a ping).
    *LIVE_SOCKET.lock().unwrap() = Some(socket.clone());
    Ok(socket)
}

/// Spawn `puretermd` as a detached child and poll until its socket becomes
/// reachable (up to ~5s).
fn spawn_daemon(socket: &Path) -> io::Result<PathBuf> {
    // Find the puretermd binary. In a .app bundle it lives beside the main
    // binary; otherwise look on PATH.
    let puretermd = find_puretermd();
    log::info!("spawning puretermd: {}", puretermd.display());

    let mut cmd = Command::new(&puretermd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Detach from the GUI's process group so `puretermd` survives GUI exit.
    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    cmd.spawn()?;

    // Poll-connect with a SHORT 300ms per-attempt timeout. Critical: the old
    // loop reused `ping` (5s rpc read timeout), so against a *wedged* daemon
    // (socket still listening but RPC loop dead) each iteration blocked 5s ×
    // 100 = ~8min, wedging cold launch on "Starting PureTerm…". `poll_daemon_ready`
    // bails immediately on the Wedged outcome instead. Healthy cold start is
    // unaffected: ConnectRefused returns instantly, and a booted daemon answers
    // well within 300ms.
    poll_daemon_ready(socket, Duration::from_millis(300), 100)
}

/// If the running daemon is a stale binary (its `exe_mtime` differs from the
/// local `puretermd` mtime, i.e. the bundle was repackaged), terminate it and
/// spawn a fresh daemon so the new code runs. No-op when the daemon is current
/// or when either mtime is unknown.
///
/// Note: a restart terminates the old daemon's PTY sessions. This is only
/// reached right after `ping`, and the mtime check guarantees it only fires
/// when the binary genuinely changed (a repackage), so losing the old sessions
/// is the intended "switch to new code" semantics.
fn restart_if_stale(socket: &Path) -> io::Result<()> {
    let local_mtime = match local_puretermd_mtime() {
        Some(m) => m,
        None => return Ok(()), // can't read local exe mtime → can't judge staleness
    };

    let status = rpc(socket, "daemon.status", None)?;
    let result = status
        .result
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no daemon.status result"))?;
    let sessions = result.get("sessions").and_then(|v| v.as_u64()).unwrap_or(0);
    let pid = result.get("pid").and_then(|v| v.as_u64());

    // Determine staleness. A new daemon always reports `exe_mtime` (even 0 if
    // it couldn't stat its own exe), so an ABSENT field means the running
    // daemon is an older binary → stale. `exe_mtime == 0` (new daemon, stat
    // failed) can't be compared reliably → treat as not-stale to avoid a
    // restart loop.
    let stale_reason = match result.get("exe_mtime") {
        None => Some("predates staleness tracking"),
        Some(v) => match v.as_u64() {
            None | Some(0) => None,
            Some(dm) if dm != local_mtime => Some("binary updated"),
            Some(_) => None,
        },
    };

    let Some(reason) = stale_reason else {
        return Ok(());
    };

    // Stale daemon — but don't destroy live sessions automatically. A repackage
    // is not consent to discard the user's running shells. Only restart when
    // nothing is at stake; otherwise log so the user can restart deliberately
    // (e.g. `pkill puretermd` then relaunch the app).
    if sessions > 0 {
        log::warn!(
            "puretermd is stale ({}) but holds {} live session(s); not auto-restarting \
             to avoid losing them. Restart manually when ready.",
            reason,
            sessions
        );
        return Ok(());
    }

    log::warn!("puretermd is stale ({}); restarting daemon", reason);
    terminate_and_respawn(socket, pid)
}

/// SIGTERM the daemon (so its flock releases), wait for it to drop the socket,
/// then spawn a fresh daemon. `pid` is the old daemon's pid from `daemon.status`.
fn terminate_and_respawn(socket: &Path, pid: Option<u64>) -> io::Result<()> {
    if let Some(pid) = pid {
        // SIGTERM → daemon exits → flock releases → fresh daemon can acquire it.
        unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    }

    // Wait for the old daemon to release the socket/lock (up to ~5s).
    for _ in 0..25 {
        std::thread::sleep(Duration::from_millis(200));
        if !ping(socket) {
            break;
        }
    }

    spawn_daemon(socket).map(|_| ())
}

/// mtime (Unix epoch secs) of the `puretermd` binary the GUI would spawn, or
/// `None` if it can't be read.
fn local_puretermd_mtime() -> Option<u64> {
    let exe = find_puretermd();
    let modified = std::fs::metadata(&exe).ok()?.modified().ok()?;
    modified.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// Locate the `puretermd` binary.
fn find_puretermd() -> PathBuf {
    // 1. Same directory as the current binary (covers .app bundles and cargo build).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("puretermd");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    // 2. Fall back to PATH.
    PathBuf::from("puretermd")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn find_puretermd_returns_path() {
        let p = find_puretermd();
        // It may or may not exist on this machine, but it should not panic.
        assert!(!p.as_os_str().is_empty());
    }

    #[test]
    fn ping_nonexistent_returns_false() {
        assert!(!ping(Path::new("/tmp/nonexistent-puretermd-test.sock")));
    }

    /// A socket that does not exist must classify as `ConnectRefused` (daemon
    /// not up yet — the normal cold-start case that should keep polling).
    #[test]
    fn ping_outcome_connect_refused_for_missing_socket() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("never-bound.sock");
        let outcome = ping_outcome(&sock, Duration::from_millis(300));
        assert_eq!(outcome, PingOutcome::ConnectRefused);
    }

    /// Regression for the cold-launch hang: a *wedged* daemon (listener still
    /// accepts connections, but the RPC loop is dead and never writes a
    /// response) must classify as `Wedged` and — critically — must do so within
    /// roughly the probe timeout, not the 5s `rpc` read timeout × 100 poll
    /// iterations that used to wedge boot for ~8 minutes.
    #[test]
    fn ping_outcome_classifies_wedged_within_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("wedged.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        // accept connections but never respond — models the dead-RPC-loop daemon
        let _acceptor = std::thread::spawn(move || {
            while let Ok((s, _)) = listener.accept() {
                // hold the connection open without writing so the client's read
                // times out rather than seeing EOF
                std::mem::forget(s);
            }
        });

        let start = std::time::Instant::now();
        let outcome = ping_outcome(&sock, Duration::from_millis(300));
        let elapsed = start.elapsed();

        assert_eq!(outcome, PingOutcome::Wedged, "wedged listener must be Wedged");
        assert!(
            elapsed < Duration::from_secs(2),
            "must bail within ~timeout (was ~8min before fix), took {elapsed:?}"
        );
    }

    /// A live server that returns a valid response must classify as `Responsive`.
    #[test]
    fn ping_outcome_responsive_for_live_server() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("live.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if let Ok(mut s) = stream {
                    std::thread::spawn(move || {
                        use std::io::{BufRead, BufReader, Write};
                        let mut line = String::new();
                        if BufReader::new(&s).read_line(&mut line).is_ok() {
                            // valid JSON-RPC response, no `error` → responsive
                            let _ = s.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n");
                        }
                    });
                }
            }
        });

        let outcome = ping_outcome(&sock, Duration::from_secs(1));
        assert_eq!(outcome, PingOutcome::Responsive);
    }

    /// The spawn-poll loop must bail FAST (well under its worst-case ceiling)
    /// when the socket is held by a wedged listener, instead of looping 100×
    /// with a 5s read timeout each. This is the direct regression test for the
    /// ~8-minute cold-launch hang.
    #[test]
    fn poll_daemon_ready_bails_fast_on_wedged() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("wedged-poll.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        std::thread::spawn(move || {
            while let Ok((s, _)) = listener.accept() {
                std::mem::forget(s);
            }
        });

        let start = std::time::Instant::now();
        let res = poll_daemon_ready(&sock, Duration::from_millis(300), 100);
        let elapsed = start.elapsed();

        assert!(res.is_err(), "wedged socket must error, got {res:?}");
        assert!(
            elapsed < Duration::from_secs(2),
            "poll must bail fast on wedged (was ~8min before fix), took {elapsed:?}"
        );
    }
}