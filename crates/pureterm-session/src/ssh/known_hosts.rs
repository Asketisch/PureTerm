use anyhow::{Context, Result};
use russh::keys::ssh_key::PublicKey;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{BufRead, Write};
use std::path::PathBuf;

/// A known host entry.
#[derive(Debug, Clone)]
pub struct KnownHostEntry {
    pub host_pattern: String,
    pub key_type: String,
    pub public_key: String,
}

/// Manages known_hosts file for SSH host key verification.
pub struct KnownHosts {
    entries: Vec<KnownHostEntry>,
    path: PathBuf,
}

#[derive(Debug)]
pub enum KeyVerificationResult {
    /// Key matches a known entry.
    Match,
    /// Key does not match (MITM or changed key).
    Mismatch { expected: String },
    /// Host not in known_hosts (first connection).
    Unknown,
}

impl fmt::Display for KeyVerificationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyVerificationResult::Match => write!(f, "Key matches"),
            KeyVerificationResult::Mismatch { expected } => {
                write!(f, "Key mismatch! Expected: {}", expected)
            }
            KeyVerificationResult::Unknown => write!(f, "Host unknown"),
        }
    }
}

impl KnownHosts {
    /// Create a new KnownHosts manager, loading from the default path.
    pub fn new() -> Self {
        let path = Self::default_path();
        let entries = Self::load_entries(&path).unwrap_or_default();
        Self { entries, path }
    }

    /// Create with a custom path (for testing).
    pub fn with_path(path: PathBuf) -> Self {
        let entries = Self::load_entries(&path).unwrap_or_default();
        Self { entries, path }
    }

    /// Default known_hosts path: ~/.ssh/known_hosts
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/"))
            .join(".ssh")
            .join("known_hosts")
    }

    /// Check if a host's public key is known and matches.
    pub fn verify_host_key(&self, host: &str, port: u16, key: &PublicKey) -> KeyVerificationResult {
        let key_base64 = Self::encode_key(key);
        let host_patterns = Self::host_patterns(host, port);

        for entry in &self.entries {
            if host_patterns.iter().any(|p| Self::pattern_matches(p, &entry.host_pattern)) {
                if entry.public_key == key_base64 {
                    return KeyVerificationResult::Match;
                } else {
                    return KeyVerificationResult::Mismatch {
                        expected: entry.public_key.clone(),
                    };
                }
            }
        }
        KeyVerificationResult::Unknown
    }

    /// Add a host key to known_hosts (TOFU - Trust On First Use).
    pub fn add_host_key(&mut self, host: &str, port: u16, key: &PublicKey) -> Result<()> {
        let key_base64 = Self::encode_key(key);
        let key_type = Self::key_type_name(key);
        let host_pattern = if port == 22 {
            host.to_string()
        } else {
            format!("[{}]:{}", host, port)
        };

        let entry = KnownHostEntry {
            host_pattern,
            key_type,
            public_key: key_base64,
        };
        self.entries.push(entry);
        self.save()
    }

    /// Save entries back to the known_hosts file.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .context("Failed to open known_hosts")?;

        // Only append the last entry (the one just added)
        if let Some(entry) = self.entries.last() {
            writeln!(file, "{} {} {}", entry.host_pattern, entry.key_type, entry.public_key)?;
        }
        Ok(())
    }

    fn load_entries(path: &PathBuf) -> Result<Vec<KnownHostEntry>> {
        let file = fs::File::open(path).context("known_hosts not found")?;
        let reader = std::io::BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Format: <host_pattern> <key_type> <base64_key>
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() == 3 {
                entries.push(KnownHostEntry {
                    host_pattern: parts[0].to_string(),
                    key_type: parts[1].to_string(),
                    public_key: parts[2].to_string(),
                });
            }
        }
        Ok(entries)
    }

    /// Generate host patterns to match against known_hosts.
    fn host_patterns(host: &str, port: u16) -> Vec<String> {
        let mut patterns = vec![host.to_string()];
        if port != 22 {
            patterns.push(format!("[{}]:{}", host, port));
        }
        // Also support hashed host entries (marked with |1|)
        patterns
    }

    /// Simple pattern matching for known_hosts entries.
    fn pattern_matches(pattern: &str, entry: &str) -> bool {
        if entry == pattern {
            return true;
        }
        // Handle comma-separated hosts
        for part in entry.split(',') {
            if part == pattern {
                return true;
            }
        }
        // Handle wildcard patterns
        if entry.contains('*') || entry.contains('?') {
            let regex_pattern = entry
                .replace('.', r"\.")
                .replace('*', ".*")
                .replace('?', ".");
            if let Ok(re) = regex::Regex::new(&format!("^{}$", regex_pattern)) {
                return re.is_match(pattern);
            }
        }
        false
    }

    fn encode_key(key: &PublicKey) -> String {
        let bytes = key.to_bytes().unwrap_or_default();
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes)
    }

    fn key_type_name(key: &PublicKey) -> String {
        format!("{:?}", key.algorithm()).to_lowercase().replace('-', "")
    }
}

impl Default for KnownHosts {
    fn default() -> Self {
        Self::new()
    }
}
