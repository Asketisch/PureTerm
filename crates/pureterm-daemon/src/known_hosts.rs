//! GAP-5.2: trust-on-first-use (TOFU) host-key store for the SOCKS5 tunnel.
//!
//! The tunnel's `russh` client verifies the server's host key against this file.
//! On first connect the key is recorded (TOFU); on subsequent connects a
//! matching key is accepted silently and a *different* key is rejected (strict —
//! the daemon has no interactive prompt path, so a mismatch surfaces via the
//! tunnel's `Failed` status).
//!
//! Scope (MVP): a single isolated file under the daemon data dir (NOT the user's
//! `~/.ssh/known_hosts` — PureTerm must not mutate it). Lines use the OpenSSH
//! one-host format for human-readability and future migration:
//!
//! ```text
//! <host-pattern>  <algorithm>  <base64-blob>
//! ```
//!
//! `<host-pattern>` is a literal hostname, bracketed `[host]:port` when the port
//! is not 22 (OpenSSH convention). Wildcard / hashed-host (`|1|...`) patterns
//! are *not* written or matched here — only exact literal comparison. That is a
//! documented limitation.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use russh::keys::ssh_key::{HashAlg, PublicKey};
use russh::keys::PublicKeyBase64;

/// Result of checking a server host key against the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyCheck {
    /// Known host whose recorded key matches → accept.
    Match,
    /// Known host whose recorded key differs → reject. Carries the stored
    /// fingerprint for the failure message.
    Mismatch(String),
    /// Never seen before → TOFU: the key was just recorded, accept this once.
    Unknown,
}

/// One recorded (host, key) pair.
#[derive(Debug, Clone)]
struct KnownEntry {
    host: String,
    blob_b64: String,
}

/// TOFU host-key store. The file is loaded into memory on `open`; appends on
/// `Unknown` are written through. Interior-locked so the `&mut self`
/// `check_server_key` callback can share one `Arc<KnownHosts>` across tunnels.
pub struct KnownHosts {
    path: PathBuf,
    entries: Mutex<Vec<KnownEntry>>,
}

impl KnownHosts {
    /// Load the store from `path`. A missing file opens empty (first run).
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

    /// Check `key` for `host_pattern` and, on first sight, record it (TOFU).
    /// - `Match`   → known host, key matches → accept (no write).
    /// - `Mismatch`→ known host, key differs → reject (no write).
    /// - `Unknown` → first sight → append the key, accept this once.
    pub fn check_and_record(&self, host_pattern: &str, key: &PublicKey) -> KeyCheck {
        let blob = key.public_key_base64();
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.iter().find(|e| e.host == host_pattern) {
            if entry.blob_b64 == blob {
                return KeyCheck::Match;
            }
            return KeyCheck::Mismatch(fingerprint_of(&entry.blob_b64));
        }
        // Unknown: TOFU record (best-effort), then accept. Append under the
        // in-process lock so two concurrent first-connects can't interleave
        // half-lines. A failed write logs and still returns Unknown (accept) —
        // failing the connection over a known_hosts I/O glitch would be worse.
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

    /// Path of the backing file (test/debug).
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Render the host pattern for a (host, port) pair: bare `host` for the default
/// SSH port 22, otherwise `[host]:port` (OpenSSH convention for non-default
/// ports).
pub(crate) fn format_host_pattern(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

/// Parse known_hosts text into entries. Malformed lines (missing fields) are
/// skipped rather than fatal — the daemon must not refuse to start over a
/// hand-edited bad line.
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

/// Append one `<host> <algo> <blob>` line and fsync.
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

/// SHA256 fingerprint of a recorded key, for the mismatch message. Falls back to
/// the raw blob if the stored line is unparseable (corruption).
fn fingerprint_of(blob_b64: &str) -> String {
    russh::keys::parse_public_key_base64(blob_b64)
        .ok()
        .map(|k| k.fingerprint(HashAlg::Sha256).to_string())
        .unwrap_or_else(|| blob_b64.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two distinct, valid ed25519 public-key blobs (deterministic — no RNG).
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
        // No file → first check is Unknown (not an error).
        assert_eq!(kh.check_and_record("gw", &key_a()), KeyCheck::Unknown);
    }

    #[test]
    fn unknown_appends_then_subsequent_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("kh");
        let kh = KnownHosts::open(path.clone()).unwrap();

        // First sight → Unknown, and the key is recorded.
        assert_eq!(kh.check_and_record("gw.example", &key_a()), KeyCheck::Unknown);
        // A fresh load of the same file must now Match the recorded key.
        let kh2 = KnownHosts::open(path).unwrap();
        assert_eq!(kh2.check_and_record("gw.example", &key_a()), KeyCheck::Match);
        // Exactly one entry line for this host.
        let written = std::fs::read_to_string(tmp.path().join("kh")).unwrap();
        let lines: Vec<&str> = written.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 1, "no duplicate lines: {written}");
    }

    #[test]
    fn mismatch_rejects_and_leaves_file_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("kh");
        // Seed: host known with key A.
        let kh = KnownHosts::open(path.clone()).unwrap();
        assert_eq!(kh.check_and_record("gw.example", &key_a()), KeyCheck::Unknown);

        let kh2 = KnownHosts::open(path.clone()).unwrap();
        // Present key B → Mismatch (not Unknown: the host IS known).
        match kh2.check_and_record("gw.example", &key_b()) {
            KeyCheck::Mismatch(_) => {}
            other => panic!("expected Mismatch, got {other:?}"),
        }
        // File must not be mutated by a mismatch.
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
        // Spin so a subsequent write would observable change mtime.
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
        // Same host, different ports → distinct entries.
        assert_eq!(kh.check_and_record("[gw]:22", &key_a()), KeyCheck::Unknown);
        assert_eq!(kh.check_and_record("[gw]:2222", &key_a()), KeyCheck::Unknown);
        // Different host, same key → still Unknown (independent host slot).
        assert_eq!(kh.check_and_record("other.example", &key_a()), KeyCheck::Unknown);

        let kh2 = KnownHosts::open(path).unwrap();
        assert_eq!(kh2.check_and_record("[gw]:22", &key_a()), KeyCheck::Match);
        assert_eq!(kh2.check_and_record("[gw]:2222", &key_a()), KeyCheck::Match);
        assert_eq!(kh2.check_and_record("other.example", &key_a()), KeyCheck::Match);
    }
}
