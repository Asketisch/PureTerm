//! Prevent system sleep while commands are running.
//!
//! Uses the macOS `caffeinate` command to prevent idle sleep when a
//! command is detected as running (OSC 133 CommandStart) and allows
//! sleep again when output ends (OSC 133 OutputEnd).

/// Manages a `caffeinate` child process to prevent system sleep.
pub struct PreventSleep {
    child: Option<std::process::Child>,
}

impl PreventSleep {
    /// Create a new PreventSleep instance (initially inactive).
    pub fn new() -> Self {
        Self { child: None }
    }

    /// Prevent the system from sleeping.
    /// Spawns `caffeinate -i` on macOS to prevent idle sleep.
    /// Does nothing if already preventing sleep.
    pub fn prevent(&mut self) {
        if self.child.is_some() {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            if let Ok(child) = std::process::Command::new("caffeinate")
                .arg("-i") // prevent idle sleep
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                log::info!("PreventSleep: caffeinate started (pid {})", child.id());
                self.child = Some(child);
            } else {
                log::warn!("PreventSleep: failed to start caffeinate");
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            // On non-macOS platforms, systemd-inhibit or similar could be used.
            // For now, this is a no-op.
            log::trace!("PreventSleep: not implemented for this platform");
        }
    }

    /// Allow the system to sleep again.
    /// Kills the `caffeinate` child process if running.
    pub fn allow(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
            log::info!("PreventSleep: caffeinate stopped");
        }
    }

    /// Check if currently preventing sleep.
    pub fn is_preventing(&self) -> bool {
        self.child.is_some()
    }
}

impl Drop for PreventSleep {
    fn drop(&mut self) {
        self.allow();
    }
}

impl Default for PreventSleep {
    fn default() -> Self {
        Self::new()
    }
}
