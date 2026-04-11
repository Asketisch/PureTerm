use anyhow::{Context, Result};
use async_trait::async_trait;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use tokio::sync::broadcast;

use crate::base::{ConnectParams, Session, SessionType};

/// Local shell session using PTY.
pub struct LocalSession {
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    output_tx: broadcast::Sender<Vec<u8>>,
    child_id: Option<u32>,
    open: bool,
}

impl LocalSession {
    pub fn new(params: ConnectParams) -> Result<Self> {
        let (output_tx, _) = broadcast::channel(4096);

        Ok(Self {
            master: None,
            writer: None,
            output_tx,
            child_id: None,
            open: false,
        })
    }

    pub fn child_id(&self) -> Option<u32> {
        self.child_id
    }
}

#[async_trait]
impl Session for LocalSession {
    fn is_open(&self) -> bool {
        self.open
    }

    async fn start(&mut self) -> Result<()> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to open PTY")?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

        let mut cmd = CommandBuilder::new(shell);
        cmd.cwd(std::env::var("HOME").unwrap_or_else(|_| "/".to_string()));

        let child = pair.slave
            .spawn_command(cmd)
            .context("Failed to spawn shell")?;

        self.child_id = child.process_id();
        self.writer = Some(pair.master.take_writer()?);
        self.master = Some(pair.master);
        self.open = true;

        // Spawn a thread to read from PTY
        let tx = self.output_tx.clone();
        let mut reader = self.master.as_ref().unwrap().try_clone_reader()?;
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        log::debug!("PTY read: {} bytes", n);
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        log::debug!("PTY read error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        if let Some(ref master) = self.master {
            master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })?;
        }
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        if let Some(ref mut writer) = self.writer {
            log::debug!("PTY write: {} bytes = {:02x?}", data.len(), data);
            writer.write_all(data)?;
            writer.flush()?;
        }
        Ok(())
    }

    fn kill(&mut self, signal: &str) -> Result<()> {
        // The PTY child will be killed when dropped
        self.open = false;
        Ok(())
    }

    fn output_stream(&self) -> broadcast::Receiver<Vec<u8>> {
        self.output_tx.subscribe()
    }

    fn session_type_name(&self) -> &'static str {
        "local"
    }
}

impl Drop for LocalSession {
    fn drop(&mut self) {
        self.open = false;
    }
}
