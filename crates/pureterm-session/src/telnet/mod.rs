mod iac;

use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, Mutex};

use crate::base::Session;
use iac::IacNegotiator;

/// 支持 IAC 协议协商的 Telnet 会话。
pub struct TelnetSession {
    host: String,
    port: u16,
    writer: Option<Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>>,
    output_tx: broadcast::Sender<Vec<u8>>,
    iac: Arc<Mutex<IacNegotiator>>,
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
            iac: Arc::new(Mutex::new(IacNegotiator::new())),
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

        // 生成任务从套接字读取,通过 IAC 协商器处理数据,然后将干净的数据转发到输出通道。
        let tx = self.output_tx.clone();
        let iac = Arc::clone(&self.iac);
        let writer = Arc::clone(self.writer.as_ref().unwrap());

        tokio::spawn(async move {
            let mut reader = read_half;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let raw = &buf[..n];

                        // 通过 IAC 协商器处理。
                        let (clean_data, responses) = {
                            let mut guard = iac.lock().await;
                            guard.process_input(raw)
                        };

                        // 将任何 IAC 响应发送回服务器。
                        if !responses.is_empty() {
                            let mut w = writer.lock().await;
                            if w.write_all(&responses).await.is_err() {
                                break;
                            }
                        }

                        // 将干净的数据转发给输出订阅者。
                        if !clean_data.is_empty() {
                            if tx.send(clean_data).is_err() {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(())
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        // 如果协商了该选项,则尝试通过 IAC 发送 NAWS 更新。
        if let Some(ref writer) = self.writer {
            let iac = Arc::clone(&self.iac);
            let writer = Arc::clone(writer);
            // 我们需要一个异步上下文来使用 Mutex,所以生成一个任务。
            tokio::spawn(async move {
                let naws_data = {
                    let mut guard = iac.lock().await;
                    guard.set_window_size(cols, rows)
                };
                if !naws_data.is_empty() {
                    let mut w = writer.lock().await;
                    let _ = w.write_all(&naws_data).await;
                }
            });
        }
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
