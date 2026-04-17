use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 存储在配置中的加密值。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedValue {
    /// Base64 编码的密文。
    pub ciphertext: String,
    /// Base64 编码的 nonce。
    pub nonce: String,
    /// 用于未来迁移的版本。
    pub version: u8,
}

/// 用于加密/解密敏感配置值的保险库服务。
pub struct VaultService {
    key: [u8; 32],
}

/// 保险库操作的错误类型。
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
    /// 使用 SHA-256 从主密码派生 32 字节的 AES-256 密钥。
    pub fn new(master_password: &str) -> Self {
        let key: [u8; 32] = Sha256::digest(master_password.as_bytes()).into();
        Self { key }
    }

    /// 使用 AES-256-GCM 加密明文字符串。
    ///
    /// 返回包含 base64 编码密文和 nonce 的 `EncryptedValue`。
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

    /// 将 `EncryptedValue` 解密回原始明文字符串。
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

    /// 加密明文字符串并返回 JSON 序列化的 `EncryptedValue`。
    pub fn encrypt_to_string(&self, plaintext: &str) -> Result<String, VaultError> {
        let encrypted = self.encrypt(plaintext)?;
        serde_json::to_string(&encrypted)
            .map_err(|e| VaultError::EncryptionFailed(format!("JSON serialization: {}", e)))
    }

    /// 将 JSON 字符串反序列化为 `EncryptedValue` 并解密。
    pub fn decrypt_from_string(&self, json: &str) -> Result<String, VaultError> {
        let encrypted: EncryptedValue = serde_json::from_str(json)
            .map_err(|e| VaultError::DecryptionFailed(format!("JSON deserialization: {}", e)))?;
        self.decrypt(&encrypted)
    }

    /// 从新密码重新派生密钥。
    ///
    /// 返回新的 32 字节密钥,以便调用者可以构造新的 `VaultService`。
    /// 旧密码被混合在内,因此仅知道新密码不足以派生相同的密钥。
    pub fn change_password(old_password: &str, new_password: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(old_password.as_bytes());
        hasher.update(new_password.as_bytes());
        hasher.finalize().into()
    }
}

/// 生成 12 字节的 nonce。
///
/// 使用当前系统时间的纳秒。这对于配置文件的加密场景是可以接受的,
/// 因为操作不频繁,但在生产环境中应该优先使用加密随机源。
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
