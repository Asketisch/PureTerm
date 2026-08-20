//! GAP-43:本地与 SSH 守护会话的目录列举与文件读取。
//!
//! 本地会话用 `std::fs::read_dir`。SSH 会话复用每会话的
//! OpenSSH ControlMaster socket(`ssh -S … host ls -1p`)——暂无独立
//! 远端二进制或 protobuf 通道。

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `remote.read` 预览的默认字节上限。
pub const READ_MAX_BYTES: u64 = 65_536;
/// `remote.write` 的默认字节上限。
pub const WRITE_MAX_BYTES: u64 = 65_536;
/// `remote.find` 返回的路径上限。
pub const FIND_MAX_RESULTS: usize = 100;

/// `remote.read` 的文本载荷。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileReadResult {
    pub path: String,
    pub content: String,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// `remote.listdir` 返回的一个目录条目。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    /// 已知时的字节大小(SSH `ls -1p` 路径仅文件有)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// `remote.find` 的一条路径匹配。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FindMatch {
    pub path: String,
    pub is_dir: bool,
}

/// 存在守护进程所属 SSH 会话上的 SSH 连接元数据。
#[derive(Debug, Clone)]
pub struct SshSessionMeta {
    pub control_socket: PathBuf,
    pub host: String,
}

/// 拒绝可能破坏单 shell 参数约束的路径。
fn validate_remote_path(path: &str) -> io::Result<()> {
    if path.is_empty() {
        return Ok(());
    }
    if path.contains('\0') || path.contains('\n') || path.contains('\r') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains invalid characters",
        ));
    }
    Ok(())
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// 列举本地目录(绝对或相对路径)。
pub fn list_local_dir(path: &Path) -> io::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for ent in std::fs::read_dir(path)? {
        let ent = ent?;
        let meta = ent.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = meta.filter(|m| m.is_file()).map(|m| m.len());
        entries.push(FileEntry {
            name: ent.file_name().to_string_lossy().into_owned(),
            is_dir,
            size,
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

/// 经既有 ControlMaster socket 列举远端目录。
pub fn list_remote_dir(
    control_socket: &Path,
    host: &str,
    remote_path: &str,
) -> io::Result<Vec<FileEntry>> {
    validate_remote_path(remote_path)?;
    let path = if remote_path.is_empty() { "." } else { remote_path };
    let script = format!("ls -1p -- {}", shell_single_quote(path));

    let output = Command::new("ssh")
        .arg("-S")
        .arg(control_socket)
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg(host)
        .arg(&script)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            format!("remote ls failed (status {})", output.status)
        } else {
            stderr
        };
        return Err(io::Error::new(io::ErrorKind::Other, msg));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let is_dir = line.ends_with('/');
        let name = if is_dir {
            line.trim_end_matches('/').to_string()
        } else {
            line.to_string()
        };
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        entries.push(FileEntry { name, is_dir, size: None });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

/// 把本地文件按 UTF-8 文本(有损)读取,上限 `max_bytes`。
pub fn read_local_file(path: &str, max_bytes: u64) -> io::Result<FileReadResult> {
    validate_remote_path(path)?;
    let p = if path.is_empty() { "." } else { path };
    let data = std::fs::read(p)?;
    let size = data.len() as u64;
    let truncated = size > max_bytes;
    let slice = &data[..data.len().min(max_bytes as usize)];
    Ok(FileReadResult {
        path: p.to_string(),
        content: String::from_utf8_lossy(slice).into_owned(),
        truncated,
        size: Some(size),
    })
}

/// 经 ControlMaster 读远端文件(`head -c` 限长)。
pub fn read_remote_file(
    control_socket: &Path,
    host: &str,
    remote_path: &str,
    max_bytes: u64,
) -> io::Result<FileReadResult> {
    validate_remote_path(remote_path)?;
    let path = if remote_path.is_empty() {
        "."
    } else {
        remote_path
    };
    let script = format!(
        "head -c {} -- {}",
        max_bytes,
        shell_single_quote(path)
    );

    let output = Command::new("ssh")
        .arg("-S")
        .arg(control_socket)
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg(host)
        .arg(&script)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            format!("remote read failed (status {})", output.status)
        } else {
            stderr
        };
        return Err(io::Error::new(io::ErrorKind::Other, msg));
    }

    let bytes = output.stdout;
    let truncated = bytes.len() as u64 >= max_bytes;
    Ok(FileReadResult {
        path: path.to_string(),
        content: String::from_utf8_lossy(&bytes).into_owned(),
        truncated,
        size: None,
    })
}

/// 写本地文本文件(创建或截断)。
pub fn write_local_file(path: &str, content: &str) -> io::Result<()> {
    validate_remote_path(path)?;
    if content.len() as u64 > WRITE_MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("content exceeds {WRITE_MAX_BYTES} bytes"),
        ));
    }
    let p = if path.is_empty() { "." } else { path };
    std::fs::write(p, content.as_bytes())
}

/// 经 `ssh -S … cat > path` 写远端文本文件。
pub fn write_remote_file(
    control_socket: &Path,
    host: &str,
    remote_path: &str,
    content: &str,
) -> io::Result<()> {
    validate_remote_path(remote_path)?;
    if content.len() as u64 > WRITE_MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("content exceeds {WRITE_MAX_BYTES} bytes"),
        ));
    }
    let path = if remote_path.is_empty() {
        "."
    } else {
        remote_path
    };
    let script = format!("cat > {}", shell_single_quote(path));
    let mut child = Command::new("ssh")
        .arg("-S")
        .arg(control_socket)
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg(host)
        .arg(&script)
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(content.as_bytes())?;
    }
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("remote write failed (status {status})"),
        ))
    }
}

fn parse_find_lines(base: &str, stdout: &str, max: usize) -> Vec<FindMatch> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let meta = std::fs::metadata(line).ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let path = if base == "." || base.is_empty() {
            line.to_string()
        } else {
            line.to_string()
        };
        out.push(FindMatch { path, is_dir });
        if out.len() >= max {
            break;
        }
    }
    out
}

/// 在 `base` 下查找匹配 `query` 的文件/目录名(不区分大小写 glob)。
pub fn find_local(base: &str, query: &str, max: usize) -> io::Result<Vec<FindMatch>> {
    validate_remote_path(base)?;
    if query.is_empty() {
        return Ok(Vec::new());
    }
    if query.contains('\0') || query.contains('\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "query contains invalid characters",
        ));
    }
    let dir = if base.is_empty() { "." } else { base };
    let pattern = format!("*{query}*");
    let output = Command::new("find")
        .arg(dir)
        .arg("-maxdepth")
        .arg("4")
        .arg("-iname")
        .arg(&pattern)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(io::Error::new(
            io::ErrorKind::Other,
            if stderr.is_empty() {
                "find failed".into()
            } else {
                stderr
            },
        ));
    }
    Ok(parse_find_lines(
        dir,
        &String::from_utf8_lossy(&output.stdout),
        max.min(FIND_MAX_RESULTS),
    ))
}

/// 经 ControlMaster + `find` 查找远端路径。
pub fn find_remote(
    control_socket: &Path,
    host: &str,
    base: &str,
    query: &str,
    max: usize,
) -> io::Result<Vec<FindMatch>> {
    validate_remote_path(base)?;
    if query.is_empty() {
        return Ok(Vec::new());
    }
    if query.contains('\0') || query.contains('\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "query contains invalid characters",
        ));
    }
    let dir = if base.is_empty() { "." } else { base };
    let pattern = format!("*{query}*");
    let script = format!(
        "find {} -maxdepth 4 -iname {}",
        shell_single_quote(dir),
        shell_single_quote(&pattern)
    );
    let output = Command::new("ssh")
        .arg("-S")
        .arg(control_socket)
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg(host)
        .arg(&script)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(io::Error::new(
            io::ErrorKind::Other,
            if stderr.is_empty() {
                "remote find failed".into()
            } else {
                stderr
            },
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let is_dir = line.ends_with('/');
        let path = if is_dir {
            line.trim_end_matches('/').to_string()
        } else {
            line.to_string()
        };
        out.push(FindMatch { path, is_dir });
        if out.len() >= max.min(FIND_MAX_RESULTS) {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn list_local_dir_reads_temp() {
        let base = std::env::temp_dir().join(format!("pureterm-remote-fs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("a.txt"), b"x").unwrap();
        fs::create_dir(base.join("sub")).unwrap();

        let entries = list_local_dir(&base).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"a.txt"));
        assert!(names.contains(&"sub"));
        let sub = entries.iter().find(|e| e.name == "sub").unwrap();
        assert!(sub.is_dir);
        let file = entries.iter().find(|e| e.name == "a.txt").unwrap();
        assert!(!file.is_dir);
        assert_eq!(file.size, Some(1));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn validate_rejects_newlines() {
        assert!(validate_remote_path("/tmp\n").is_err());
    }

    #[test]
    fn shell_single_quote_escapes() {
        assert_eq!(shell_single_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn read_local_file_truncates() {
        let base = std::env::temp_dir().join(format!("pureterm-remote-read-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let file = base.join("big.txt");
        fs::write(&file, vec![b'x'; 100]).unwrap();

        let result = read_local_file(file.to_str().unwrap(), 32).unwrap();
        assert_eq!(result.content.len(), 32);
        assert!(result.truncated);
        assert_eq!(result.size, Some(100));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn write_local_file_roundtrip() {
        let base = std::env::temp_dir().join(format!("pureterm-remote-write-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let file = base.join("out.txt");
        write_local_file(file.to_str().unwrap(), "hello remote").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "hello remote");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn find_local_matches_name() {
        let base = std::env::temp_dir().join(format!("pureterm-remote-find-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("Cargo.toml"), b"").unwrap();
        fs::create_dir(base.join("src")).unwrap();
        let hits = find_local(base.to_str().unwrap(), "cargo", 10).unwrap();
        assert!(hits.iter().any(|h| h.path.contains("Cargo.toml")));
        let _ = fs::remove_dir_all(&base);
    }
}
