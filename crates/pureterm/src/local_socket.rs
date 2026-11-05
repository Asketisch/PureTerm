//! T-core-windows-conpty (E.2):跨平台本地域套接字抽象。
//!
//!
//! 在 Unix 上是 `std::os::unix::net::UnixStream`(`SOCK_STREAM` 类型的
//! AF_UNIX socket);在 Windows 上则是 Named-Pipe 客户端
//! (`\\.\pipe\<name>`);v1 存根是一个单元结构体,
//! 其 `connect()` / `pair()` 一律拒绝为 `Unsupported`。该抽象
//! 刻意保持极小 —— GUI 的 IPC 层是唯一使用方,
//! 规格是「让所有 `UnixStream::connect/try_clone/...` 调用
//! 都经过同一个类型,使 Windows 移植成为一次机械式跟进 ——
//! 而非常规重构」。
//!
//! v1 以 Unix 路径作为规范实现,并对外暴露一个
//! Windows 的 `LocalStream` 文档存根,保证每个平台都能
//! 构建通过。测试面(回环流对)只覆盖 Unix,由
//! `#[cfg(unix)]` 条件编译保护。

use std::io;

/// 平台自身的本地域套接字流。Unix 上是
/// `UnixStream`;Windows 上是 v1 存根。`Read` / `Write` 的
/// 实现会随 v2 与真实的
/// `NamedPipeClient` 包装一起落地。
#[cfg(unix)]
pub type LocalStream = std::os::unix::net::UnixStream;

/// Windows v1 存根。v2:替换为包装了
/// `NamedPipeClient` 的 `std::io` 兼容类型。
///
/// 为让 IPC 客户端(`ipc::client` / `tmux_cli::handlers`)的
/// `write_all` / `shutdown` / `BufReader` 代码在所有平台都能编译,
/// 这里实现 `Read` / `Write`(一律返回 Unsupported)并提供
/// 与 `UnixStream::shutdown` 同名同签名的方法。真正的连接由
/// `connect()` 在 v1 就拒绝,故这些 IO 方法在 Windows 上永不会被
/// 成功调用 —— 它们只承担「让类型系统闭合」的职责。
#[cfg(not(unix))]
pub struct LocalStream {
    _phantom: std::marker::PhantomData<()>,
}

#[cfg(not(unix))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "LocalStream IO is Unix-only in v1; the Windows named-pipe port lives in v2",
    )
}

#[cfg(not(unix))]
impl LocalStream {
    /// 与 `std::os::unix::net::UnixStream::shutdown` 对齐的桩。
    #[allow(clippy::unused_self)]
    pub fn shutdown(&self, _how: std::net::Shutdown) -> io::Result<()> {
        Err(unsupported())
    }
}

#[cfg(not(unix))]
impl std::io::Read for LocalStream {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(unsupported())
    }
}

#[cfg(not(unix))]
impl std::io::Write for LocalStream {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(unsupported())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(unsupported())
    }
}

// `BufReader::new(&stream)` 需要 `&LocalStream: Read`。
// std 已为 `&T where T: Read + ?Sized` 之外的类型未自动派生;
// 这里给引用类型补一份,使客户端的阻塞读代码在 Windows 上闭合。
#[cfg(not(unix))]
impl std::io::Read for &LocalStream {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(unsupported())
    }
}

/// 连接到本地域套接字监听端。`path` 参数是
/// Unix 上的文件系统路径,或是 Windows 上的 `\\.\pipe\<name>` 字面量
/// (调用方通过既有的 `cli::socket`
/// `Option<PathBuf>`).
///
/// v1:在 Unix 上是对 `UnixStream::connect` 的薄封装;在 Windows 上
/// 返回 `io::ErrorKind::Unsupported` —— v2 跟进
/// 会替换为 Named-Pipe 客户端。
pub fn connect(path: &std::path::Path) -> io::Result<LocalStream> {
    #[cfg(unix)]
    {
        std::os::unix::net::UnixStream::connect(path)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "LocalStream::connect is Unix-only in v1; the Windows port lives in v2",
        ))
    }
}

/// 带截止时间地连接本地域套接字监听端。
/// Unix 路径使用 `std::os::unix::net::UnixStream::connect`,随后
/// 设置 `set_read_timeout` / `set_write_timeout`;Windows 路径
/// `tokio::net::windows::named_pipe::ClientOptions::open_with_timeout`
/// 在 v2 中完成。
pub fn connect_timeout(
    path: &std::path::Path,
    timeout: std::time::Duration,
) -> io::Result<LocalStream> {
    #[cfg(unix)]
    {
        let s = std::os::unix::net::UnixStream::connect(path)?;
        s.set_read_timeout(Some(timeout))?;
        s.set_write_timeout(Some(timeout))?;
        Ok(s)
    }
    #[cfg(not(unix))]
    {
        let _ = (path, timeout);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "LocalStream::connect_timeout is Unix-only in v1",
        ))
    }
}

/// 一对已连接的流(进程内)。v1 返回一对
/// `UnixStream`;Windows 的 v2 替代品是 `tokio::io::DuplexStream` 或
/// 一对 Named-Pipe。
pub fn pair() -> io::Result<(LocalStream, LocalStream)> {
    #[cfg(unix)]
    {
        std::os::unix::net::UnixStream::pair()
    }
    #[cfg(not(unix))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "LocalStream::pair is Unix-only in v1",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[cfg(unix)]
    #[test]
    fn pair_round_trip() {
        let (mut a, mut b) = pair().expect("pair");
        a.write_all(b"hello").expect("write a");
        let mut buf = [0u8; 5];
        b.read_exact(&mut buf).expect("read b");
        assert_eq!(&buf, b"hello");
    }

    #[cfg(unix)]
    #[test]
    fn connect_to_listener() {
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pt.sock");
        let listener = UnixListener::bind(&path).expect("bind");
        let t = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().expect("accept");
            s.write_all(b"hi").expect("write");
        });
        let mut s = connect(&path).expect("connect");
        let mut buf = [0u8; 2];
        s.read_exact(&mut buf).expect("read");
        assert_eq!(&buf, b"hi");
        t.join().expect("thread");
    }

    #[cfg(not(unix))]
    #[test]
    fn windows_unsupported_in_v1() {
        assert!(connect(std::path::Path::new("dummy")).is_err());
        assert!(pair().is_err());
    }
}
