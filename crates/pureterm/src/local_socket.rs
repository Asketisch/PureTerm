//! T-core-windows-conpty (E.2): cross-platform local-domain-socket
//! abstraction.
//!
//! On Unix this is `std::os::unix::net::UnixStream` (a `SOCK_STREAM`
//! AF_UNIX socket). On Windows it would be a Named-Pipe client
//! (`\\.\pipe\<name>`); the v1 stub is a unit struct that
//! `connect()` / `pair()` reject as `Unsupported`. The abstraction
//! is intentionally tiny — the GUI's IPC layer is the only consumer,
//! and the spec is "make every `UnixStream::connect/try_clone/...`
//! go through one type so the Windows port is a follow-up mechanical
//! change".
//!
//! v1 keeps the Unix path the canonical implementation and exposes a
//! Windows `LocalStream` doc-stub so the build remains green on every
//! platform. The test surface (the round-trip stream pair) is
//! Unix-only and lives behind `#[cfg(unix)]`.

use std::io;

/// The platform's local-domain-socket stream. On Unix this is
/// `UnixStream`; on Windows it is a v1 stub. The `Read` / `Write`
/// impls on the Windows stub land in v2 alongside the real
/// `NamedPipeClient` wrapper.
#[cfg(unix)]
pub type LocalStream = std::os::unix::net::UnixStream;

/// Windows v1 stub. v2: replace with a `NamedPipeClient` wrapped in a
/// `std::io`-compatible type.
#[cfg(not(unix))]
pub struct LocalStream {
    _phantom: std::marker::PhantomData<()>,
}

/// Connect to a local-domain-socket listener. The `path` argument is a
/// filesystem path on Unix and a `\\.\pipe\<name>` literal on Windows
/// (the caller picks the right form via the existing `cli::socket`
/// `Option<PathBuf>`).
///
/// v1: on Unix, thin wrapper over `UnixStream::connect`. On Windows,
/// returns an `io::ErrorKind::Unsupported` — the v2 follow-up
/// substitutes a Named-Pipe client.
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

/// Connect to a local-domain-socket listener with a deadline. The
/// Unix path uses `std::os::unix::net::UnixStream::connect` followed
/// by `set_read_timeout` / `set_write_timeout`; the Windows path is
/// `tokio::net::windows::named_pipe::ClientOptions::open_with_timeout`
/// in v2.
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

/// Pair of connected streams (in-process). v1 returns a `UnixStream`
/// pair; the Windows v2 substitute is a `tokio::io::DuplexStream` or
/// a Named-Pipe pair.
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
