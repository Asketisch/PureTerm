use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Write},
    path::PathBuf,
};

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm,
};
use aes_gcm::aead::rand_core::RngCore;
use argon2::password_hash::SaltString;

/// Encrypted credential storage with master password (cross-platform)
pub struct CredentialStore {
    creds_path: PathBuf,
    master_key: Option<Vec<u8>>,
}

/// Stored credential entry
#[derive(serde::Serialize, serde::Deserialize)]
struct CredentialEntry {
    host: String,
    encrypted: Vec<u8>,
    nonce: Vec<u8>,
}

/// Credential store file format
#[derive(serde::Serialize, serde::Deserialize)]
struct CredentialFile {
    salt: String,
    verifier: Vec<u8>, // encrypted known plaintext to verify password
    nonce: Vec<u8>,    // nonce used for verifier encryption
    entries: Vec<CredentialEntry>,
}

impl CredentialStore {
    /// Create a new credential store at the given path
    pub fn new(creds_path: PathBuf) -> Self {
        Self {
            creds_path,
            master_key: None,
        }
    }

    /// Check if credentials file exists
    pub fn exists(&self) -> bool {
        self.creds_path.exists()
    }

    /// Check if store is unlocked
    pub fn is_unlocked(&self) -> bool {
        self.master_key.is_some()
    }

    /// Setup master password for first time (creates encrypted file)
    pub fn setup_master(&self, master: &str) -> io::Result<()> {
        if self.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "credential file already exists",
            ));
        }

        // Generate random salt
        let salt = SaltString::generate(&mut OsRng);
        let salt_bytes = salt.as_str().as_bytes();

        // Derive key using Argon2id
        let key = derive_key(master, salt_bytes);

        // Create a verifier (encrypted known plaintext to verify password)
        let verifier_plaintext = b"PURETERM_CREDENTIAL_STORE_V1";
        let (verifier_enc, verifier_nonce) = encrypt(&key, verifier_plaintext);

        // Create empty credential file
        let cred_file = CredentialFile {
            salt: salt.to_string(),
            verifier: verifier_enc,
            nonce: verifier_nonce,
            entries: Vec::new(),
        };

        // Write to file
        let file = File::create(&self.creds_path)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, &cred_file)?;
        writer.flush()?;

        // Note: We don't store the key in memory after setup_master for security.
        // User must call unlock() to unlock the store and load the key into memory.
        Ok(())
    }

    /// Unlock with master password
    pub fn unlock(&mut self, master: &str) -> io::Result<()> {
        let file = File::open(&self.creds_path)?;
        let mut reader = BufReader::new(file);
        let cred_file: CredentialFile = serde_json::from_reader(&mut reader)?;

        // Derive key from password
        let salt_bytes = cred_file.salt.as_bytes();
        let key = derive_key(master, salt_bytes);

        // Verify password by attempting to decrypt the verifier
        let decrypted = match decrypt(&key, &cred_file.verifier, &cred_file.nonce) {
            Ok(pt) => pt,
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "invalid master password",
                ));
            },
        };

        // Verify the plaintext matches expected value
        if &decrypted[..] != b"PURETERM_CREDENTIAL_STORE_V1" {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "invalid master password",
            ));
        }

        // Password verified, store key
        self.master_key = Some(key);
        Ok(())
    }

    /// Lock (clear master key from memory)
    pub fn lock(&mut self) {
        if let Some(ref mut key) = self.master_key {
            key.fill(0);
        }
        self.master_key = None;
    }

    /// Store encrypted credential
    pub fn store(&self, host: &str, password: &[u8]) -> io::Result<()> {
        let key = self
            .master_key
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "store is locked"))?;

        // Read existing file
        let file = File::open(&self.creds_path)?;
        let mut reader = BufReader::new(file);
        let mut cred_file: CredentialFile = serde_json::from_reader(&mut reader)?;

        // Remove existing entry for this host if present
        cred_file.entries.retain(|e| e.host != host);

        // Encrypt the password
        let (encrypted, nonce) = encrypt(key, password);

        // Add new entry
        cred_file.entries.push(CredentialEntry {
            host: host.to_string(),
            encrypted,
            nonce,
        });

        // Write back
        let file = File::create(&self.creds_path)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, &cred_file)?;
        writer.flush()?;

        Ok(())
    }

    /// Retrieve decrypted credential
    pub fn retrieve(&self, host: &str) -> io::Result<Vec<u8>> {
        let key = self
            .master_key
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "store is locked"))?;

        // Read existing file
        let file = File::open(&self.creds_path)?;
        let mut reader = BufReader::new(file);
        let cred_file: CredentialFile = serde_json::from_reader(&mut reader)?;

        // Find entry for host
        let entry = cred_file
            .entries
            .iter()
            .find(|e| e.host == host)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("no credential found for host: {}", host),
                )
            })?;

        // Decrypt
        let decrypted = decrypt(key, &entry.encrypted, &entry.nonce).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "failed to decrypt credential")
        })?;

        Ok(decrypted)
    }
}

/// Derive a 256-bit key from password using Argon2id
fn derive_key(password: &str, salt: &[u8]) -> Vec<u8> {
    let argon2 = argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(65536, 3, 4, Some(32)).expect("invalid argon2 params"),
    );

    let mut key = vec![0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .expect("argon2 hash failed");
    key
}

/// Encrypt plaintext using AES-256-GCM
fn encrypt(key: &[u8], plaintext: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let cipher = Aes256Gcm::new_from_slice(key).expect("invalid key length");
    // Generate a random 12-byte nonce
    let mut nonce_bytes = [0u8; 12];
    OsRng.try_fill_bytes(&mut nonce_bytes).expect("failed to generate nonce");
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .expect("encryption failed");

    (ciphertext, nonce_bytes.to_vec())
}

/// Decrypt ciphertext using AES-256-GCM
fn decrypt(key: &[u8], ciphertext: &[u8], nonce_bytes: &[u8]) -> Result<Vec<u8>, ()> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| ())?;
    let nonce = aes_gcm::Nonce::from_slice(nonce_bytes);

    cipher.decrypt(nonce, ciphertext).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_encryption_roundtrip(password: &str, plaintext: &[u8]) {
        let salt = b"test_salt_for_argon2";
        let key = derive_key(password, salt);

        let (ciphertext, nonce) = encrypt(&key, plaintext);
        let decrypted = decrypt(&key, &ciphertext, &nonce).expect("decryption failed");

        assert_eq!(plaintext, decrypted.as_slice());
    }

    #[test]
    fn test_credential_store_setup_unlock_lock() {
        let temp_dir = std::env::temp_dir();
        let creds_path = temp_dir.join("test_creds_").with_extension("json");

        let store = CredentialStore::new(creds_path.clone());

        // Setup master password
        store.setup_master("test_master_password").expect("setup failed");

        assert!(store.exists());

        // Unlock with correct password
        let mut store = store;
        store.unlock("test_master_password").expect("unlock failed");
        assert!(store.is_unlocked());

        // Lock
        store.lock();
        assert!(!store.is_unlocked());

        // Clean up
        let _ = fs::remove_file(&creds_path);
    }

    #[test]
    fn test_credential_store_store_retrieve() {
        let temp_dir = std::env::temp_dir();
        let creds_path = temp_dir.join("test_creds_store_retrieve_" ).with_extension("json");

        let store = CredentialStore::new(creds_path.clone());
        store.setup_master("store_retrieve_password").expect("setup failed");

        let mut store = store;
        store.unlock("store_retrieve_password").expect("unlock failed");

        // Store a credential
        store
            .store("example.com", b"secret_password_123")
            .expect("store failed");

        // Retrieve it
        let retrieved = store.retrieve("example.com").expect("retrieve failed");
        assert_eq!(retrieved, b"secret_password_123");

        // Retrieve non-existent should fail
        assert!(store.retrieve("nonexistent.com").is_err());

        // Lock and verify cannot store/retrieve
        store.lock();
        assert!(store.store("test.com", b"test").is_err());
        assert!(store.retrieve("example.com").is_err());

        // Clean up
        let _ = fs::remove_file(&creds_path);
    }

    #[test]
    fn test_wrong_password_fails() {
        let temp_dir = std::env::temp_dir();
        let creds_path = temp_dir
            .join("test_creds_wrong_password_")
            .with_extension("json");

        let store = CredentialStore::new(creds_path.clone());
        store
            .setup_master("correct_password")
            .expect("setup failed");

        let mut store = store;
        let result = store.unlock("wrong_password");
        assert!(result.is_err());

        // Clean up
        let _ = fs::remove_file(&creds_path);
    }

    #[test]
    fn test_encryption_roundtrip_basic() {
        test_encryption_roundtrip("password", b"hello world");
    }

    #[test]
    fn test_encryption_roundtrip_empty() {
        test_encryption_roundtrip("password", b"");
    }

    #[test]
    fn test_encryption_roundtrip_binary() {
        test_encryption_roundtrip(
            "complex_password!@#$",
            b"\x00\x01\x02\xff\xfe\xfd binary \x00 data",
        );
    }

    #[test]
    fn test_encryption_roundtrip_longer() {
        let plaintext = b"The quick brown fox jumps over the lazy dog. 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz";
        test_encryption_roundtrip("longer_test_password", plaintext);
    }
}
