//! SSH PTY 适配器——SSH 连接的 EventedPty 实现。
//!
//! 用 `openpty()` 为 ssh 子进程创建真实的内核 PTY。
//! 这使 OpenSSH 能检测到 TTY,并经 `readpassphrase()`
//! 交互式提示输入密码。SSH 自动管理终端模式(raw 模式、回显开关)
//! ——不会出现双重回显,因为 SSH 在会话期间把本地 PTY 设为
//! raw 模式。
//!
//! ## 架构
//!
//! ```text
//! PureTerm ←master fd→ PTY slave ← ssh child → remote host
//! ```
//!
//! - `openpty()` 创建 master/slave PTY 对
//! - ssh 的 stdin/stdout 连到 slave 端
//! - `pre_exec` 调用 `setsid()` + `TIOCSCTTY`,使 `/dev/tty` → slave
//! - 调整大小用 master fd 上的 `ioctl(TIOCSWINSZ)`(可靠的内核级操作)
//! - stderr 保持为管道,用于错误检测(连接被拒等)
//!
//! ## 从 P1 保留下来的缺陷修复(GAP-5.7/5.8/5.9 计划)
//!
//! - **#2 SIGWINCH** —— 调整大小改用 ioctl 而非转义序列
//! - **#5 unregister_signal 错误** —— `Drop` 记录日志并继续
//! - **#9 graceful_close** —— 写入 `exit\r`,等待,然后 SIGHUP
//!
//! ## 本版本新增
//!
//! - 用真实本地 PTY(openpty)替代 Stdio::piped()——支持交互式
//!   密码认证,由 SSH 正确管理终端模式
//! - 用 `ioctl(TIOCSWINSZ)` 调整大小,替代转义序列
//! - 支持 `SSH_ASKPASS`,自动填充已保存的密码

use std::fs::{self, File, Permissions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd as _, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};
use polling::{Event, PollMode, Poller};
use rustix_openpty::rustix::termios::Winsize;
use signal_hook::low_level::{pipe as signal_pipe, unregister as unregister_signal};
use signal_hook::{SigId, consts as sigconsts};

/// 实现 EventedPty 的 SSH PTY 适配器。
///
/// 包装一个带真实内核 PTY(经 `openpty()`)的 `ssh -tt` 子进程。
/// master fd 双向使用:读远端输出、写
/// 用户输入。SSH 自动管理 PTY 上的终端模式
/// (raw 模式、回显)。
pub struct SshPty {
    child: Child,
    /// PTY master fd——双向(读远端输出,写用户输入)
    master: File,
    signals: File,
    sig_id: SigId,
    /// SSH_ASKPASS 脚本的临时目录(Drop 时清理)
    askpass_tempdir: Option<tempfile::TempDir>,
}

/// PTY 读/写事件的兴趣注册。
const PTY_READ_WRITE_TOKEN: usize = 0;

/// 子进程退出事件的兴趣注册。
const PTY_CHILD_EVENT_TOKEN: usize = 1;

/// 经 `TIOCSCTTY` ioctl 设置控制终端。
fn set_controlling_terminal(fd: libc::c_int) -> io::Result<()> {
    let res = unsafe {
        #[allow(clippy::cast_lossless)]
        libc::ioctl(fd, libc::TIOCSCTTY as _, 0)
    };
    if res == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// 把文件描述符设为非阻塞模式。
unsafe fn set_nonblocking(fd: libc::c_int) {
    let res = unsafe {
        libc::fcntl(fd, libc::F_SETFL, libc::fcntl(fd, libc::F_GETFL, 0) | libc::O_NONBLOCK)
    };
    assert_eq!(res, 0, "fcntl F_SETFL O_NONBLOCK failed");
}

impl SshPty {
    /// PTY master `File`(双向)。守护进程用它轮询、
    /// 读远端输出,并用 `ioctl(TIOCSWINSZ)` 调整大小——它不需要
    /// `EventedPty` trait 实现,只需要裸 master fd。
    pub fn master_file(&self) -> &File {
        &self.master
    }

    /// ssh 子进程的 pid。
    pub fn child_pid(&self) -> u32 {
        self.child.id()
    }

    /// OpenSSH `SSH_ASKPASS_REQUIRE` 的取值。
    ///
    /// 必须是 `force`,**不能**是 `prefer`。我们给 ssh 子进程设置了控制
    /// 终端(`pre_exec` → `set_controlling_terminal`),而 OpenSSH 的
    /// `=prefer` 模式只在**没有** tty 时才使用 `SSH_ASKPASS`。
    /// 有 tty 时,`=prefer` 在终端上提示,已保存的
    /// 密码永远不会被提供——于是密码认证的主机能连上,把
    /// 提示符作为第一个字节发出(→ 误报 `Connected`),随后被
    /// 服务器关闭并以 `failure_kind: unknown` 失败。`=force` 无论有无 tty
    /// 都使用 askpass 助手,因此已保存的密码总能被提供。
    fn askpass_require_mode() -> &'static str {
        "force"
    }

    /// 用真实内核 PTY 拉起新的 SSH 会话。
    ///
    /// 用 `openpty()` 创建 PTY master/slave 对。ssh 子
    /// 进程以 slave 端作为 stdin/stdout。`pre_exec` 调用
    /// `setsid()` + `TIOCSCTTY`,使 SSH 看到正规的控制终端,
    /// 并能交互式提示输入密码。
    ///
    /// 若提供了 `password`,则配置 `SSH_ASKPASS` 自动填充。
    /// `password` 为 `None` 时,SSH 用 PTY 做交互式提示。
    pub fn spawn(
        cmd: &[String],
        window_size: Option<(u16, u16)>,
        password: Option<&str>,
        agent_sock: Option<&Path>,
    ) -> io::Result<(Self, Option<File>)> {
        // 为 openpty 构建初始 winsize。
        let winsize = window_size.map(|(cols, rows)| Winsize {
            ws_row: rows as libc::c_ushort,
            ws_col: cols as libc::c_ushort,
            ws_xpixel: 0,
            ws_ypixel: 0,
        });

        // 创建 PTY master/slave 对。
        let pty = rustix_openpty::openpty(None, winsize.as_ref())?;
        let master_fd: OwnedFd = pty.controller;
        let slave_fd: OwnedFd = pty.user;

        let master_raw = master_fd.as_raw_fd();
        let slave_raw = slave_fd.as_raw_fd();

        // 以 slave PTY 作为 stdin/stdout 构建 ssh 命令。
        let mut builder = Command::new(&cmd[0]);
        builder.args(&cmd[1..]);

        // 把 stdin/stdout 设为 PTY slave(各自复制一份)。
        builder.stdin(slave_fd.try_clone()?);
        builder.stdout(slave_fd);
        // stderr 保持为管道,用于错误检测。
        builder.stderr(Stdio::piped());

        // GAP-5.4:请求 agent 转发时显式为子进程设置 SSH_AUTH_SOCK。
        // `Command` 已继承我们的环境,但解析出的
        // socket(配置覆盖 / macOS `launchctl getenv` 回退)
        // 可能与继承到的不同,因此显式设置。
        if let Some(p) = agent_sock {
            builder.env("SSH_AUTH_SOCK", p);
        }

        // 为已保存密码的自动填充配置 SSH_ASKPASS。
        let askpass_tempdir = if let Some(pw) = password {
            let dir = tempfile::TempDir::new()?;
            fs::set_permissions(dir.path(), Permissions::from_mode(0o700))?;

            // 把密码写入临时文件(权限 0600)。
            let pw_file_path = dir.path().join("pw");
            fs::write(&pw_file_path, pw.as_bytes())?;
            fs::set_permissions(&pw_file_path, Permissions::from_mode(0o600))?;

            // 创建输出该密码的 askpass 脚本。
            let script_path = dir.path().join("askpass.sh");
            let script_content = format!(
                "#!/bin/sh\ncat \"{}\"",
                pw_file_path.display()
            );
            fs::write(&script_path, script_content)?;
            fs::set_permissions(&script_path, Permissions::from_mode(0o700))?;

            builder.env("SSH_ASKPASS", script_path);
            builder.env("SSH_ASKPASS_REQUIRE", Self::askpass_require_mode());
            // 即使在 macOS 上,SSH_ASKPASS 也要求 DISPLAY 存在。
            if std::env::var("DISPLAY").is_err() {
                builder.env("DISPLAY", ":0");
            }

            Some(dir)
        } else {
            None
        };

        // pre_exec:创建新会话并设置控制终端。
        // 这镜像了 alacritty_terminal/src/tty/unix.rs 的本地 PTY 设置。
        unsafe {
            builder.pre_exec(move || {
                // 创建新会话(脱离 PureTerm 的控制终端)。
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }

                // 把 PTY slave 设为控制终端。
                // 这使 /dev/tty 指向我们的 PTY slave,使 SSH 的
                // readpassphrase() 能提示输入密码。
                set_controlling_terminal(slave_raw)?;

                // 关闭子进程中的两个 PTY fd——设置控制终端
                // 之后不再需要。
                libc::close(slave_raw);
                libc::close(master_raw);

                Ok(())
            });
        }

        // 拉起前准备 SIGCHLD 信号处理。
        let (signals, sig_id) = {
            let (sender, recv) = UnixStream::pair()?;
            sender.set_nonblocking(false)?;
            recv.set_nonblocking(true)?;
            let sig_id = signal_pipe::register(sigconsts::SIGCHLD, sender)?;
            let signals_file = unsafe { File::from_raw_fd(recv.into_raw_fd()) };
            (signals_file, sig_id)
        };

        let mut child = builder.spawn()?;

        // 把 master fd 设为非阻塞,供事件循环 poller 使用。
        unsafe {
            set_nonblocking(master_raw);
        }

        let master = File::from(master_fd);

        // 把 stderr 捕获为非阻塞 File,用于错误检测。
        let stderr = child.stderr.take().map(|pipe| {
            let raw_fd = pipe.into_raw_fd();
            let file = unsafe { File::from_raw_fd(raw_fd) };
            unsafe {
                let flags = libc::fcntl(file.as_raw_fd(), libc::F_GETFL, 0);
                if flags >= 0 {
                    libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
                }
            }
            file
        });

        Ok((
            Self {
                child,
                master,
                signals,
                sig_id,
                askpass_tempdir,
            },
            stderr,
        ))
    }

    /// 经 `ioctl(TIOCSWINSZ)` 调整 PTY 大小。
    ///
    /// 比转义序列更可靠:由内核层处理,
    /// 并自动经 SSH 的 PTY 尺寸
    /// 协商传播。
    pub fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        let win = Winsize {
            ws_row: rows as libc::c_ushort,
            ws_col: cols as libc::c_ushort,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let res =
            unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &win as *const _) };
        if res < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// 请求远端 shell 干净退出,然后最多等待 `timeout`。
    ///
    /// 1. 向 PTY master 写 `exit\r`,让远端 shell 收到。
    /// 2. 轮询 `try_wait`,至多 `timeout`。
    /// 3. 子进程仍在运行时,发送 `SIGHUP` 并返回 TimedOut。
    pub fn graceful_close(&mut self, timeout: Duration) -> io::Result<()> {
        // 步骤 1:请求远端 shell 退出。
        if let Err(e) = self.master.write_all(b"exit\r") {
            log::debug!("SshPty::graceful_close: write 'exit\\r' failed: {}", e);
        }
        let _ = self.master.flush();

        // 步骤 2:轮询子进程退出。
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait() {
                Ok(Some(_status)) => return Ok(()),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                },
                Err(e) => {
                    log::debug!("SshPty::graceful_close: try_wait error: {}", e);
                    return Ok(());
                },
            }
        }

        // 步骤 3:期限已过;升级到 SIGHUP。
        log::warn!(
            "SshPty::graceful_close: ssh child did not exit within {:?}; sending SIGHUP",
            timeout
        );
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGHUP);
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "ssh child did not exit within graceful_close timeout",
        ))
    }
}

impl OnResize for SshPty {
    fn on_resize(&mut self, window_size: WindowSize) {
        if let Err(e) = self.resize(window_size.num_cols, window_size.num_lines) {
            log::debug!("SshPty::on_resize: ioctl TIOCSWINSZ failed: {}", e);
        }
    }
}

impl Drop for SshPty {
    fn drop(&mut self) {
        // Bug #5 修复:注销失败时记录日志而非 panic。
        if !unregister_signal(self.sig_id) {
            log::warn!("SshPty::Drop: unregister_signal returned false (id={:?})", self.sig_id);
        }

        // 尽力收殓子进程。
        if let Ok(None) = self.child.try_wait() {
            unsafe {
                libc::kill(self.child.id() as i32, libc::SIGHUP);
            }
        }
        let _ = self.child.wait();

        // SSH_ASKPASS 临时文件由 TempDir drop 自动清理。
    }
}

impl EventedReadWrite for SshPty {
    type Reader = File;
    type Writer = File;

    #[inline]
    unsafe fn register(
        &mut self,
        poll: &Arc<Poller>,
        mut interest: Event,
        poll_opts: PollMode,
    ) -> io::Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        unsafe {
            poll.add_with_mode(&self.master, interest, poll_opts)?;
        }
        unsafe {
            poll.add_with_mode(
                &self.signals,
                Event::readable(PTY_CHILD_EVENT_TOKEN),
                PollMode::Level,
            )
        }
    }

    #[inline]
    fn reregister(
        &mut self,
        poll: &Arc<Poller>,
        mut interest: Event,
        poll_opts: PollMode,
    ) -> io::Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        poll.modify_with_mode(&self.master, interest, poll_opts)?;
        poll.modify_with_mode(
            &self.signals,
            Event::readable(PTY_CHILD_EVENT_TOKEN),
            PollMode::Level,
        )
    }

    #[inline]
    fn deregister(&mut self, poll: &Arc<Poller>) -> io::Result<()> {
        poll.delete(&self.master)?;
        poll.delete(&self.signals)
    }

    #[inline]
    fn reader(&mut self) -> &mut Self::Reader {
        &mut self.master
    }

    #[inline]
    fn writer(&mut self) -> &mut Self::Writer {
        &mut self.master
    }
}

impl EventedPty for SshPty {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        use std::io::ErrorKind;
        let mut buf = [0u8; 1];
        if let Err(err) = self.signals.read(&mut buf) {
            if err.kind() != ErrorKind::WouldBlock {
                log::error!("Error reading from signal pipe: {}", err);
            }
            return None;
        }

        match self.child.try_wait() {
            Err(err) => {
                log::error!("Error checking child process termination: {}", err);
                None
            },
            Ok(None) => None,
            Ok(exit_status) => Some(ChildEvent::Exited(exit_status)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证拉起后调整大小可用(基于 ioctl,而非转义序列)。
    #[test]
    fn test_resize_succeeds_after_spawn() {
        let cmd = vec!["cat".to_string()];
        let (mut pty, _stderr) =
            SshPty::spawn(&cmd, Some((80, 24)), None, None).expect("spawn cat");
        pty.resize(132, 50).expect("resize must not error");
    }

    /// graceful_close 不得 panic。我们用不会响应
    /// `exit\r` 退出的 `cat`,因此预期 TimedOut 或 Ok(若子进程恰好退出)。
    #[test]
    fn test_graceful_close_does_not_panic() {
        let cmd = vec!["cat".to_string()];
        let (mut pty, _stderr) = SshPty::spawn(&cmd, None, None, None).expect("spawn cat");
        let result = pty.graceful_close(Duration::from_millis(50));
        let _ = result;
    }

    /// 验证 reader 与 writer 现在返回同一个 fd(master fd)。
    #[test]
    fn test_reader_writer_are_same_fd() {
        let cmd = vec!["cat".to_string()];
        let (mut pty, _stderr) = SshPty::spawn(&cmd, None, None, None).expect("spawn cat");
        use std::os::fd::AsRawFd;
        assert_eq!(
            pty.reader().as_raw_fd(),
            pty.writer().as_raw_fd(),
            "reader and writer must point to the same master fd",
        );
    }

    /// 验证提供密码时会创建 SSH_ASKPASS 临时目录。
    #[test]
    fn test_askpass_tempdir_created_with_password() {
        let cmd = vec!["cat".to_string()];
        let (pty, _stderr) =
            SshPty::spawn(&cmd, Some((80, 24)), Some("test_password"), None).expect("spawn cat");
        assert!(pty.askpass_tempdir.is_some());
        let dir = pty.askpass_tempdir.as_ref().unwrap();
        assert!(dir.path().join("askpass.sh").exists());
        assert!(dir.path().join("pw").exists());
    }

    /// 验证密码为 None 时不创建 askpass 临时目录。
    #[test]
    fn test_no_askpass_without_password() {
        let cmd = vec!["cat".to_string()];
        let (pty, _stderr) = SshPty::spawn(&cmd, Some((80, 24)), None, None).expect("spawn cat");
        assert!(pty.askpass_tempdir.is_none());
    }

    /// 回归守护:`SSH_ASKPASS_REQUIRE` 必须是 `force`。在
    /// PureTerm 给 ssh 的控制终端下,`=prefer` 永不调用
    /// askpass 助手,已保存的密码被静默忽略,
    /// 密码认证的主机无法连接(连上即死,
    /// `failure_kind: unknown`)。人工验证:同一主机在
    /// `=force` 下认证正常。此测试防止退回 `prefer`。
    #[test]
    fn askpass_require_is_force_not_prefer() {
        assert_eq!(SshPty::askpass_require_mode(), "force");
    }
}
