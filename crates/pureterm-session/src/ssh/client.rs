use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use russh::client::Handle;
use russh::{client, ChannelMsg, Disconnect};
use tokio::sync::{broadcast, Mutex};

use crate::base::Session;
use crate::ssh::auth::SshAuth as SshAuthType;

struct SshClientHandler;

impl client::Handler for SshClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        // TODO: Implement proper host key verification with known_hosts
        log::warn!("Accepting server key without verification (TODO: implement known_hosts)");
        Ok(true)
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

        let handler = SshClientHandler;
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
                anyhow::bail!("SSH agent authentication not yet implemented")
            }
            SshAuthType::PromptPassword => {
                anyhow::bail!("Interactive password prompt not yet implemented")
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
