use anyhow::{Context, Result};
use async_trait::async_trait;
use serialport::{self, SerialPort};
use std::io::{Read, Write};
use std::time::Duration;
use tokio::sync::broadcast;

use crate::base::Session;

/// Serial port session.
pub struct SerialSession {
    port_name: String,
    baud_rate: u32,
    writer: Option<Box<dyn SerialPort>>,
    output_tx: broadcast::Sender<Vec<u8>>,
    open: bool,
}

impl SerialSession {
    pub fn new(port_name: String, baud_rate: u32) -> Self {
        let (output_tx, _) = broadcast::channel(4096);
        Self {
            port_name,
            baud_rate,
            writer: None,
            output_tx,
            open: false,
        }
    }

    /// List available serial ports.
    pub fn list_ports() -> Vec<serialport::SerialPortInfo> {
        serialport::available_ports().unwrap_or_default()
    }
}

#[async_trait]
impl Session for SerialSession {
    fn is_open(&self) -> bool {
        self.open
    }

    async fn start(&mut self) -> Result<()> {
        let port = serialport::new(&self.port_name, self.baud_rate)
            .timeout(Duration::from_millis(100))
            .open()
            .context("Failed to open serial port")?;

        let mut reader = port.try_clone()
            .context("Failed to clone serial port")?;
        self.writer = Some(port);
        self.open = true;

        // Spawn thread to read from serial port
        let tx = self.output_tx.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                        continue;
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(())
    }

    fn resize(&mut self, _cols: u16, _rows: u16) -> Result<()> {
        // Serial ports don't have a concept of terminal size
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        if let Some(ref mut writer) = self.writer {
            writer.write_all(data)?;
            writer.flush()?;
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
        "serial"
    }
}

impl Drop for SerialSession {
    fn drop(&mut self) {
        self.open = false;
    }
}
