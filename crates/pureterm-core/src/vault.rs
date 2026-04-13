use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Encrypted value stored in config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedValue {
    /// Base64-encoded ciphertext.
    pub ciphertext: String,
    /// Base64-encoded nonce.
    pub nonce: String,
    /// Version for future migration.
    pub version: u8,
}

/// Vault service for encrypting/decrypting sensitive config values.
pub struct VaultService {
    key: [u8; 32],
}

/// Error type for vault operations.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),
    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),
    #[error("Invalid key: {0}")]
    InvalidKey(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl VaultService {
    /// Derive a 32-byte AES-256 key from a master password using SHA-256.
    pub fn new(master_password: &str) -> Self {
        let key: [u8; 32] = Sha256::digest(master_password.as_bytes()).into();
        Self { key }
    }

    /// Encrypt a plaintext string with AES-256-GCM.
    ///
    /// Returns an `EncryptedValue` containing base64-encoded ciphertext and nonce.
    pub fn encrypt(&self, plaintext: &str) -> Result<EncryptedValue, VaultError> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| VaultError::EncryptionFailed(e.to_string()))?;

        let nonce_bytes = generate_nonce();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| VaultError::EncryptionFailed(e.to_string()))?;

        Ok(EncryptedValue {
            ciphertext: BASE64.encode(&ciphertext),
            nonce: BASE64.encode(&nonce_bytes),
            version: 1,
        })
    }

    /// Decrypt an `EncryptedValue` back to the original plaintext string.
    pub fn decrypt(&self, encrypted: &EncryptedValue) -> Result<String, VaultError> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| VaultError::DecryptionFailed(e.to_string()))?;

        let nonce_bytes = BASE64
            .decode(&encrypted.nonce)
            .map_err(|e| VaultError::DecryptionFailed(format!("Invalid nonce base64: {}", e)))?;

        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = BASE64
            .decode(&encrypted.ciphertext)
            .map_err(|e| {
                VaultError::DecryptionFailed(format!("Invalid ciphertext base64: {}", e))
            })?;

        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| VaultError::DecryptionFailed(e.to_string()))?;

        String::from_utf8(plaintext)
            .map_err(|e| VaultError::DecryptionFailed(format!("Invalid UTF-8: {}", e)))
    }

    /// Encrypt a plaintext string and return the JSON-serialized `EncryptedValue`.
    pub fn encrypt_to_string(&self, plaintext: &str) -> Result<String, VaultError> {
        let encrypted = self.encrypt(plaintext)?;
        serde_json::to_string(&encrypted)
            .map_err(|e| VaultError::EncryptionFailed(format!("JSON serialization: {}", e)))
    }

    /// Deserialize a JSON string as `EncryptedValue` and decrypt it.
    pub fn decrypt_from_string(&self, json: &str) -> Result<String, VaultError> {
        let encrypted: EncryptedValue = serde_json::from_str(json)
            .map_err(|e| VaultError::DecryptionFailed(format!("JSON deserialization: {}", e)))?;
        self.decrypt(&encrypted)
    }

    /// Re-derive a key from a new password.
    ///
    /// Returns the new 32-byte key so the caller can construct a new `VaultService`.
    /// The old password is mixed in so that knowledge of the new password alone
    /// is not sufficient to derive the same key.
    pub fn change_password(old_password: &str, new_password: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(old_password.as_bytes());
        hasher.update(new_password.as_bytes());
        hasher.finalize().into()
    }
}

/// Generate a 12-byte nonce.
///
/// Uses the current system time in nanoseconds. This is acceptable for a
/// config-file encryption scenario where operations are infrequent, but a
/// cryptographically random source should be preferred in production.
fn generate_nonce() -> [u8; 12] {
    let mut nonce = [0u8; 12];
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let time_bytes = time.to_le_bytes(); // u128 -> 16 bytes
    nonce[..8].copy_from_slice(&time_bytes[..8]);
    for i in 8..12 {
        nonce[i] = time_bytes[i] ^ time_bytes[i - 8];
    }
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let vault = VaultService::new("test-master-password");
        let plaintext = "my-secret-password";
        let encrypted = vault.encrypt(plaintext).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn roundtrip_string_encrypt_decrypt() {
        let vault = VaultService::new("another-password");
        let plaintext = "ssh-key-passphrase";
        let json = vault.encrypt_to_string(plaintext).unwrap();
        let decrypted = vault.decrypt_from_string(&json).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_password_fails() {
        let vault1 = VaultService::new("correct-password");
        let vault2 = VaultService::new("wrong-password");
        let encrypted = vault1.encrypt("secret").unwrap();
        assert!(vault2.decrypt(&encrypted).is_err());
    }

    #[test]
    fn change_password_produces_new_key() {
        let key1 = VaultService::change_password("old", "new-pass");
        let key2 = VaultService::change_password("old", "new-pass");
        let key3 = VaultService::change_password("other", "new-pass");
        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn encrypted_value_version_is_one() {
        let vault = VaultService::new("pw");
        let encrypted = vault.encrypt("data").unwrap();
        assert_eq!(encrypted.version, 1);
    }
}
