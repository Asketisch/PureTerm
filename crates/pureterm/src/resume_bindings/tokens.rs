use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
/// HMAC-SHA256 type alias for token computation.
type HmacSha256 = Hmac<Sha256>;

// ── GAP-7.7 M2: HMAC-signed approval tokens ───────────────────────────────
//
// A resume binding runs arbitrary shell, so auto-execution is gated by a
// persisted HMAC-SHA256 token. The GUI mints a token (over the binding's
// canonical payload) when the user clicks Always in the Feed panel; the
// secret lives at `<data_dir>/resume.key` (0600), the approved digests at
// `<data_dir>/resume-tokens.json`. Threat model: drive-by / accidental config
// edits, not a determined local attacker (single-user workstation).

/// Authorization outcome for executing a resume binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthDecision {
    /// A valid HMAC token matches → run silently.
    Approved,
    /// No matching token → needs human approval via the Feed panel.
    NeedsPrompt,
}

/// Path of the long-lived HMAC secret: `<data_dir>/resume.key` (0600).
pub(crate) fn secret_path() -> PathBuf {
    pureterm_daemon::discovery::data_dir().join("resume.key")
}

/// Path of the approved-token digest list: `<data_dir>/resume-tokens.json`.
pub(crate) fn tokens_path() -> PathBuf {
    pureterm_daemon::discovery::data_dir().join("resume-tokens.json")
}

/// Canonical, stable byte encoding of a binding for HMAC. Uses `\0` separators
/// (cannot appear in a TOML string) so `kind`/`checkpoint`/`shell` boundaries
/// are unambiguous. Stable across runs — do NOT use JSON (key-order unstable).
pub(super) fn canonical_payload(kind: &str, checkpoint: &str, shell: &[String]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(kind.as_bytes());
    v.push(0);
    v.extend_from_slice(checkpoint.as_bytes());
    v.push(0);
    v.extend_from_slice(shell.join("\0").as_bytes());
    v
}

/// Hex-encoded HMAC-SHA256 of the binding's canonical payload under `secret`.
pub(crate) fn compute_token(secret: &[u8], kind: &str, checkpoint: &str, shell: &[String]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&canonical_payload(kind, checkpoint, shell));
    hex::encode(mac.finalize().into_bytes())
}

/// Check whether `tokens` contains the digest for this binding.
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

/// Read or lazily create the 32-byte secret at `path` (0600 on create).
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

/// Read the approved-token list; empty on any error.
pub(crate) fn load_tokens_at(path: &Path) -> Vec<String> {
    let Ok(s) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&s).unwrap_or_default()
}

/// Atomically persist the approved-token list.
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

/// Cached secret for the process (read/created once on first use).
pub(crate) fn ensure_secret() -> [u8; 32] {
    static SECRET: OnceLock<[u8; 32]> = OnceLock::new();
    *SECRET.get_or_init(|| ensure_secret_at(&secret_path()))
}

/// Authorize against the on-disk token store + secret.
pub(crate) fn authorize(kind: &str, checkpoint: &str, shell: &[String]) -> AuthDecision {
    let secret = ensure_secret();
    let tokens = load_tokens_at(&tokens_path());
    authorize_with(&secret, &tokens, kind, checkpoint, shell)
}

/// Mint + persist the token for this binding (idempotent). Called by the GUI
/// when the user picks Always on a resume-binding approval.
pub(crate) fn approve_token(kind: &str, checkpoint: &str, shell: &[String]) {
    let secret = ensure_secret();
    let mut tokens = load_tokens_at(&tokens_path());
    let t = compute_token(&secret, kind, checkpoint, shell);
    if !tokens.iter().any(|x| x == &t) {
        tokens.push(t);
        let _ = save_tokens_at(&tokens_path(), &tokens);
    }
}
