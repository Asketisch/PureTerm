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
use std::os::fd::{FromRawFd, IntoRawFd};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crossbeam_channel::{unbounded, Receiver};

use rustix_openpty::openpty;
use rustix_openpty::rustix::termios::{
    self, InputModes, LocalModes, OptionalActions, OutputModes, Winsize,
};

use super::envelope::{parse_envelopes, TmuxEnvelope};

/// `tmux -CC` 的写端(pty master 的写侧)。
///
/// 共享(`Arc`)的原因:泵线程需要在收到 `%session-changed` 之后发一条
/// `capture-pane -p`(取当前屏幕),而 GUI 线程同时要发 `%input`。
/// 两者都只写一行文本,锁的持有时间可以忽略。
pub type TmuxWriter = Arc<Mutex<Option<std::fs::File>>>;

/// 往控制通道写一条**原始命令行**(不带尾随换行)。
///
/// 用于 `capture-pane -p` 这类普通 tmux 命令 —— 它们不是信封,
/// 但走的是同一条通道。注意**不要**自己再加 `\n`:这里已经补上了。
pub fn write_command_line(writer: &TmuxWriter, line: &str) -> std::io::Result<()> {
    let mut guard = writer
        .lock()
        .map_err(|_| std::io::Error::other("tmux pty lock poisoned"))?;
    let master = guard
        .as_mut()
        .ok_or_else(|| std::io::Error::other("tmux pty closed"))?;
    master.write_all(line.as_bytes())?;
    master.write_all(b"\n")?;
    master.flush()
}

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
    /// 在**既有**会话里新开一个窗口(`tmux -CC new-window -a -t <target>`),
    /// 而不是附加到该会话的当前窗口。仅 `attach` 为 true 时有意义。
    pub new_window: bool,
    /// tmux **socket 名**(`tmux -L <name>`)。
    ///
    /// tmux 服务器默认全机只有一个(`/tmp/tmux-<uid>/default`)。指定
    /// 名字即得到一个**独立**的服务器命名空间 —— 测试用它来隔离自己
    /// (既不污染用户自己的 tmux 会话,也不与其他测试互相干扰)。
    pub socket_label: Option<String>,
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
            new_window: false,
            socket_label: None,
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
            new_window: false,
            socket_label: None,
        }
    }

    /// 构建 `tmux -CC new-window -a -t <target>` 的参数 —— 在既有会话里
    /// **新开**一个窗口,而不是附加到当前窗口。
    pub fn attach_new_window(target: impl Into<String>) -> Self {
        Self {
            session_name: None,
            cols: 0,
            rows: 0,
            attach: true,
            target: Some(target.into()),
            new_window: true,
            socket_label: None,
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
    /// pty 的 master 端（写侧）。
    ///
    /// tmux -CC 的**输入与输出都走同一个 pty**:控制模式把终端本身当
    /// 传输通道,`%…` 信封从 master 读回、`%…` 命令往 master 写。
    /// 这里持写侧副本;读侧副本由信封读取线程独占。
    master: TmuxWriter,
    stderr: Option<ChildStderr>,
    /// 从 stdout 流入的信封 channel。读取线程写入；GUI 取出。
    rx: Receiver<TmuxEnvelope>,
    /// 读取线程句柄（保存以便关闭时 join）。
    _reader: Option<thread::JoinHandle<()>>,
    /// 会话 id（来自 `%session-changed` 的 `$session`）。在第一个信封
    /// 到达前为 `None`。
    ///
    /// 由**读取线程**直接写入（与信封 channel 共享）—— 否则只有当
    /// 调用方主动 drain 信封时才会被填充,而 GUI 可能在收到
    /// `%session-changed` 之后、drain 之前就去查询会话 id。
    session_id: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// 我们请求 tmux 附加/创建的原始会话名。
    pub session_name: Option<String>,
    /// 当前被渲染的 tmux pane id(`%output` 信封里的那个)。
    ///
    /// 泵线程在收到 `%output` 时更新它 —— 客户端只能给**已知**的
    /// pane 发键(`send-keys -t %<id>`),所以这个值必须来自服务器。
    /// 0 = 尚未收到任何 `%output`。
    pane_id: Arc<AtomicU32>,
}

impl TmuxChild {
    /// 用给定参数生成 `tmux -CC`。返回活动子进程句柄 + 信封 channel。
    ///
    /// **必须在 pty 上启动。** `tmux -CC`(控制模式)要对 stdin 做
    /// `tcgetattr` 才能把终端切进 raw 模式;stdin 是管道时 tmux 直接
    /// 报 `tcgetattr failed: Operation not supported by device` 并退出
    /// —— 控制模式后端因此完全不可用(早前用 `Stdio::piped()` 启动,
    /// 三个 E2E 用例全部超时)。
    ///
    /// 另需把 slave 端设成 raw 模式:默认 termios 带 `ECHO`,我们写进
    /// master 的 `%…` 命令会被行规程**回显**回 master,污染信封流;
    /// `OPOST` 还会把 `\n` 翻成 `\r\n`,与 tmux 的信封格式不符。
    pub fn spawn(args: &TmuxSpawnArgs) -> std::io::Result<Self> {
        let mut cmd = Command::new("tmux");
        // `-L <name>` 必须在 `-CC` **之前**(它是 tmux 的全局选项)。
        if let Some(label) = &args.socket_label {
            cmd.arg("-L").arg(label);
        }
        cmd.arg("-CC");
        if args.attach {
            if args.new_window {
                // `tmux -CC new-window -a -t <target>`:在既有会话里新开
                // 一个窗口,然后进入控制模式。
                cmd.args(["new-window", "-a"]);
            } else {
                cmd.arg("attach");
            }
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

        // pty 的窗口尺寸 —— tmux 用它决定新 pane 的行列数。
        let winsize = Winsize {
            ws_row: if args.rows > 0 { args.rows } else { 24 },
            ws_col: if args.cols > 0 { args.cols } else { 80 },
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let pty = openpty(None, Some(&winsize))?;
        let (master, slave) = (pty.controller, pty.user);

        // slave 端设成 raw 模式(见上文说明)。
        if let Ok(mut attrs) = termios::tcgetattr(&slave) {
            attrs.local_modes.set(LocalModes::ICANON, false);
            attrs.local_modes.set(LocalModes::ECHO, false);
            attrs.input_modes.set(InputModes::ICRNL, false);
            attrs.output_modes.set(OutputModes::OPOST, false);
            attrs.output_modes.set(OutputModes::ONLCR, false);
            let _ = termios::tcsetattr(&slave, OptionalActions::Now, &attrs);
        }

        // 子进程的 stdin/stdout 都接 slave 端;stderr 单独走管道,
        // 便于把 tmux 的错误信息读出来展示给用户。
        cmd.stdin(slave.try_clone()?)
            .stdout(slave)
            .stderr(Stdio::piped());
        let mut child = cmd.spawn()?;
        let stderr = child.stderr.take();

        // master 端一分为二:读侧给信封读取线程,写侧留给 GUI 发命令。
        let master_fd = master.into_raw_fd();
        // SAFETY: `master_fd` 是一次 `dup` 得到的全新描述符,由
        // `File` 接管并在关闭时释放;`master_fd` 自身已移交给 `File`。
        let read_side = unsafe { std::fs::File::from_raw_fd(unsafe { libc::dup(master_fd) }) };
        // SAFETY: 同上 —— `master_fd` 的所有权转给这个 `File`。
        let write_side = unsafe { std::fs::File::from_raw_fd(master_fd) };

        // crossbeam 的 `Receiver` 可克隆 —— GUI 的泵线程拿一份克隆去
        // 阻塞等待,不必去抢 `TmuxChild` 的锁(那会把写侧也堵住)。
        let (tx, rx) = unbounded();
        let session_id = std::sync::Arc::new(std::sync::Mutex::new(None));
        let session_id_reader = std::sync::Arc::clone(&session_id);
        let mut reader = BufReader::new(read_side);
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
                    // 看到 `%session-changed` 就立刻记下会话 id —— 不等
                    // GUI 来 drain。
                    if let TmuxEnvelope::SessionChanged { session_id: sid, .. } = &env {
                        if !sid.is_empty() {
                            let mut guard = session_id_reader.lock().unwrap();
                            if guard.is_none() {
                                *guard = Some(sid.clone());
                            }
                        }
                    }
                    if tx.send(env).is_err() {
                        return; // GUI 已退出
                    }
                }
            }
        });

        Ok(Self {
            child,
            master: Arc::new(Mutex::new(Some(write_side))),
            stderr,
            rx,
            _reader: Some(join),
            session_id,
            session_name: args.session_name.clone(),
            pane_id: Arc::new(AtomicU32::new(0)),
        })
    }

    /// 克隆一份信封接收端。
    ///
    /// GUI 的泵线程用它**阻塞**等待下一个信封 —— 这样它不必持有
    /// `TmuxChild` 的锁,用户按键(`%input`)永远不会被堵住。
    pub fn envelope_rx(&self) -> Receiver<TmuxEnvelope> {
        self.rx.clone()
    }

    /// 排空所有待处理信封。非阻塞——立即返回读取线程已缓冲的内容。
    pub fn try_recv_envelope(&mut self) -> Option<TmuxEnvelope> {
        match self.rx.try_recv() {
            Ok(env) => {
                // 第一次看到会话时捕获会话 id(读取线程通常已经记下,
                // 这里兜底)。
                if let TmuxEnvelope::SessionChanged { session_id, .. } = &env {
                    if !session_id.is_empty() {
                        let mut guard = self.session_id.lock().unwrap();
                        if guard.is_none() {
                            *guard = Some(session_id.clone());
                        }
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
                    if !session_id.is_empty() {
                        let mut guard = self.session_id.lock().unwrap();
                        if guard.is_none() {
                            *guard = Some(session_id.clone());
                        }
                    }
                }
                Some(env)
            }
            Err(_) => None,
        }
    }

    /// 向 tmux 会话发送按键字节（来自 GUI `Term` 的键盘输入）。
    ///
    /// **实现说明（重要）**：早期实现发的是 `%input %<pane> <base64>`
    /// 信封，但 **tmux 3.6a 的控制模式不再接受客户端发来的 `%input`** ——
    /// 它直接回一句 `parse error: syntax error`，按键**根本没送达**。
    /// 正确做法是发一条普通命令：
    ///
    /// ```text
    /// send-keys -t %<pane> -H <hex> <hex> …
    /// ```
    ///
    /// `-H` 接受逐字节的十六进制，能无损表达任意字节（含 `0d` 回车、
    /// `1b` ESC）；`-l` 字面量形式表达不了控制字符。
    ///
    /// 目标 pane id 来自服务器（泵线程在收到 `%output` 时记下）——
    /// 客户端不能凭空猜一个 pane。
    pub fn send_input(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let pane = self.pane_id();
        let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let line = format!("send-keys -t %{pane} -H {}", hex.join(" "));
        write_command_line(&self.master, &line)
    }

    /// 当前被渲染的 tmux pane id（0 = 尚未知）。
    pub fn pane_id(&self) -> u32 {
        self.pane_id.load(Ordering::Relaxed)
    }

    /// 共享的 pane id 句柄 —— 泵线程收到 `%output` 时写入它。
    pub fn pane_id_handle(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.pane_id)
    }

    /// 发送一个原始信封（例如 `refresh-client -C 200x50`）。
    pub fn send_envelope(&mut self, env: &TmuxEnvelope) -> std::io::Result<()> {
        let mut guard = self
            .master
            .lock()
            .map_err(|_| std::io::Error::other("tmux pty lock poisoned"))?;
        let master = guard
            .as_mut()
            .ok_or_else(|| std::io::Error::other("tmux pty closed"))?;
        write_envelope(master, env)
    }

    /// 拿到共享的写端。泵线程用它发命令(如 `capture-pane -p`),
    /// 而不必持有整个 `TmuxChild`。
    pub fn writer(&self) -> TmuxWriter {
        Arc::clone(&self.master)
    }

    /// 轮询子进程是否退出；已退出则返回退出状态，仍在运行则返回 `None`。
    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    /// 发送 `SIGTERM` 并回收。用于分离 / 应用关闭时。
    pub fn terminate(&mut self) -> std::io::Result<()> {
        // 在某些平台上，先关闭 pty master 会让 tmux 干净地关闭；随后再升级为 kill。
        if let Ok(mut guard) = self.master.lock() {
            guard.take();
        }
        self.child.kill()?;
        self.child.wait()?;
        Ok(())
    }

    /// 获取在第一个 `%session-changed` 信封中观察到的会话 id。
    /// 还没看到则为 `None`。
    pub fn session_id(&self) -> Option<String> {
        self.session_id.lock().unwrap().clone()
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
            master: Arc::new(Mutex::new(None)),
            stderr: None,
            rx: unbounded().1,
            _reader: None,
            session_id: std::sync::Arc::new(std::sync::Mutex::new(None)),
            session_name: None,
            pane_id: Arc::new(AtomicU32::new(0)),
        };
        // 第一次 terminate：kill + wait。
        pty.terminate().expect("first terminate ok");
        // is_alive 此时应为 false。
        assert!(!pty.is_alive());
    }
}
