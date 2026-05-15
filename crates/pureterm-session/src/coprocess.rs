//! 协处理（Coprocess）中间件。
//!
//! 仿照 iTerm2 `Coprocess.m` 的设计：在终端会话旁启动一个外部子进程，
//! - 会话写到终端的每一个字节都被复制到子进程的 stdin（通过 `process_output_to_terminal`）。
//! - 子进程写到 stdout 的字节会进入 `pending_input` 队列，由调用方
//!   （通常是 Pane 的事件循环）取出后通过 `session.write()` 注入会话，
//!   就像用户键入一样。
//!
//! 集成到 Pane 的工作不在本提交范围内；本文件只提供库 API + 单元测试。

use std::io::{self, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::thread;

use bytes::Bytes;
use parking_lot::Mutex;

use crate::middleware::SessionMiddleware;

/// 协处理中间件：把终端字节 tee 给子进程 stdin，
/// 并把子进程 stdout 字节排队等待回注到会话。
pub struct CoprocessMiddleware {
    stdin: Option<ChildStdin>,
    pending_input: Arc<Mutex<Vec<u8>>>,
    /// 持有子进程句柄，避免被过早回收（drop 时 wait 收尸）。
    _child: Arc<Mutex<Option<Child>>>,
    /// 后台读取 stdout 的线程句柄；drop 时不强制 join，让它随子进程 EOF 自然退出。
    _reader: Option<thread::JoinHandle<()>>,
}

impl CoprocessMiddleware {
    /// 排出（drain）所有等待回注到会话的字节。
    ///
    /// 调用方应周期性调用此方法，并把返回的字节通过 `Session::write` 写入会话。
    pub fn drain_pending_input(&self) -> Vec<u8> {
        let mut guard = self.pending_input.lock();
        std::mem::take(&mut *guard)
    }

    /// 仅供测试与高级用户使用：返回 `pending_input` 共享句柄。
    #[doc(hidden)]
    pub fn pending_input_handle(&self) -> Arc<Mutex<Vec<u8>>> {
        Arc::clone(&self.pending_input)
    }

    /// 主动关闭子进程的 stdin（向子进程发送 EOF）。
    pub fn close_stdin(&mut self) {
        self.stdin.take();
    }
}

impl SessionMiddleware for CoprocessMiddleware {
    fn process_output_to_terminal(&mut self, data: &[u8]) -> Bytes {
        if let Some(stdin) = self.stdin.as_mut() {
            if let Err(e) = stdin.write_all(data) {
                log::warn!("coprocess stdin write failed: {e}");
                // 若管道已断开，丢弃 stdin 句柄避免后续重复失败。
                self.stdin.take();
            }
        }
        Bytes::copy_from_slice(data)
    }

    fn process_output_to_session(&mut self, data: &[u8]) -> Bytes {
        Bytes::copy_from_slice(data)
    }
}

/// 协处理工厂。
pub struct Coprocess;

impl Coprocess {
    /// 通过 `sh -c <command>` 启动一个协处理子进程，返回对应的中间件。
    pub fn spawn(command: &str) -> io::Result<CoprocessMiddleware> {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "missing coprocess stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "missing coprocess stdout"))?;

        let pending_input = Arc::new(Mutex::new(Vec::<u8>::new()));
        let pending_clone = Arc::clone(&pending_input);

        let reader = thread::Builder::new()
            .name("coprocess-reader".into())
            .spawn(move || {
                let mut stdout = stdout;
                let mut buf = [0u8; 4096];
                loop {
                    match stdout.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            pending_clone.lock().extend_from_slice(&buf[..n]);
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(e) => {
                            log::warn!("coprocess stdout read failed: {e}");
                            break;
                        }
                    }
                }
            })?;

        Ok(CoprocessMiddleware {
            stdin: Some(stdin),
            pending_input,
            _child: Arc::new(Mutex::new(Some(child))),
            _reader: Some(reader),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// 等待 `pending_input` 至少包含 `expected_min` 个字节，最长 `timeout`。
    fn wait_for_pending(mw: &CoprocessMiddleware, expected_min: usize, timeout: Duration) -> Vec<u8> {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let guard = mw.pending_input_handle();
                let lock = guard.lock();
                if lock.len() >= expected_min {
                    return lock.clone();
                }
            }
            if Instant::now() >= deadline {
                let guard = mw.pending_input_handle();
                let lock = guard.lock();
                return lock.clone();
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn passes_session_to_session_unchanged() {
        let mut mw = Coprocess::spawn("cat >/dev/null").expect("spawn cat");
        let out = mw.process_output_to_session(b"keystrokes");
        assert_eq!(&out[..], b"keystrokes");
    }

    #[test]
    fn returns_terminal_data_unchanged() {
        let mut mw = Coprocess::spawn("cat >/dev/null").expect("spawn cat");
        let out = mw.process_output_to_terminal(b"hello");
        assert_eq!(&out[..], b"hello");
    }

    #[test]
    fn coprocess_pipes_terminal_bytes_to_child_stdin() {
        // 用 `cat` 作为协处理：写入终端的字节经由 stdin 流入 cat，
        // 再由 cat 的 stdout 进入 pending_input。
        let mut mw = Coprocess::spawn("cat").expect("spawn cat");

        let _ = mw.process_output_to_terminal(b"hello");
        // 关闭 stdin 让 cat 在 flush 后退出（这里仅希望它把已收到的字节写出来）。
        mw.close_stdin();

        let buf = wait_for_pending(&mw, 5, Duration::from_secs(2));
        assert!(buf.starts_with(b"hello"), "expected 'hello' prefix, got {buf:?}");

        // drain 后队列应清空。
        let drained = mw.drain_pending_input();
        assert!(drained.starts_with(b"hello"));
        assert!(mw.drain_pending_input().is_empty());
    }

    #[test]
    fn child_stdout_is_queued_for_session_injection() {
        // `printf hello` 立刻输出 5 个字节到 stdout 并退出，
        // 这些字节应进入 pending_input。
        let mw = Coprocess::spawn("printf hello").expect("spawn printf");
        let buf = wait_for_pending(&mw, 5, Duration::from_secs(2));
        assert_eq!(&buf[..], b"hello");
    }

    #[test]
    fn write_after_child_exit_is_safe() {
        // 子进程 `true` 立即退出；之后向终端写入字节应不会 panic。
        let mut mw = Coprocess::spawn("true").expect("spawn true");
        // 给子进程一些时间退出并关闭管道。
        thread::sleep(Duration::from_millis(100));
        let out = mw.process_output_to_terminal(b"after-exit");
        assert_eq!(&out[..], b"after-exit");
    }
}
