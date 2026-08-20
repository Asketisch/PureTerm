//! TTY 相关功能。

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::Arc;
use std::{env, io};

use polling::{Event, PollMode, Poller};

#[cfg(not(windows))]
mod unix;
#[cfg(not(windows))]
pub use self::unix::*;

#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
pub use self::windows::*;

/// `Pty` 接口的配置。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Options {
    /// Shell 选项。
    ///
    /// [`None`] 使用默认 shell。
    pub shell: Option<Shell>,

    /// Shell 启动目录。
    pub working_directory: Option<PathBuf>,

    /// 退出终端前排空子进程输出。
    pub drain_on_exit: bool,

    /// 额外环境变量。
    pub env: HashMap<String, String>,

    /// 指定 Windows shell 参数是否应被转义。
    ///
    /// - 为 `true`:按标准 C 运行时规则转义参数。
    /// - 为 `false`:原样传递参数,不做额外转义。
    #[cfg(target_os = "windows")]
    pub escape_args: bool,
}

/// Shell 选项。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Shell {
    /// 启动时运行的 shell 程序路径。
    pub(crate) program: String,
    /// 传给 shell 的参数。
    pub(crate) args: Vec<String>,
}

impl Shell {
    pub fn new(program: String, args: Vec<String>) -> Self {
        Self { program, args }
    }
}

/// 流的读和/或写行为。
///
/// 这是对轮询接口的抽象,允许单一读写对象
/// 或分离的读、写对象。
pub trait EventedReadWrite {
    type Reader: io::Read;
    type Writer: io::Write;

    /// # 安全性
    ///
    /// 底层源必须比其在 `Poller` 中的注册活得久。
    unsafe fn register(&mut self, _: &Arc<Poller>, _: Event, _: PollMode) -> io::Result<()>;
    fn reregister(&mut self, _: &Arc<Poller>, _: Event, _: PollMode) -> io::Result<()>;
    fn deregister(&mut self, _: &Arc<Poller>) -> io::Result<()>;

    fn reader(&mut self) -> &mut Self::Reader;
    fn writer(&mut self) -> &mut Self::Writer;
}

/// 与 TTY 子进程相关的事件。
#[derive(Debug, PartialEq, Eq)]
pub enum ChildEvent {
    /// 指示子进程已退出。
    Exited(Option<ExitStatus>),
}

/// 伪终端(PTY)。
///
/// 这是 EventedReadWrite 的细化,额外提供一条通道,PTY 子进程做了
/// 我们关心的(写 TTY 之外的)事情时经它通知我们。特别是,这在 UNIX 上
/// 支持无竞态的子进程退出通知(对比 `SIGCHLD`)。
pub trait EventedPty: EventedReadWrite {
    /// 尝试取一个事件。
    ///
    /// 成功返回 `Some(event)`,无可取事件时返回 `None`。
    fn next_child_event(&mut self) -> Option<ChildEvent>;
}

/// 设置环境变量。
pub fn setup_env() {
    // 可用时默认用 'alacritty' terminfo,否则
    // 用 'xterm-256color'。可被下方用户配置
    // 覆盖。
    let terminfo = if terminfo_exists("alacritty") { "alacritty" } else { "xterm-256color" };
    unsafe { env::set_var("TERM", terminfo) };

    // 宣告支持 24 位色。
    unsafe { env::set_var("COLORTERM", "truecolor") };
}

/// 检查系统上是否存在某 terminfo 条目。
fn terminfo_exists(terminfo: &str) -> bool {
    // 取 terminfo 首字符作为父目录名。
    let first = terminfo.get(..1).unwrap_or_default();
    let first_hex = format!("{:x}", first.chars().next().unwrap_or_default() as usize);

    // terminfo 文件存在于指定位置时返回 true。
    macro_rules! check_path {
        ($path:expr) => {
            if $path.join(first).join(terminfo).exists()
                || $path.join(&first_hex).join(terminfo).exists()
            {
                return true;
            }
        };
    }

    if let Some(dir) = env::var_os("TERMINFO") {
        check_path!(PathBuf::from(&dir));
    } else if let Some(home) = home::home_dir() {
        check_path!(home.join(".terminfo"));
    }

    if let Ok(dirs) = env::var("TERMINFO_DIRS") {
        for dir in dirs.split(':') {
            check_path!(PathBuf::from(dir));
        }
    }

    if let Ok(prefix) = env::var("PREFIX") {
        let path = PathBuf::from(prefix);
        check_path!(path.join("etc/terminfo"));
        check_path!(path.join("lib/terminfo"));
        check_path!(path.join("share/terminfo"));
    }

    check_path!(PathBuf::from("/etc/terminfo"));
    check_path!(PathBuf::from("/lib/terminfo"));
    check_path!(PathBuf::from("/usr/share/terminfo"));
    check_path!(PathBuf::from("/boot/system/data/terminfo"));

    // 未找到合法的 terminfo 路径。
    false
}
