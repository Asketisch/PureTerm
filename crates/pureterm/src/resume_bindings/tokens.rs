use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
/// 用于令牌计算的 HMAC-SHA256 类型别名。
type HmacSha256 = Hmac<Sha256>;

// ── GAP-7.7 M2:HMAC 签名的审批令牌 ───────────────────────────────────────────────
//
// resume 绑定会运行任意 shell,因此自动执行由
// 持久化的 HMAC-SHA256 令牌把关。GUI 在用户于信息流面板
// 点击「始终允许」时,基于绑定的规范负载铸造令牌;
// 密钥位于 `<data_dir>/resume.key`(0600),已批准摘要位于
// `<data_dir>/resume-tokens.json`。威胁模型:误入 / 意外修改
// 配置,而非坚定的本地攻击者(单用户工作站)。

/// 执行 resume 绑定的授权结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthDecision {
    /// 有效的 HMAC 令牌匹配 → 静默运行。
    Approved,
    /// 无匹配令牌 → 需要经信息流面板人工审批。
    NeedsPrompt,
}

/// 长期 HMAC 密钥路径:`<data_dir>/resume.key`(0600)。
pub(crate) fn secret_path() -> PathBuf {
    pureterm_daemon::discovery::data_dir().join("resume.key")
}

/// 已批准令牌摘要列表路径:`<data_dir>/resume-tokens.json`。
pub(crate) fn tokens_path() -> PathBuf {
    pureterm_daemon::discovery::data_dir().join("resume-tokens.json")
}

/// 绑定用于 HMAC 的规范、稳定的字节编码。用 `\0` 分隔,
/// 该字符不可能出现在 TOML 字符串中,因此 `kind` / `checkpoint` / `shell` 的边界
/// 无歧义。跨运行稳定 —— 不要用 JSON(键序不稳定)。
pub(super) fn canonical_payload(kind: &str, checkpoint: &str, shell: &[String]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(kind.as_bytes());
    v.push(0);
    v.extend_from_slice(checkpoint.as_bytes());
    v.push(0);
    v.extend_from_slice(shell.join("\0").as_bytes());
    v
}

/// 用 `secret` 对绑定规范负载求得的十六进制 HMAC-SHA256。
pub(crate) fn compute_token(secret: &[u8], kind: &str, checkpoint: &str, shell: &[String]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&canonical_payload(kind, checkpoint, shell));
    hex::encode(mac.finalize().into_bytes())
}

/// 检查 `tokens` 是否包含该绑定的摘要。
pub(crate) fn authorize_with(
    secret: &[u8],
    tokens: &[String],
    kind: &str,
    checkpoint: &str,
    shell: &[String],
) -> AuthDecision {
    let t = compute_token(secret, kind, checkpoint, shell);
    if tokens.iter().any(|x| x == &t) {
        AuthDecision::Approved
    } else {
        AuthDecision::NeedsPrompt
    }
}

/// 读取或惰性创建 `path` 处的 32 字节密钥(创建时 0600)。
pub(crate) fn ensure_secret_at(path: &Path) -> [u8; 32] {
    use std::io::{Read, Write};
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut buf = [0u8; 32];
        if f.read_exact(&mut buf).is_ok() {
            return buf;
        }
    }
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
        {
            let _ = f.write_all(&buf);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = std::fs::write(path, buf);
    }
    buf
}

/// 读取已批准的令牌列表;任何错误时返回空。
pub(crate) fn load_tokens_at(path: &Path) -> Vec<String> {
    let Ok(s) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&s).unwrap_or_default()
}

/// 原子持久化已批准的令牌列表。
pub(crate) fn save_tokens_at(path: &Path, tokens: &[String]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    std::fs::write(tmp.path(), serde_json::to_vec(tokens)?)?;
    tmp.persist(path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    Ok(())
}

/// 进程级缓存的密钥(首次使用时读取 / 创建一次)。
pub(crate) fn ensure_secret() -> [u8; 32] {
    static SECRET: OnceLock<[u8; 32]> = OnceLock::new();
    *SECRET.get_or_init(|| ensure_secret_at(&secret_path()))
}

/// 依据磁盘上的令牌存储 + 密钥进行授权。
pub(crate) fn authorize(kind: &str, checkpoint: &str, shell: &[String]) -> AuthDecision {
    let secret = ensure_secret();
    let tokens = load_tokens_at(&tokens_path());
    authorize_with(&secret, &tokens, kind, checkpoint, shell)
}

/// 为绑定铸造并持久化令牌(幂等)。GUI 在用户于
/// resume 绑定审批中选择「始终允许」时调用。
pub(crate) fn approve_token(kind: &str, checkpoint: &str, shell: &[String]) {
    let secret = ensure_secret();
    let mut tokens = load_tokens_at(&tokens_path());
    let t = compute_token(&secret, kind, checkpoint, shell);
    if !tokens.iter().any(|x| x == &t) {
        tokens.push(t);
        let _ = save_tokens_at(&tokens_path(), &tokens);
    }
}
