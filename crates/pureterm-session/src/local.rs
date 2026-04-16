use anyhow::{Context, Result};
use async_trait::async_trait;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use tokio::sync::broadcast;

use crate::base::{ConnectParams, Session};
use crate::shell_integration::{self, Shell};

/// Local shell session using PTY.
pub struct LocalSession {
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    output_tx: broadcast::Sender<Vec<u8>>,
    child_id: Option<u32>,
    open: bool,
}

impl LocalSession {
    pub fn new(_params: ConnectParams) -> Result<Self> {
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

        let shell_path = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let shell = Shell::detect_from_path(&shell_path);

        let mut cmd = CommandBuilder::new(shell_path);

        // Set working directory
        if let Ok(cwd) = std::env::var("HOME") {
            cmd.cwd(cwd);
        }

        // Build child environment: start from inherited env, then ensure
        // critical variables are set. When launched from Finder (double-click),
        // the inherited env is minimal — LANG, PATH, etc. may be missing.
        let mut env: HashMap<String, String> = std::env::vars().collect();
        env.insert("TERM".to_string(), "xterm-256color".to_string());
        env.insert("COLORTERM".to_string(), "truecolor".to_string());

        // Ensure UTF-8 locale — required for correct Unicode rendering
        if !env.contains_key("LANG") {
            env.insert("LANG".to_string(), "en_US.UTF-8".to_string());
        }
        for lc_var in ["LC_ALL", "LC_CTYPE"] {
            if !env.contains_key(lc_var) {
                env.insert(lc_var.to_string(), "en_US.UTF-8".to_string());
            }
        }

        // Ensure PATH includes common tool directories (Homebrew, local bins)
        if let Some(path) = env.get("PATH") {
            if !path.contains("/usr/local/bin") {
                let mut extended = path.clone();
                for extra in ["/usr/local/bin", "/opt/homebrew/bin", "/opt/homebrew/sbin"] {
                    if std::path::Path::new(extra).is_dir() && !extended.contains(extra) {
                        extended = format!("{}:{}", extra, extended);
                    }
                }
                env.insert("PATH".to_string(), extended);
            }
        }

        // Apply shell integration if supported
        shell_integration::apply_env(shell, &mut env);

        // Set the environment for the command
        for (key, value) in &env {
            cmd.env(key, value);
        }

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

    fn kill(&mut self, _signal: &str) -> Result<()> {
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
