use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use russh::client::Handle;
use russh::{client, ChannelMsg, Disconnect};
use tokio::sync::{broadcast, Mutex};

use crate::base::Session;
use crate::ssh::auth::SshAuth as SshAuthType;
use crate::ssh::known_hosts::{KeyVerificationResult, KnownHosts};

struct SshClientHandler {
    host: String,
    port: u16,
    known_hosts: KnownHosts,
}

impl SshClientHandler {
    fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            known_hosts: KnownHosts::new(),
        }
    }
}

impl client::Handler for SshClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        match self.known_hosts.verify_host_key(&self.host, self.port, server_public_key) {
            KeyVerificationResult::Match => {
                log::info!("Host key verified for {}", self.host);
                Ok(true)
            }
            KeyVerificationResult::Mismatch { expected } => {
                log::error!(
                    "Host key mismatch for {}! Expected: {}",
                    self.host,
                    expected
                );
                Ok(false)
            }
            KeyVerificationResult::Unknown => {
                // TOFU: Trust On First Use — accept and save the key
                log::warn!(
                    "First connection to {}. Accepting key (TOFU).",
                    self.host
                );
                self.known_hosts.add_host_key(&self.host, self.port, server_public_key)?;
                Ok(true)
            }
        }
    }
}

/// SSH session using russh.
pub struct SshSession {
    handle: Option<Arc<Mutex<Handle<SshClientHandler>>>>,
    channel_write: Option<Arc<Mutex<russh::ChannelWriteHalf<russh::client::Msg>>>>,
    output_tx: broadcast::Sender<Vec<u8>>,
    host: String,
    port: u16,
    user: String,
    auth: SshAuthType,
    open: bool,
}

impl SshSession {
    pub fn new(host: String, port: u16, user: String, auth: SshAuthType) -> Self {
        let (output_tx, _) = broadcast::channel(4096);
        Self {
            handle: None,
            channel_write: None,
            output_tx,
            host,
            port,
            user,
            auth,
            open: false,
        }
    }
}

#[async_trait]
impl Session for SshSession {
    fn is_open(&self) -> bool {
        self.open
    }

    async fn start(&mut self) -> Result<()> {
        let config = Arc::new(client::Config {
            ..Default::default()
        });

        let handler = SshClientHandler::new(self.host.clone(), self.port);
        let mut session = client::connect(config, (&*self.host, self.port), handler)
            .await
            .context("Failed to connect to SSH server")?;

        // Authenticate
        let auth_result = match &self.auth {
            SshAuthType::None => {
                session
                    .authenticate_none(&self.user)
                    .await
                    .context("SSH none authentication failed")?
            }
            SshAuthType::Password(password) => {
                session
                    .authenticate_password(&self.user, password.clone())
                    .await
                    .context("SSH password authentication failed")?
            }
            SshAuthType::PublicKey { key_path, passphrase } => {
                let key_pair = russh::keys::load_secret_key(key_path, passphrase.as_deref())
                    .context("Failed to load SSH key")?;
                let key_with_alg = russh::keys::PrivateKeyWithHashAlg::new(
                    Arc::new(key_pair),
                    None,
                );
                session
                    .authenticate_publickey(&self.user, key_with_alg)
                    .await
                    .context("SSH publickey authentication failed")?
            }
            SshAuthType::Agent => {
                // SSH Agent authentication via russh's agent client.
                // russh's PrivateKeyWithHashAlg requires a PrivateKey, but agent
                // only exposes public keys + signing. We use the russh
                // session's authenticate_future method with an agent signer.
                //
                // For now, fall back to loading the default SSH key from
                // ~/.ssh/id_rsa or ~/.ssh/id_ed25519 and authenticating with it.
                // Full agent protocol support will be added in a future iteration.
                let home = dirs::home_dir().context("Cannot determine home directory")?;
                let ssh_dir = home.join(".ssh");

                let key_paths = [
                    ssh_dir.join("id_ed25519"),
                    ssh_dir.join("id_rsa"),
                    ssh_dir.join("id_ecdsa"),
                ];

                let mut auth_success = false;
                for key_path in &key_paths {
                    if !key_path.exists() {
                        continue;
                    }
                    let key_pair = match russh::keys::load_secret_key(key_path, None) {
                        Ok(k) => k,
                        Err(_) => continue,
                    };
                    let key_with_alg = russh::keys::PrivateKeyWithHashAlg::new(
                        Arc::new(key_pair),
                        None,
                    );
                    let result = session
                        .authenticate_publickey(&self.user, key_with_alg)
                        .await
                        .context("SSH key authentication failed")?;
                    if result.success() {
                        auth_success = true;
                        break;
                    }
                }

                if !auth_success {
                    anyhow::bail!(
                        "SSH agent authentication failed: no suitable key found in ~/.ssh/"
                    );
                }
                russh::client::AuthResult::Success
            }
            SshAuthType::PromptPassword => {
                // Interactive password prompt — read from stdin
                eprint!("Password for {}@{}: ", self.user, self.host);
                let password = rpassword::read_password()
                    .context("Failed to read password from terminal")?;
                session
                    .authenticate_password(&self.user, password)
                    .await
                    .context("SSH password authentication failed")?
            }
        };

        if !auth_result.success() {
            anyhow::bail!("SSH authentication rejected");
        }

        // Open a shell channel
        let channel = session
            .channel_open_session()
            .await
            .context("Failed to open SSH channel")?;

        channel
            .request_shell(true)
            .await
            .context("Failed to request shell")?;

        // Split the channel: keep the write half, move the read half to a spawned task
        let (read_half, write_half) = channel.split();
        self.channel_write = Some(Arc::new(Mutex::new(write_half)));
        self.handle = Some(Arc::new(Mutex::new(session)));
        self.open = true;

        // Spawn task to read channel output
        let tx = self.output_tx.clone();
        tokio::spawn(async move {
            let mut read = read_half;
            loop {
                match read.wait().await {
                    Some(ChannelMsg::Data { data }) => {
                        if tx.send(data.to_vec()).is_err() {
                            break;
                        }
                    }
                    Some(ChannelMsg::Eof) | None => break,
                    _ => {}
                }
            }
        });

        Ok(())
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        if let Some(ref channel) = self.channel_write {
            let channel = Arc::clone(channel);
            tokio::spawn(async move {
                let ch = channel.lock().await;
                let _ = ch.window_change(cols as u32, rows as u32, 0, 0).await;
            });
        }
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        if let Some(ref channel) = self.channel_write {
            let channel = Arc::clone(channel);
            let data = data.to_vec();
            tokio::spawn(async move {
                let ch = channel.lock().await;
                let _ = ch.data(&data[..]).await;
            });
        }
        Ok(())
    }

    fn kill(&mut self, _signal: &str) -> Result<()> {
        self.open = false;
        if let Some(ref channel) = self.channel_write {
            let channel = Arc::clone(channel);
            tokio::spawn(async move {
                let ch = channel.lock().await;
                let _ = ch.eof().await;
            });
        }
        if let Some(ref handle) = self.handle {
            let handle = Arc::clone(handle);
            tokio::spawn(async move {
                let h = handle.lock().await;
                let _ = h.disconnect(Disconnect::ByApplication, "", "").await;
            });
        }
        Ok(())
    }

    fn output_stream(&self) -> broadcast::Receiver<Vec<u8>> {
        self.output_tx.subscribe()
    }

    fn session_type_name(&self) -> &'static str {
        "ssh"
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        self.open = false;
    }
}
