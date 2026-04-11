use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum SshAuth {
    PromptPassword,
    Password(String),
    PublicKey {
        key_path: PathBuf,
        passphrase: Option<String>,
    },
    Agent,
    None,
}
