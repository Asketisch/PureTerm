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
use zeroize::Zeroizing;

/// 加密凭据存储,支持主密码(跨平台)
pub struct CredentialStore {
    creds_path: PathBuf,
    /// Bug #7 修复:将主密钥包在 `Zeroizing` 中,使其在丢弃(`lock`、
    /// panic 展开或 `Drop`)时自动清零内存。之前的 `Vec<u8>` 只在显式
    /// `lock` 调用时清空字节,其余时间密钥都留在内存中。
    master_key: Option<Zeroizing<Vec<u8>>>,
}

/// 存储的凭据条目
#[derive(serde::Serialize, serde::Deserialize)]
struct CredentialEntry {
    host: String,
    encrypted: Vec<u8>,
    nonce: Vec<u8>,
}

/// 凭据库文件格式
#[derive(serde::Serialize, serde::Deserialize)]
struct CredentialFile {
    salt: String,
    verifier: Vec<u8>, // 加密的已知明文,用于校验主密码
    nonce: Vec<u8>,    // 校验器加密所用的 nonce
    entries: Vec<CredentialEntry>,
}

impl CredentialStore {
    /// 在给定路径创建新的凭据库
    pub fn new(creds_path: PathBuf) -> Self {
        Self {
            creds_path,
            master_key: None,
        }
    }

    /// 检查凭据文件是否存在
    pub fn exists(&self) -> bool {
        self.creds_path.exists()
    }

    /// 检查凭据库是否已解锁
    pub fn is_unlocked(&self) -> bool {
        self.master_key.is_some()
    }

    /// 首次设置主密码(同时创建加密文件)
    pub fn setup_master(&self, master: &str) -> io::Result<()> {
        if self.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "credential file already exists",
            ));
        }

        // 生成随机盐值
        let salt = SaltString::generate(&mut OsRng);
        let salt_bytes = salt.as_str().as_bytes();

        // 使用 Argon2id 派生密钥
        let key = derive_key(master, salt_bytes);

        // 创建校验器(加密的已知明文,用于校验密码)
        let verifier_plaintext = b"PURETERM_CREDENTIAL_STORE_V1";
        let (verifier_enc, verifier_nonce) = encrypt(key.as_slice(), verifier_plaintext);

        // 创建空的凭据文件
        let cred_file = CredentialFile {
            salt: salt.to_string(),
            verifier: verifier_enc,
            nonce: verifier_nonce,
            entries: Vec::new(),
        };

        // 写入文件
        let file = File::create(&self.creds_path)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, &cred_file)?;
        writer.flush()?;

        // 注意:出于安全考虑,setup_master 之后不会在内存中保留密钥。
        // 用户必须调用 unlock() 解锁凭据库并将密钥载入内存。
        Ok(())
    }

    /// 使用主密码解锁
    pub fn unlock(&mut self, master: &str) -> io::Result<()> {
        let file = File::open(&self.creds_path)?;
        let mut reader = BufReader::new(file);
        let cred_file: CredentialFile = serde_json::from_reader(&mut reader)?;

        // 从密码派生密钥
        let salt_bytes = cred_file.salt.as_bytes();
        let key = derive_key(master, salt_bytes);

        // 通过尝试解密校验器来验证密码
        let decrypted = match decrypt(key.as_slice(), &cred_file.verifier, &cred_file.nonce) {
            Ok(pt) => pt,
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "invalid master password",
                ));
            },
        };

        // 校验明文与期望值一致
        if &decrypted[..] != b"PURETERM_CREDENTIAL_STORE_V1" {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "invalid master password",
            ));
        }

        // 密码验证通过,将密钥包在 Zeroizing 中,丢弃时自动清零缓冲区。
        self.master_key = Some(key);
        Ok(())
    }

    /// 锁定(从内存中清除主密钥)。`Zeroizing` 包装保证底层缓冲区在
    /// 丢弃时清零,这里只需丢弃句柄即可。
    pub fn lock(&mut self) {
        self.master_key = None;
    }

    /// 存储加密凭据
    pub fn store(&self, host: &str, password: &[u8]) -> io::Result<()> {
        let key = self
            .master_key
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "store is locked"))?;

        // 读取已有文件
        let file = File::open(&self.creds_path)?;
        let mut reader = BufReader::new(file);
        let mut cred_file: CredentialFile = serde_json::from_reader(&mut reader)?;

        // 移除该主机已有的条目(若存在)
        cred_file.entries.retain(|e| e.host != host);

        // 加密密码
        let (encrypted, nonce) = encrypt(key.as_slice(), password);

        // 新增条目
        cred_file.entries.push(CredentialEntry {
            host: host.to_string(),
            encrypted,
            nonce,
        });

        // 写回文件
        let file = File::create(&self.creds_path)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, &cred_file)?;
        writer.flush()?;

        Ok(())
    }

    /// 取出解密后的凭据
    pub fn retrieve(&self, host: &str) -> io::Result<Vec<u8>> {
        let key = self
            .master_key
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "store is locked"))?;

        // 读取已有文件
        let file = File::open(&self.creds_path)?;
        let mut reader = BufReader::new(file);
        let cred_file: CredentialFile = serde_json::from_reader(&mut reader)?;

        // 查找主机的条目
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

        // 解密
        let decrypted = decrypt(key.as_slice(), &entry.encrypted, &entry.nonce).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "failed to decrypt credential")
        })?;

        Ok(decrypted)
    }
}

impl Drop for CredentialStore {
    /// Bug #7 修复:确保主密钥在丢弃时被清除。`Zeroizing` 包装已负责
    /// 清零缓冲区,这里只需丢弃该字段(例如在 panic 展开时,或所有者
    /// 未显式调用 `lock` 即被丢弃时)。
    fn drop(&mut self) {
        self.master_key = None;
    }
}

/// 使用 Argon2id 从密码派生 256 位密钥。
///
/// 返回值包在 `Zeroizing` 中,丢弃时自动清零缓冲区。
fn derive_key(password: &str, salt: &[u8]) -> Zeroizing<Vec<u8>> {
    let argon2 = argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(65536, 3, 4, Some(32)).expect("invalid argon2 params"),
    );

    let mut key = Zeroizing::new(vec![0u8; 32]);
    argon2
        .hash_password_into(password.as_bytes(), salt, key.as_mut_slice())
        .expect("argon2 hash failed");
    key
}

/// 使用 AES-256-GCM 加密明文
fn encrypt(key: &[u8], plaintext: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let cipher = Aes256Gcm::new_from_slice(key).expect("invalid key length");
    // 生成随机的 12 字节 nonce
    let mut nonce_bytes = [0u8; 12];
    OsRng.try_fill_bytes(&mut nonce_bytes).expect("failed to generate nonce");
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .expect("encryption failed");

    (ciphertext, nonce_bytes.to_vec())
}

/// 使用 AES-256-GCM 解密密文
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

        let (ciphertext, nonce) = encrypt(key.as_slice(), plaintext);
        let decrypted = decrypt(key.as_slice(), &ciphertext, &nonce).expect("decryption failed");

        assert_eq!(plaintext, decrypted.as_slice());
    }

    #[test]
    fn test_credential_store_setup_unlock_lock() {
        let temp_dir = std::env::temp_dir();
        let creds_path = temp_dir.join("test_creds_").with_extension("json");

        let store = CredentialStore::new(creds_path.clone());

        // 设置主密码
        store.setup_master("test_master_password").expect("setup failed");

        assert!(store.exists());

        // 用正确密码解锁
        let mut store = store;
        store.unlock("test_master_password").expect("unlock failed");
        assert!(store.is_unlocked());

        // 锁定
        store.lock();
        assert!(!store.is_unlocked());

        // 清理
        let _ = fs::remove_file(&creds_path);
    }

    #[test]
    fn test_credential_store_store_retrieve() {
        let temp_dir = std::env::temp_dir();
        let creds_path = temp_dir.join(format!(
            "test_creds_store_retrieve_{}",
            std::process::id()
        )).with_extension("json");

        let store = CredentialStore::new(creds_path.clone());
        store.setup_master("store_retrieve_password").expect("setup failed");

        let mut store = store;
        store.unlock("store_retrieve_password").expect("unlock failed");

        // 存储一条凭据
        store
            .store("example.com", b"secret_password_123")
            .expect("store failed");

        // 取出该凭据
        let retrieved = store.retrieve("example.com").expect("retrieve failed");
        assert_eq!(retrieved, b"secret_password_123");

        // 取不存在的凭据应失败
        assert!(store.retrieve("nonexistent.com").is_err());

        // 锁定后验证无法存储/取出
        store.lock();
        assert!(store.store("test.com", b"test").is_err());
        assert!(store.retrieve("example.com").is_err());

        // 清理
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

        // 清理
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
