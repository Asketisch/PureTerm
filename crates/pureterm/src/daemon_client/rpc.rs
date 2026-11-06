//! 核心 RPC 原语 + 守护进程启动逻辑（ensure_daemon、spawn、staleness）。

use std::io::{self, BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::local_socket::connect as local_connect;

use pureterm_jsonrpc::{JsonRpcRequest, JsonRpcResponse};

/// 向守护进程发送一次性的 JSON-RPC 请求（5s 读写超时）并返回响应。
pub fn rpc(
    socket: &Path,
    method: &str,
    params: Option<serde_json::Value>,
) -> io::Result<JsonRpcResponse> {
    rpc_with_timeout(socket, method, params, Duration::from_secs(5))
}

/// 与 `rpc` 相同，但每次读/写超时由调用方指定。默认 5s 适合真正的
/// RPC，但存活探测（`ping_outcome`/`poll_daemon_ready`）必须远低于它：
/// 否则 *卡死* 的守护进程（监听器仍接受连接，RPC 循环已死）会让每次
/// 探测阻塞满整个超时——在启动轮询循环中，会让冷启动卡在
/// “正在启动 PureTerm…” 长达数分钟。
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

/// 探测守护进程。若可达返回 `true`。
pub fn ping(socket: &Path) -> bool {
    rpc(socket, "daemon.ping", None)
        .map(|r| r.error.is_none())
        .unwrap_or(false)
}

/// 短超时存活探测的结果。区分 *什么都没有*
/// （`ConnectRefused`——正常的冷启动，继续轮询）与 *有东西但已死*
/// （`Wedged`——监听器接受连接，但 RPC 循环从不响应）。
/// `Wedged` 的存在是为了让调用方快速退出，而不是循环数分钟：
/// 原来的 `ping` 把两者都折叠为 `false`，导致启动轮询循环对卡死的
/// socket 用 5s 读超时轮询 100 次（约 8 分钟挂起）。
#[derive(Debug, PartialEq, Eq)]
enum PingOutcome {
    /// 已连接且及时收到 JSON-RPC 响应——守护进程存活。
    Responsive,
    /// socket 路径上没有监听（连接被拒 / 无条目）。
    ConnectRefused,
    /// socket 接受连接但从不响应——守护进程的 RPC 循环已卡死。
    Wedged,
}

/// 用短超时探测守护进程并对结果分类。供需要限定等待时间的
/// `ensure_daemon` 和 `poll_daemon_ready` 使用。
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
        Err(_) => return PingOutcome::Wedged, // 此处的序列化不会失败;防御性处理
    };
    json.push('\n');

    if stream.write_all(json.as_bytes()).is_err() {
        return PingOutcome::Wedged;
    }
    // 半关闭写侧，使守护进程看到 EOF 并刷新其回复。
    let _ = stream.shutdown(std::net::Shutdown::Write);

    let mut buf = String::new();
    match BufReader::new(&stream).read_line(&mut buf) {
        // EOF / 超时 / 任何 I/O 错误 → 没有及时响应。
        Ok(0) | Err(_) => PingOutcome::Wedged,
        Ok(_) => {
            if buf.trim().is_empty() {
                PingOutcome::Wedged
            } else {
                // 任何可解析的响应都意味着守护进程已应答 = 存活。
                match serde_json::from_str::<JsonRpcResponse>(buf.trim()) {
                    Ok(_) => PingOutcome::Responsive,
                    Err(_) => PingOutcome::Wedged,
                }
            }
        }
    }
}

/// 轮询守护进程 socket 直到它可响应；若监听器卡死则立即退出。从
/// `spawn_daemon` 中提取，使快速退出契约无需启动真实二进制即可
/// 单元测试。
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

/// 已验证守护进程 socket 路径的进程级缓存。`ensure_daemon` 每个恢复的
/// 面板调用一次（见 `terminal_context`），没有它，冷启动轮询和陈旧性
/// RPC 会每个面板重复一遍。缓存路径在每次调用时重新探测（便宜，一次
/// 本地 socket 往返），失败即逐出，因此守护进程崩溃会在下次调用时自愈。
static LIVE_SOCKET: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// 确保守护进程在运行（按需启动）。
///
/// 成功时返回守护进程 socket 路径。若守护进程已可达，先检查它是否是
/// *陈旧* 的二进制（比本地 `puretermd` 旧，即安装包被重新打包过），
/// 并重启它，使新代码真正运行。否则把 `puretermd` 作为分离子进程启动，
/// 并轮询直到 socket 可达（最多约 5s）。
pub fn ensure_daemon() -> io::Result<PathBuf> {
    // 快路径：先前已验证且仍在响应的 socket。只持有锁以克隆路径——
    // 绝不在 ping/spawn 期间持锁（那会让调用方串行化）。短探测超时，使
    // 之后卡死的守护进程能在毫秒级被检测到，而不是 5s `rpc` 读超时。
    let cached: Option<PathBuf> = LIVE_SOCKET.lock().unwrap().clone();
    if let Some(p) = &cached {
        if matches!(ping_outcome(p, Duration::from_millis(300)), PingOutcome::Responsive) {
            return Ok(p.clone());
        }
    }

    let socket = pureterm_daemon::discovery::socket_path();
    match ping_outcome(&socket, Duration::from_millis(300)) {
        PingOutcome::Responsive => {
            // 可达，但可能是重新打包前的陈旧二进制。
            // 安全时重启以运行新代码；这里的失败不是致命的。
            if let Err(e) = restart_if_stale(&socket) {
                log::warn!("daemon staleness check failed: {e}");
            }
        }
        PingOutcome::Wedged => {
            // socket 被一个接受连接但从不响应（RPC 循环已死）的守护进程持有。
            // 启动新的 `puretermd` 也无济于事——卡死的进程存活期间，它无法获得
            // 实例锁 / 重新绑定——所以抛出错误，让调用方（`new_local_auto`）回退
            // 到直接本地 PTY。完整功能（会话存活）通过 `pkill puretermd` + 重新
            // 启动恢复。
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
    // 到这里守护进程已就绪——要么本来就热着，要么被重启，要么刚被
    // 启动（spawn_daemon 只有在轮询确认 ping 成功后才返回 Ok）。
    *LIVE_SOCKET.lock().unwrap() = Some(socket.clone());
    Ok(socket)
}

/// 把 `puretermd` 作为分离子进程启动，并轮询直到其 socket 可达
/// （最多约 5s）。
fn spawn_daemon(socket: &Path) -> io::Result<PathBuf> {
    // 查找 puretermd 二进制。在 .app 包中它与主二进制同目录；
    // 否则在 PATH 中查找。
    let puretermd = find_puretermd();
    log::info!("spawning puretermd: {}", puretermd.display());

    let mut cmd = Command::new(&puretermd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // 从 GUI 的进程组分离，使 `puretermd` 在 GUI 退出后存活。
    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    cmd.spawn()?;

    // 轮询连接，每次尝试用短的 300ms 超时。关键：旧循环复用了 `ping`
    // （5s 的 rpc 读超时），因此面对 *卡死* 的守护进程（socket 仍在监听但
    // RPC 循环已死），每次迭代阻塞 5s × 100 = 约 8 分钟，使冷启动卡在
    // “正在启动 PureTerm…”。`poll_daemon_ready` 改为在 Wedged 结果时立即
    // 退出。健康的冷启动不受影响：ConnectRefused 立即返回，已启动的
    // 守护进程在 300ms 内轻松应答。
    poll_daemon_ready(socket, Duration::from_millis(300), 100)
}

/// 若运行中的守护进程是陈旧二进制（其 `exe_mtime` 与本地 `puretermd`
/// 的 mtime 不同，即安装包被重新打包过），终止它并启动新守护进程，
/// 使新代码运行。守护进程为最新或任一 mtime 未知时为空操作。
///
/// 注意：重启会终止旧守护进程的 PTY 会话。这只在 `ping` 之后立即到达，
/// 且 mtime 检查保证只在二进制真正变化（重新打包）时触发，因此丢失
/// 旧会话正是预期的“切换到新代码”语义。
fn restart_if_stale(socket: &Path) -> io::Result<()> {
    let local_mtime = match local_puretermd_mtime() {
        Some(m) => m,
        None => return Ok(()), // 读不到本地可执行文件的 mtime → 无法判断是否陈旧
    };

    let status = rpc(socket, "daemon.status", None)?;
    let result = status
        .result
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no daemon.status result"))?;
    let sessions = result.get("sessions").and_then(|v| v.as_u64()).unwrap_or(0);
    let pid = result.get("pid").and_then(|v| v.as_u64());

    // 判断陈旧性。新守护进程总是报告 `exe_mtime`（即便无法 stat 自身 exe
    // 时为 0），所以字段缺失意味着运行中的守护进程是更老的二进制 → 陈旧。
    // `exe_mtime == 0`（新守护进程，stat 失败）无法可靠比较 → 视为不陈旧，
    // 以避免重启循环。
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

    // 守护进程已陈旧——但不要自动销毁存活会话。重新打包不等于同意丢弃
    // 用户正在运行的 shell。只有在没有风险时才重启；否则记录日志，让用户
    // 自行决定重启（例如 `pkill puretermd` 后重新启动应用）。
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

/// 向守护进程发送 SIGTERM（使其 flock 释放），等待它释放 socket，
/// 然后启动新的守护进程。`pid` 是来自 `daemon.status` 的旧守护进程 pid。
fn terminate_and_respawn(socket: &Path, pid: Option<u64>) -> io::Result<()> {
    if let Some(pid) = pid {
        // SIGTERM → 守护进程退出 → flock 释放 → 新守护进程可获取它。
        // Windows 无 SIGTERM;守护进程本身在 Windows 是 Unsupported,
        // 故这里直接跳过(后续轮询会超时并返回明确错误)。
        #[cfg(unix)]
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        #[cfg(not(unix))]
        let _ = pid;
    }

    // 等待旧守护进程释放 socket/锁（最多约 5s）。
    for _ in 0..25 {
        std::thread::sleep(Duration::from_millis(200));
        if !ping(socket) {
            break;
        }
    }

    spawn_daemon(socket).map(|_| ())
}

/// GUI 将要启动的 `puretermd` 二进制的 mtime（Unix 纪元秒），
/// 读不到时为 `None`。
fn local_puretermd_mtime() -> Option<u64> {
    let exe = find_puretermd();
    let modified = std::fs::metadata(&exe).ok()?.modified().ok()?;
    modified.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// 定位 `puretermd` 二进制。
fn find_puretermd() -> PathBuf {
    // 1. 与当前二进制同目录（覆盖 .app 包与 cargo build）。
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("puretermd");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    // 2. 回退到 PATH。
    PathBuf::from("puretermd")
}

// 该测试模块用 UnixListener 搭本地 socket 回环,Windows 无该类型。
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn find_puretermd_returns_path() {
        let p = find_puretermd();
        // 它在当前机器上可能存在也可能不存在，但不应 panic。
        assert!(!p.as_os_str().is_empty());
    }

    #[test]
    fn ping_nonexistent_returns_false() {
        assert!(!ping(Path::new("/tmp/nonexistent-puretermd-test.sock")));
    }

    /// 不存在的 socket 必须归类为 `ConnectRefused`（守护进程尚未就绪——
    /// 应继续轮询的正常冷启动情形）。
    #[test]
    fn ping_outcome_connect_refused_for_missing_socket() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("never-bound.sock");
        let outcome = ping_outcome(&sock, Duration::from_millis(300));
        assert_eq!(outcome, PingOutcome::ConnectRefused);
    }

    /// 冷启动挂起的回归测试：*卡死* 的守护进程（监听器仍接受连接，但
    /// RPC 循环已死且从不写入响应）必须归类为 `Wedged`，并且——关键——
    /// 必须在约一个探测超时内完成，而不是曾经让启动卡住约 8 分钟的
    /// 5s `rpc` 读超时 × 100 次轮询。
    #[test]
    fn ping_outcome_classifies_wedged_within_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("wedged.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        // 接受连接但从不响应——模拟 RPC 循环已死的守护进程
        let _acceptor = std::thread::spawn(move || {
            while let Ok((s, _)) = listener.accept() {
                // 保持连接打开但不写入，使客户端读超时而不是看到 EOF
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

    /// 返回有效响应的存活服务器必须归类为 `Responsive`。
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
                            // 有效 JSON-RPC 响应，无 `error` → 有响应
                            let _ = s.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n");
                        }
                    });
                }
            }
        });

        let outcome = ping_outcome(&sock, Duration::from_secs(1));
        assert_eq!(outcome, PingOutcome::Responsive);
    }

    /// 当 socket 被卡死的监听器持有时，启动轮询循环必须快速退出（远低于
    /// 其最坏情况上限），而不是以每次 5s 读超时循环 100 次。这是针对约
    /// 8 分钟冷启动挂起的直接回归测试。
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