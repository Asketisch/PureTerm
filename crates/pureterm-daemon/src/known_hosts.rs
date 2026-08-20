//! GAP-5.2:SOCKS5 隧道的首用即信(TOFU)主机密钥存储。
//!
//! 隧道的 `russh` 客户端用此文件校验服务器主机密钥。
//! 首次连接时记录密钥(TOFU);后续连接中
//! 匹配的密钥静默接受,不同的密钥被拒绝(严格模式——
//! 守护进程没有交互式提示路径,因此失配经由
//! 隧道的 `Failed` 状态浮出)。
//!
//! 范围(MVP):守护进程数据目录下的单一隔离文件(不是用户的
//! `~/.ssh/known_hosts`——PureTerm 绝不能改动它)。行采用 OpenSSH
//! 单主机格式,便于人读与未来迁移:
//!
//! ```text
//! <host-pattern>  <algorithm>  <base64-blob>
//! ```
//!
//! `<host-pattern>` 是字面主机名;端口非 22 时使用带方括号的
//! `[host]:port`(OpenSSH 约定)。通配 / 哈希主机(`|1|...`)模式
//! 在此既不写入也不匹配——只做精确字面比较。这是已记录的
//! 限制。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use russh::keys::ssh_key::{HashAlg, PublicKey};
use russh::keys::PublicKeyBase64;

/// 用存储校验服务器主机密钥的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyCheck {
    /// 已知主机且记录密钥匹配 → 接受。
    Match,
    /// 已知主机但密钥不同 → 拒绝。携带已存
    /// 指纹供失败消息使用。
    Mismatch(String),
    /// 从未见过 → TOFU:刚记录该密钥,本次接受。
    Unknown,
}

/// 一条已记录的 (host, key) 对。
#[derive(Debug, Clone)]
struct KnownEntry {
    host: String,
    blob_b64: String,
}

/// TOFU 主机密钥存储。`open` 时把文件载入内存;`Unknown` 时的
/// 追加直写磁盘。内部加锁,使 `&mut self` 的
/// `check_server_key` 回调能在多条隧道间共享一个 `Arc<KnownHosts>`。
pub struct KnownHosts {
    path: PathBuf,
    entries: Mutex<Vec<KnownEntry>>,
}

impl KnownHosts {
    /// 从 `path` 加载存储。文件缺失则打开为空(首次运行)。
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let entries = match std::fs::read_to_string(&path) {
            Ok(s) => parse_lines(&s),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok(Self {
            path,
            entries: Mutex::new(entries),
        })
    }

    /// 校验 `host_pattern` 的 `key`;首次见到时记录之(TOFU)。
    /// - `Match`   → 已知主机且密钥匹配 → 接受(不写)。
    /// - `Mismatch`→ 已知主机但密钥不同 → 拒绝(不写)。
    /// - `Unknown` → 首见 → 追加密钥,本次接受。
    pub fn check_and_record(&self, host_pattern: &str, key: &PublicKey) -> KeyCheck {
        let blob = key.public_key_base64();
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.iter().find(|e| e.host == host_pattern) {
            if entry.blob_b64 == blob {
                return KeyCheck::Match;
            }
            return KeyCheck::Mismatch(fingerprint_of(&entry.blob_b64));
        }
        // Unknown:TOFU 记录(尽力而为),随后接受。在
        // 进程内锁下追加,使两个并发的首次连接不会交错出
        // 半行。写入失败仅记录日志并仍返回 Unknown(接受)——
        // 因 known_hosts 的 I/O 小毛病而断开连接更糟。
        let algo = key.algorithm();
        if let Err(e) = append_line(&self.path, host_pattern, algo.as_str(), &blob) {
            log::warn!("known_hosts: failed to record {host_pattern}: {e}");
        }
        entries.push(KnownEntry {
            host: host_pattern.to_string(),
            blob_b64: blob,
        });
        KeyCheck::Unknown
    }

    /// 后备文件路径(测试/调试)。
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// 渲染 (host, port) 对的主机模式:默认 SSH 端口 22 用裸 `host`,
/// 否则用 `[host]:port`(OpenSSH 对非默认
/// 端口的约定)。
pub(crate) fn format_host_pattern(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

/// 把 known_hosts 文本解析为条目。畸形行(缺字段)被
/// 跳过而非致命——守护进程不能因一条手改的坏行
/// 拒绝启动。
fn parse_lines(s: &str) -> Vec<KnownEntry> {
    let mut out = Vec::new();
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        match (fields.next(), fields.next(), fields.next()) {
            (Some(host), Some(_algo), Some(blob)) => out.push(KnownEntry {
                host: host.to_string(),
                blob_b64: blob.to_string(),
            }),
            _ => continue,
        }
    }
    out
}

/// 追加一行 `<host> <algo> <blob>` 并 fsync。
fn append_line(path: &Path, host: &str, algo: &str, blob: &str) -> io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{host} {algo} {blob}")?;
    f.sync_all()?;
    Ok(())
}

/// 已记录密钥的 SHA256 指纹,用于失配消息。已存行无法解析时
/// 回退为原始 blob(损坏情形)。
fn fingerprint_of(blob_b64: &str) -> String {
    russh::keys::parse_public_key_base64(blob_b64)
        .ok()
        .map(|k| k.fingerprint(HashAlg::Sha256).to_string())
        .unwrap_or_else(|| blob_b64.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两个不同且合法的 ed25519 公钥 blob(确定性——无 RNG)。
    const KEY_A_B64: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f";
    const KEY_B_B64: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIAMKERgfJi00O0JJUFdeZWxzeoGIj5adpKuyucDHztXc";

    fn key_a() -> PublicKey {
        russh::keys::parse_public_key_base64(KEY_A_B64).expect("parse KEY_A")
    }
    fn key_b() -> PublicKey {
        russh::keys::parse_public_key_base64(KEY_B_B64).expect("parse KEY_B")
    }

    #[test]
    fn format_host_pattern_default_port_is_bare_host() {
        assert_eq!(format_host_pattern("gw.example", 22), "gw.example");
    }

    #[test]
    fn format_host_pattern_nondefault_port_is_bracketed() {
        assert_eq!(format_host_pattern("gw.example", 2222), "[gw.example]:2222");
    }

    #[test]
    fn missing_file_opens_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let kh = KnownHosts::open(tmp.path().join("nope")).unwrap();
        // 无文件 → 首次校验为 Unknown(不是错误)。
        assert_eq!(kh.check_and_record("gw", &key_a()), KeyCheck::Unknown);
    }

    #[test]
    fn unknown_appends_then_subsequent_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("kh");
        let kh = KnownHosts::open(path.clone()).unwrap();

        // 首见 → Unknown,且密钥被记录。
        assert_eq!(kh.check_and_record("gw.example", &key_a()), KeyCheck::Unknown);
        // 重新加载同一文件必须 Match 已记录的密钥。
        let kh2 = KnownHosts::open(path).unwrap();
        assert_eq!(kh2.check_and_record("gw.example", &key_a()), KeyCheck::Match);
        // 该主机恰好一行条目。
        let written = std::fs::read_to_string(tmp.path().join("kh")).unwrap();
        let lines: Vec<&str> = written.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 1, "no duplicate lines: {written}");
    }

    #[test]
    fn mismatch_rejects_and_leaves_file_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("kh");
        // 种子:主机已知,持有密钥 A。
        let kh = KnownHosts::open(path.clone()).unwrap();
        assert_eq!(kh.check_and_record("gw.example", &key_a()), KeyCheck::Unknown);

        let kh2 = KnownHosts::open(path.clone()).unwrap();
        // 呈现密钥 B → Mismatch(不是 Unknown:主机确实已知)。
        match kh2.check_and_record("gw.example", &key_b()) {
            KeyCheck::Mismatch(_) => {}
            other => panic!("expected Mismatch, got {other:?}"),
        }
        // 失配不得改动文件。
        let before = std::fs::read_to_string(&path).unwrap();
        let _ = kh2.check_and_record("gw.example", &key_b());
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(before, after, "mismatch must not rewrite the file");
    }

    #[test]
    fn match_does_not_write() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("kh");
        let kh = KnownHosts::open(path.clone()).unwrap();
        assert_eq!(kh.check_and_record("gw.example", &key_a()), KeyCheck::Unknown);

        let mtime0 = std::fs::metadata(&path).unwrap().modified().unwrap();
        // 自旋等待,使后续写入的 mtime 变化可观察。
        std::thread::sleep(std::time::Duration::from_millis(20));
        let kh2 = KnownHosts::open(path).unwrap();
        assert_eq!(kh2.check_and_record("gw.example", &key_a()), KeyCheck::Match);
        let mtime1 = std::fs::metadata(tmp.path().join("kh")).unwrap().modified().unwrap();
        assert_eq!(mtime0, mtime1, "a Match must not touch the file");
    }

    #[test]
    fn distinct_hosts_and_ports_do_not_collide() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("kh");
        let kh = KnownHosts::open(path.clone()).unwrap();
        // 同主机不同端口 → 不同条目。
        assert_eq!(kh.check_and_record("[gw]:22", &key_a()), KeyCheck::Unknown);
        assert_eq!(kh.check_and_record("[gw]:2222", &key_a()), KeyCheck::Unknown);
        // 不同主机同一密钥 → 仍为 Unknown(主机槽位独立)。
        assert_eq!(kh.check_and_record("other.example", &key_a()), KeyCheck::Unknown);

        let kh2 = KnownHosts::open(path).unwrap();
        assert_eq!(kh2.check_and_record("[gw]:22", &key_a()), KeyCheck::Match);
        assert_eq!(kh2.check_and_record("[gw]:2222", &key_a()), KeyCheck::Match);
        assert_eq!(kh2.check_and_record("other.example", &key_a()), KeyCheck::Match);
    }
}
