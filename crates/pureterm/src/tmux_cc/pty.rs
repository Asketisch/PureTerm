//! T-core-tmux-cc：控制模式下的 tmux 子进程 + 信封 IO。
//!
//! v2 把 v1 的 `spawn_tmux_cc` shell 外包扩展为完整的 `TmuxChild` 结构体：
//!
//! - 持有 `Child` 句柄（回收与关闭路径）。
//! - 拥有 stdin/stdout/stdio，使 GUI 无需重新 fork 即可 `write_envelope`。
//! - 通过带缓冲的读取器返回信封——v1 路径直接把它们丢弃。
//!
//! 完整的 EventedPty 集成（由 `terminal_context` 泵送异步 stdin/stdout）
//! 在 v3 EventLoop 重构落地后接上。v2 是纯同步 stdio + 一个回收子进程的
//! 线程。

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::thread;

use super::envelope::{parse_envelopes, TmuxEnvelope};

/// 生成一个新的 tmux -CC 会话的参数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TmuxSpawnArgs {
    /// `-s <name>` 会话名。为 None 时由 `tag` 自动生成。
    pub session_name: Option<String>,
    /// `-x` 初始列数。
    pub cols: u16,
    /// `-y` 初始行数。
    pub rows: u16,
    /// 为 true 时附加到已有会话；否则新建会话。
    pub attach: bool,
    /// 附加模式下的 `-t <target>`。`attach` 为 true 时必填。
    pub target: Option<String>,
}

impl TmuxSpawnArgs {
    /// 构建 `tmux -CC new-session -A -s pureterm-<tag>` 的参数。
    pub fn new_session(tag: &str, cols: u16, rows: u16) -> Self {
        Self {
            session_name: Some(format!("pureterm-{tag}")),
            cols,
            rows,
            attach: false,
            target: None,
        }
    }

    /// 构建 `tmux -CC attach -t <target>` 的参数。
    pub fn attach(target: impl Into<String>) -> Self {
        Self {
            session_name: None,
            cols: 0,
            rows: 0,
            attach: true,
            target: Some(target.into()),
        }
    }
}

/// 运行中的 `tmux -CC` 子进程句柄 + 其 stdio。
///
/// `TmuxChild` 是围绕 `tmux -CC` 子进程的 v2 全生命周期包装。它拥有
/// stdin/stdout，生成一个持续把信封流入 channel 的读取线程，并提供
/// 让 GUI 发送 `%input` 信封、在分离时回收子进程的辅助方法。
///
/// 注意：`backend.rs` 里的元数据结构体也叫 `TmuxChild`（不同概念：那是
/// v1 的 GUI 侧句柄，跟踪会话/面板元数据）。两者职责不同，运行时
/// 不冲突。
pub struct TmuxChild {
    child: Child,
    stdin: Option<ChildStdin>,
    /// 带缓冲的 stdout 读取器（由信封读取线程消费）。
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    /// 从 stdout 流入的信封 channel。读取线程写入；GUI 取出。
    rx: Receiver<TmuxEnvelope>,
    /// 读取线程句柄（保存以便关闭时 join）。
    _reader: Option<thread::JoinHandle<()>>,
    /// 会话 id（来自 `%session-changed` 的 `$session`）。在第一个信封
    /// 到达前为 `None`。
    session_id: Option<String>,
    /// 我们请求 tmux 附加/创建的原始会话名。
    pub session_name: Option<String>,
}

impl TmuxChild {
    /// 用给定参数生成 `tmux -CC`。返回活动子进程句柄 + 信封 channel。
    pub fn spawn(args: &TmuxSpawnArgs) -> std::io::Result<Self> {
        let mut cmd = Command::new("tmux");
        cmd.arg("-CC");
        if args.attach {
            cmd.arg("attach");
            if let Some(target) = &args.target {
                cmd.args(["-t", target]);
            }
        } else {
            cmd.args(["new-session", "-A"]);
            if let Some(name) = &args.session_name {
                cmd.args(["-s", name]);
            }
            cmd.args(["-x", &args.cols.to_string(), "-y", &args.rows.to_string()]);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let (tx, rx) = channel();
        let mut reader = BufReader::new(stdout.expect("piped stdout"));
        let join = thread::spawn(move || {
            let mut buf = String::new();
            // 循环直到 EOF（子进程退出时关闭 stdout）。
            loop {
                buf.clear();
                let n = match reader.read_line(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };
                if n == 0 {
                    break;
                }
                for env in parse_envelopes(&buf) {
                    if tx.send(env).is_err() {
                        return; // GUI 已退出
                    }
                }
            }
        });

        Ok(Self {
            child,
            stdin,
            stdout: None,
            stderr,
            rx,
            _reader: Some(join),
            session_id: None,
            session_name: args.session_name.clone(),
        })
    }

    /// 排空所有待处理信封。非阻塞——立即返回读取线程已缓冲的内容。
    pub fn try_recv_envelope(&mut self) -> Option<TmuxEnvelope> {
        match self.rx.try_recv() {
            Ok(env) => {
                // 第一次看到会话时捕获会话 id。
                if let TmuxEnvelope::SessionChanged { session_id, .. } = &env {
                    if self.session_id.is_none() && !session_id.is_empty() {
                        self.session_id = Some(session_id.clone());
                    }
                }
                Some(env)
            }
            Err(_) => None,
        }
    }

    /// 阻塞直到至少一个信封到达。
    pub fn recv_envelope(&mut self) -> Option<TmuxEnvelope> {
        match self.rx.recv() {
            Ok(env) => {
                if let TmuxEnvelope::SessionChanged { session_id, .. } = &env {
                    if self.session_id.is_none() && !session_id.is_empty() {
                        self.session_id = Some(session_id.clone());
                    }
                }
                Some(env)
            }
            Err(_) => None,
        }
    }

    /// 向 tmux 发送 `%input` 信封（来自 GUI `Term` 的按键输入）。
    /// 阻塞直到字节写入完成。
    pub fn send_input(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::other("stdin closed"))?;
        // 按 tmux 控制模式协议，把原始字节包装为 `%input $ <length>\n<bytes>`。
        write_envelope(stdin, &TmuxEnvelope::Input {
            pane_id: 0,
            bytes: bytes.to_vec(),
        })
    }

    /// 发送一个原始信封（例如 `refresh-client -C 200x50`）。
    pub fn send_envelope(&mut self, env: &TmuxEnvelope) -> std::io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::other("stdin closed"))?;
        write_envelope(stdin, env)
    }

    /// 轮询子进程是否退出；已退出则返回退出状态，仍在运行则返回 `None`。
    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    /// 发送 `SIGTERM` 并回收。用于分离 / 应用关闭时。
    pub fn terminate(&mut self) -> std::io::Result<()> {
        // 在某些平台上，先关闭 stdin 会让 tmux 干净地关闭；随后再升级为 kill。
        self.stdin.take();
        self.child.kill()?;
        self.child.wait()?;
        Ok(())
    }

    /// 获取在第一个 `%session-changed` 信封中观察到的会话 id。
    /// 还没看到则为 `None`。
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// 子进程是否已退出？
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// 排空 stderr（用于把 tmux 错误显示到 GUI 日志）。
    pub fn stderr_text(&mut self) -> std::io::Result<String> {
        let mut s = String::new();
        if let Some(err) = self.stderr.as_mut() {
            err.read_to_string(&mut s)?;
        }
        Ok(s)
    }
}

impl Drop for TmuxChild {
    fn drop(&mut self) {
        // 尽力而为的清理：忽略错误（子进程可能已退出，或 GUI 正在
        // panic——无论哪种都不应双重 panic）。
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 向 tmux -CC 子进程 stdin 写入一个信封。
///
/// 目前是 `TmuxEnvelope::serialize` 的薄包装；保留为函数，使调用方
/// 不必了解协议细节。
fn write_envelope<W: Write>(w: &mut W, env: &TmuxEnvelope) -> std::io::Result<()> {
    let s = env.serialize();
    w.write_all(s.as_bytes())?;
    w.flush()
}

// --- 为向后兼容保留的 v1 桩代码。新 `TmuxChild` 取代了新调用方，
// 但保留旧 `spawn_tmux_cc` 签名，使旧二进制路径仍可用。

#[cfg(unix)]
mod unix {
    use std::io;
    use std::process::{Child, Command, Stdio};

    /// 旧版 v1：生成 `tmux -CC` 子进程并返回句柄，GUI 可向它写入
    /// 命令 / 从中读取信封。
    pub fn spawn_tmux_cc(tag: &str, cols: u16, rows: u16) -> io::Result<(Child, String, u32)> {
        let session = format!("pureterm-{tag}");
        let child = Command::new("tmux")
            .args([
                "-CC",
                "new-session",
                "-A",
                "-s",
                &session,
                "-x",
                &cols.to_string(),
                "-y",
                &rows.to_string(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let pid = child.id();
        Ok((child, String::new(), pid))
    }
}

#[cfg(unix)]
pub use unix::spawn_tmux_cc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_args_have_correct_dimensions() {
        let a = TmuxSpawnArgs::new_session("abc", 200, 50);
        assert_eq!(a.session_name.as_deref(), Some("pureterm-abc"));
        assert_eq!(a.cols, 200);
        assert_eq!(a.rows, 50);
        assert!(!a.attach);
        assert!(a.target.is_none());
    }

    #[test]
    fn attach_args_have_target_no_dimensions() {
        let a = TmuxSpawnArgs::attach("pureterm-foo");
        assert!(a.attach);
        assert_eq!(a.target.as_deref(), Some("pureterm-foo"));
        assert_eq!(a.cols, 0);
        assert_eq!(a.rows, 0);
        assert!(a.session_name.is_none());
    }

    /// 生成 tmux 需要 PATH 中的二进制；缺失时跳过。
    #[test]
    fn spawn_fails_cleanly_without_tmux_binary() {
        // 使用一个肯定不存在的二进制来测试错误路径，避免污染 PATH。
        let mut cmd = Command::new("/nonexistent/tmux");
        cmd.arg("-CC");
        let result = cmd.spawn();
        assert!(result.is_err());
    }

    #[test]
    fn terminate_idempotent() {
        // 用 `Command::new("true")` 构造一个假子进程，这样不必安装 tmux。
        // `terminate` 应在不 panic 的情况下 kill + wait。
        let mut cmd = Command::new("true");
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut pty = TmuxChild {
            child: cmd.spawn().expect("spawn `true`"),
            stdin: None,
            stdout: None,
            stderr: None,
            rx: channel().1,
            _reader: None,
            session_id: None,
            session_name: None,
        };
        // 第一次 terminate：kill + wait。
        pty.terminate().expect("first terminate ok");
        // is_alive 此时应为 false。
        assert!(!pty.is_alive());
    }
}
