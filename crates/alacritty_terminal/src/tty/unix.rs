//! TTY 相关功能。

use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{Error, ErrorKind, Read, Result};
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
#[cfg(target_os = "macos")]
use std::path::Path;
use std::process::{Child, Command};
use std::sync::Arc;
use std::{env, ptr};

use libc::{F_GETFL, F_SETFL, O_NONBLOCK, TIOCSCTTY, c_int, fcntl};
use log::error;
use polling::{Event, PollMode, Poller};
use rustix_openpty::openpty;
use rustix_openpty::rustix::termios::Winsize;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use rustix_openpty::rustix::termios::{self, InputModes, OptionalActions};
use signal_hook::low_level::{pipe as signal_pipe, unregister as unregister_signal};
use signal_hook::{SigId, consts as sigconsts};

use crate::event::{OnResize, WindowSize};
use crate::tty::{ChildEvent, EventedPty, EventedReadWrite, Options};

// PTY 读写的兴趣注册。
pub(crate) const PTY_READ_WRITE_TOKEN: usize = 0;

// 新子进程事件的兴趣注册。
pub(crate) const PTY_CHILD_EVENT_TOKEN: usize = 1;

macro_rules! die {
    ($($arg:tt)*) => {{
        error!($($arg)*);
        std::process::exit(1);
    }};
}

/// 其实只在 BSD 上需要,但别处也无害。
fn set_controlling_terminal(fd: c_int) -> Result<()> {
    let res = unsafe {
        // TIOSCTTY 随平台变化,`ioctl` 调用随架构
        //(32/64 位)不同。因此用泛型转型确保
        // 不出问题。为允许该泛型转型,禁用了
        // clippy 警告。
        #[allow(clippy::cast_lossless)]
        libc::ioctl(fd, TIOCSCTTY as _, 0)
    };

    if res == 0 { Ok(()) } else { Err(Error::last_os_error()) }
}

#[derive(Debug)]
struct Passwd<'a> {
    name: &'a str,
    dir: &'a str,
    shell: &'a str,
}

/// 返回指向所给 buf 的 Passwd 结构。
///
/// # 不安全性
///
/// 若 `Passwd` 存活期间 `buf` 被改变,几乎必然出问题。
fn get_pw_entry(buf: &mut [i8; 1024]) -> Result<Passwd<'_>> {
    // 创建清零的 passwd 结构。
    let mut entry: MaybeUninit<libc::passwd> = MaybeUninit::uninit();

    let mut res: *mut libc::passwd = ptr::null_mut();

    // 尝试读取 pw 文件。
    let uid = unsafe { libc::getuid() };
    let status = unsafe {
        libc::getpwuid_r(uid, entry.as_mut_ptr(), buf.as_mut_ptr() as *mut _, buf.len(), &mut res)
    };
    let entry = unsafe { entry.assume_init() };

    if status < 0 {
        return Err(Error::other("getpwuid_r failed"));
    }

    if res.is_null() {
        return Err(Error::other("pw not found"));
    }

    // 健全性检查。
    assert_eq!(entry.pw_uid, uid);

    // 构建借用的 Passwd 结构。
    Ok(Passwd {
        name: unsafe { CStr::from_ptr(entry.pw_name).to_str().unwrap() },
        dir: unsafe { CStr::from_ptr(entry.pw_dir).to_str().unwrap() },
        shell: unsafe { CStr::from_ptr(entry.pw_shell).to_str().unwrap() },
    })
}

pub struct Pty {
    child: Child,
    file: File,
    signals: UnixStream,
    sig_id: SigId,
}

impl Pty {
    pub fn child(&self) -> &Child {
        &self.child
    }

    pub fn file(&self) -> &File {
        &self.file
    }
}

/// 新 shell 会话所需的用户信息。
struct ShellUser {
    user: String,
    home: String,
    shell: String,
}

impl ShellUser {
    /// 先从相应环境变量找 shell、用户名、长名与主目录,
    /// 再回退到查询 `passwd`。
    fn from_env() -> Result<Self> {
        let mut buf = [0; 1024];
        let pw = get_pw_entry(&mut buf);

        let user = match env::var("USER") {
            Ok(user) => user,
            Err(_) => match pw {
                Ok(ref pw) => pw.name.to_owned(),
                Err(err) => return Err(err),
            },
        };

        let home = match env::var("HOME") {
            Ok(home) => home,
            Err(_) => match pw {
                Ok(ref pw) => pw.dir.to_owned(),
                Err(err) => return Err(err),
            },
        };

        let shell = match env::var("SHELL") {
            Ok(shell) => shell,
            Err(_) => match pw {
                Ok(ref pw) => pw.shell.to_owned(),
                Err(err) => return Err(err),
            },
        };

        Ok(Self { user, home, shell })
    }
}

#[cfg(not(target_os = "macos"))]
fn default_shell_command(shell: &str, _user: &str, _home: &str) -> Command {
    Command::new(shell)
}

#[cfg(target_os = "macos")]
fn default_shell_command(shell: &str, user: &str, home: &str) -> Command {
    let shell_name = shell.rsplit('/').next().unwrap();

    // macOS 上用 `login` 命令,使 shell 表现为 tty 会话。
    let mut login_command = Command::new("/usr/bin/login");

    // 以 '-' 前置 argv[0] 执行 shell,使其成为登录 shell。
    // `login` 通常自己做这件事,但 `-l` 禁用了它。
    let exec = format!("exec -a -{} {}", shell_name, shell);

    // 由于用了 -l,`login` 不会切换到用户主目录;而
    // `login` 只在当前工作目录检查 .hushlogin,会漏掉
    // 用户主目录里的。我们自己检查并传 `-q`
    // 来修复。
    let has_home_hushlogin = Path::new(home).join(".hushlogin").exists();

    // -f:为已登录用户绕过认证。
    // -l:跳过切换到 $HOME 与给 argv[0] 前置 '-'。
    // -p:保留环境。
    // -q:表现得像存在 `.hushlogin`。
    //
    // XXX:因 `exec -a` 用 zsh 而非 sh。
    let flags = if has_home_hushlogin { "-qflp" } else { "-flp" };
    login_command.args([flags, user, "/bin/zsh", "-fc", &exec]);
    login_command
}

/// 创建新 TTY 并返回与其交互的句柄。
pub fn new(config: &Options, window_size: WindowSize, window_id: u64) -> Result<Pty> {
    let pty = openpty(None, Some(&window_size.to_winsize()))?;
    let (master, slave) = (pty.controller, pty.user);
    from_fd(config, window_id, master, slave)
}

/// 从 PTY 的文件描述符创建新 TTY。
pub fn from_fd(config: &Options, window_id: u64, master: OwnedFd, slave: OwnedFd) -> Result<Pty> {
    let master_fd = master.as_raw_fd();
    let slave_fd = slave.as_raw_fd();

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Ok(mut termios) = termios::tcgetattr(&master) {
        // 设置字符编码为 UTF-8。
        termios.input_modes.set(InputModes::IUTF8, true);
        let _ = termios::tcsetattr(&master, OptionalActions::Now, &termios);
    }

    let user = ShellUser::from_env()?;

    let mut builder = if let Some(shell) = config.shell.as_ref() {
        let mut cmd = Command::new(&shell.program);
        cmd.args(shell.args.as_slice());
        cmd
    } else {
        default_shell_command(&user.shell, &user.user, &user.home)
    };

    // 把子进程的 stdin/stdout/stderr 设为 PTY 的 slave fd。
    builder.stdin(slave.try_clone()?);
    builder.stderr(slave.try_clone()?);
    builder.stdout(slave);

    // 设置 shell 环境。
    let window_id = window_id.to_string();
    builder.env("ALACRITTY_WINDOW_ID", &window_id);
    builder.env("USER", user.user);
    builder.env("HOME", user.home);
    // 为依赖 X11 hack 的客户端设置 Window ID。
    builder.env("WINDOWID", window_id);
    for (key, value) in &config.env {
        builder.env(key, value);
    }

    // 阻止子进程继承 linux 特有的启动通知环境变量。
    builder.env_remove("XDG_ACTIVATION_TOKEN");
    builder.env_remove("DESKTOP_STARTUP_ID");

    let working_directory = config
        .working_directory
        .as_ref()
        .and_then(|path| CString::new(path.as_os_str().as_bytes()).ok());

    unsafe {
        builder.pre_exec(move || {
            // 创建新进程组。
            let err = libc::setsid();
            if err == -1 {
                return Err(Error::last_os_error());
            }

            // 设置工作目录,忽略无效路径。
            if let Some(working_directory) = working_directory.as_ref() {
                libc::chdir(working_directory.as_ptr());
            }

            set_controlling_terminal(slave_fd)?;

            // 不再需要 slave/master fd。
            libc::close(slave_fd);
            libc::close(master_fd);

            libc::signal(libc::SIGCHLD, libc::SIG_DFL);
            libc::signal(libc::SIGHUP, libc::SIG_DFL);
            libc::signal(libc::SIGINT, libc::SIG_DFL);
            libc::signal(libc::SIGQUIT, libc::SIG_DFL);
            libc::signal(libc::SIGTERM, libc::SIG_DFL);
            libc::signal(libc::SIGALRM, libc::SIG_DFL);

            Ok(())
        });
    }

    // 拉起子进程前准备信号处理。
    let (signals, sig_id) = {
        let (sender, recv) = UnixStream::pair()?;

        // 注册 SIGCHLD 管道的接收端。
        let sig_id = signal_pipe::register(sigconsts::SIGCHLD, sender)?;
        recv.set_nonblocking(true)?;
        (recv, sig_id)
    };

    match builder.spawn() {
        Ok(child) => {
            unsafe {
                // 也许应在此函数之外做,避免把非阻塞
                // 强加给消费者。也许又该如此?
                set_nonblocking(master_fd);
            }

            Ok(Pty { child, file: File::from(master), signals, sig_id })
        },
        Err(err) => Err(Error::new(
            err.kind(),
            format!(
                "Failed to spawn command '{}': {}",
                builder.get_program().to_string_lossy(),
                err
            ),
        )),
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // 确保 PTY 被正确终止。
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGHUP);
        }

        // 清除 signal-hook 处理器。
        unregister_signal(self.sig_id);

        let _ = self.child.wait();
    }
}

impl EventedReadWrite for Pty {
    type Reader = File;
    type Writer = File;

    #[inline]
    unsafe fn register(
        &mut self,
        poll: &Arc<Poller>,
        mut interest: Event,
        poll_opts: PollMode,
    ) -> Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        unsafe {
            poll.add_with_mode(&self.file, interest, poll_opts)?;
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
    ) -> Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        poll.modify_with_mode(&self.file, interest, poll_opts)?;

        poll.modify_with_mode(
            &self.signals,
            Event::readable(PTY_CHILD_EVENT_TOKEN),
            PollMode::Level,
        )
    }

    #[inline]
    fn deregister(&mut self, poll: &Arc<Poller>) -> Result<()> {
        poll.delete(&self.file)?;
        poll.delete(&self.signals)
    }

    #[inline]
    fn reader(&mut self) -> &mut File {
        &mut self.file
    }

    #[inline]
    fn writer(&mut self) -> &mut File {
        &mut self.file
    }
}

impl EventedPty for Pty {
    #[inline]
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        // 看看是否收到过 SIGCHLD。
        let mut buf = [0u8; 1];
        if let Err(err) = self.signals.read(&mut buf) {
            if err.kind() != ErrorKind::WouldBlock {
                error!("Error reading from signal pipe: {err}");
            }
            return None;
        }

        // 对子进程做 match。
        match self.child.try_wait() {
            Err(err) => {
                error!("Error checking child process termination: {err}");
                None
            },
            Ok(None) => None,
            Ok(exit_status) => Some(ChildEvent::Exited(exit_status)),
        }
    }
}

impl OnResize for Pty {
    /// 调整 PTY 大小。
    ///
    /// 告知内核窗口尺寸已变:新的像素
    /// 尺寸与行/列数。
    fn on_resize(&mut self, window_size: WindowSize) {
        let win = window_size.to_winsize();

        let res = unsafe { libc::ioctl(self.file.as_raw_fd(), libc::TIOCSWINSZ, &win as *const _) };

        if res < 0 {
            die!("ioctl TIOCSWINSZ failed: {}", Error::last_os_error());
        }
    }
}

/// 能产出 `Winsize` 的类型。
pub trait ToWinsize {
    /// 取一个 `Winsize`。
    fn to_winsize(self) -> Winsize;
}

impl ToWinsize for WindowSize {
    fn to_winsize(self) -> Winsize {
        let ws_row = self.num_lines as libc::c_ushort;
        let ws_col = self.num_cols as libc::c_ushort;

        let ws_xpixel = ws_col * self.cell_width as libc::c_ushort;
        let ws_ypixel = ws_row * self.cell_height as libc::c_ushort;
        Winsize { ws_row, ws_col, ws_xpixel, ws_ypixel }
    }
}

unsafe fn set_nonblocking(fd: c_int) {
    let res = unsafe { fcntl(fd, F_SETFL, fcntl(fd, F_GETFL, 0) | O_NONBLOCK) };
    assert_eq!(res, 0);
}

#[test]
fn test_get_pw_entry() {
    let mut buf: [i8; 1024] = [0; 1024];
    let _pw = get_pw_entry(&mut buf).unwrap();
}
