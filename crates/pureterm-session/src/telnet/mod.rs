use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, Mutex};

use crate::base::Session;

/// Telnet session.
pub struct TelnetSession {
    host: String,
    port: u16,
    writer: Option<Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>>,
    output_tx: broadcast::Sender<Vec<u8>>,
    open: bool,
}

impl TelnetSession {
    pub fn new(host: String, port: u16) -> Self {
        let (output_tx, _) = broadcast::channel(4096);
        Self {
            host,
            port,
            writer: None,
            output_tx,
            open: false,
        }
    }
}

#[async_trait]
impl Session for TelnetSession {
    fn is_open(&self) -> bool {
        self.open
    }

    async fn start(&mut self) -> Result<()> {
        let stream = TcpStream::connect((&*self.host, self.port))
            .await
            .context("Failed to connect to Telnet server")?;

        let (read_half, write_half) = tokio::io::split(stream);

        self.writer = Some(Arc::new(Mutex::new(write_half)));
        self.open = true;

        // Spawn task to read from socket
        let tx = self.output_tx.clone();
        tokio::spawn(async move {
            let mut reader = read_half;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(())
    }

    fn resize(&mut self, _cols: u16, _rows: u16) -> Result<()> {
        // Telnet NAWS could be implemented here
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        if let Some(ref writer) = self.writer {
            let writer = Arc::clone(writer);
            let data = data.to_vec();
            tokio::spawn(async move {
                let mut w = writer.lock().await;
                let _ = w.write_all(&data).await;
            });
        }
        Ok(())
    }

    fn kill(&mut self, _signal: &str) -> Result<()> {
        self.open = false;
        Ok(())
    }

    fn output_stream(&self) -> broadcast::Receiver<Vec<u8>> {
        self.output_tx.subscribe()
    }

    fn session_type_name(&self) -> &'static str {
        "telnet"
    }
}

impl Drop for TelnetSession {
    fn drop(&mut self) {
        self.open = false;
    }
}
