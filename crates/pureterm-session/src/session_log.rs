use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::SystemTime;

/// Configuration for session logging.
#[derive(Debug, Clone)]
pub struct SessionLogConfig {
    pub enabled: bool,
    /// Directory for log files. Defaults to `~/.config/pureterm/logs/`.
    pub log_dir: Option<PathBuf>,
    /// Whether to log user input as well.
    pub log_input: bool,
    /// Maximum log file size in bytes; rotates when exceeded.
    pub max_file_size: Option<u64>,
}

impl Default for SessionLogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            log_dir: None,
            log_input: false,
            max_file_size: None,
        }
    }
}

/// Session log writer that records terminal output (and optionally input) with timestamps.
#[allow(dead_code)]
pub struct SessionLog {
    file: Option<File>,
    log_dir: PathBuf,
    session_name: String,
    start_time: SystemTime,
    log_input: bool,
    file_size: u64,
    max_file_size: Option<u64>,
    file_index: u32,
}

impl SessionLog {
    /// Create a new session logger.
    ///
    /// If `config.enabled` is false, the logger is inert (all write calls are no-ops).
    /// The log file is created with a name based on `session_name` and the current timestamp.
    pub fn new(session_name: &str, config: &SessionLogConfig) -> Self {
        let log_dir = config
            .log_dir
            .clone()
            .unwrap_or_else(Self::default_log_dir);
        let file = if config.enabled {
            fs::create_dir_all(&log_dir).ok();
            let filename = Self::log_filename(session_name, 0);
            let path = log_dir.join(&filename);
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .ok()
        } else {
            None
        };

        Self {
            file,
            log_dir,
            session_name: session_name.to_string(),
            start_time: SystemTime::now(),
            log_input: config.log_input,
            file_size: 0,
            max_file_size: config.max_file_size,
            file_index: 0,
        }
    }

    /// Default log directory: `~/.config/pureterm/logs/`
    pub fn default_log_dir() -> PathBuf {
        dirs_home()
            .join(".config")
            .join("pureterm")
            .join("logs")
    }

    /// Generate a log filename.
    ///
    /// - index 0: `{name}_{YYYYMMDD_HHMMSS}.log`
    /// - index > 0: `{name}_{YYYYMMDD_HHMMSS}_{index}.log`
    pub fn log_filename(session_name: &str, index: u32) -> String {
        let now: chrono_like::DateTime = chrono_like::now_local();
        let ts = format!(
            "{:04}{:02}{:02}_{:02}{:02}{:02}",
            now.year, now.month, now.day,
            now.hour, now.minute, now.second,
        );
        if index == 0 {
            format!("{session_name}_{ts}.log")
        } else {
            format!("{session_name}_{ts}_{index}.log")
        }
    }

    /// Write terminal output with a timestamp prefix.
    ///
    /// Each chunk is prefixed with `[HH:MM:SS.mmm] `.
    /// Non-UTF-8 bytes are written as-is after the timestamp.
    pub fn log_output(&mut self, data: &[u8]) {
        if self.file.is_none() {
            return;
        }
        let prefix = format_timestamp_prefix();
        self.write_chunk(prefix.as_bytes(), data);
    }

    /// Write user input with a timestamp prefix.
    ///
    /// Each chunk is prefixed with `[HH:MM:SS.mmm] < ` (only if `log_input` is enabled).
    pub fn log_input(&mut self, data: &[u8]) {
        if !self.log_input || self.file.is_none() {
            return;
        }
        let prefix = format_timestamp_input_prefix();
        self.write_chunk(prefix.as_bytes(), data);
    }

    /// Flush buffered data to disk.
    pub fn flush(&mut self) {
        if let Some(ref mut f) = self.file {
            let _ = f.flush();
        }
    }

    /// Check file size and rotate if the max is exceeded.
    ///
    /// Closes the current file and opens a new one with an incremented index.
    pub fn rotate_if_needed(&mut self) {
        let max = match self.max_file_size {
            Some(m) => m,
            None => return,
        };
        if self.file_size < max {
            return;
        }

        self.file_index += 1;
        let filename = Self::log_filename(&self.session_name, self.file_index);
        let path = self.log_dir.join(&filename);

        if let Ok(new_file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            self.file = Some(new_file);
            self.file_size = 0;
        }
    }

    // -- internal helpers --

    fn write_chunk(&mut self, prefix: &[u8], data: &[u8]) {
        if let Some(ref mut f) = self.file {
            let mut written: u64 = 0;
            if let Ok(n) = f.write(prefix) {
                written += n as u64;
            }
            if let Ok(n) = f.write(data) {
                written += n as u64;
            }
            if let Ok(n) = f.write(b"\n") {
                written += n as u64;
            }
            self.file_size += written;
            self.rotate_if_needed();
        }
    }
}

impl Drop for SessionLog {
    fn drop(&mut self) {
        self.flush();
    }
}

// ---------------------------------------------------------------------------
// Minimal helpers to avoid pulling in a full chrono dependency.
// Uses std APIs so that only the standard library is required.
// ---------------------------------------------------------------------------

mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(PathBuf::from)
    }
}

fn dirs_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Lightweight timestamp struct to avoid a chrono dependency.
mod chrono_like {
    #[derive(Debug)]
    pub struct DateTime {
        pub year: i32,
        pub month: u32,
        pub day: u32,
        pub hour: u32,
        pub minute: u32,
        pub second: u32,
    }

    pub fn now_local() -> DateTime {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Simple algorithm; good enough for filenames.
        let (year, month, day, hour, minute, second) = unix_to_date(secs);
        DateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
        }
    }

    /// Convert Unix timestamp (seconds) to (year, month, day, hour, minute, second).
    fn unix_to_date(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
        let days_since_epoch = secs / 86400;
        let time_of_day = secs % 86400;
        let hour = (time_of_day / 3600) as u32;
        let minute = ((time_of_day % 3600) / 60) as u32;
        let second = (time_of_day % 60) as u32;

        // Convert days since epoch to year/month/day (copied from clock algorithms).
        let days = days_since_epoch as i64;

        // Shift to March-based year so that leap day is at end of year.
        let z = days + 719468;
        let era: i64;
        let doe: i64; // day of era
        if z >= 0 {
            era = z / 146097;
            doe = z % 146097;
        } else {
            era = (z - 146096) / 146097;
            doe = z - era * 146097;
        }
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year (March-based)
        let mp = (5 * doy + 2) / 153; // month index (0=March .. 11=February)
        let d = doy - (153 * mp + 2) / 5 + 1; // day of month
        let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month (1-12)
        let y = if m <= 2 { y + 1 } else { y };

        (y as i32, m as u32, d as u32, hour, minute, second)
    }
}

fn format_timestamp_prefix() -> String {
    let d = chrono_like::now_local();
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        % 1000;
    format!("[{:02}:{:02}:{:02}.{:03}] ", d.hour, d.minute, d.second, millis)
}

fn format_timestamp_input_prefix() -> String {
    let d = chrono_like::now_local();
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        % 1000;
    format!("[{:02}:{:02}:{:02}.{:03}] < ", d.hour, d.minute, d.second, millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default() {
        let cfg = SessionLogConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.log_dir.is_none());
        assert!(!cfg.log_input);
        assert!(cfg.max_file_size.is_none());
    }

    #[test]
    fn disabled_logger_is_noop() {
        let cfg = SessionLogConfig::default();
        let mut logger = SessionLog::new("test", &cfg);
        logger.log_output(b"hello");
        logger.log_input(b"world");
        logger.flush();
        assert!(logger.file.is_none());
    }

    #[test]
    fn enabled_logger_creates_file() {
        let dir = std::env::temp_dir().join("pureterm_test_log");
        let _ = fs::remove_dir_all(&dir);
        let cfg = SessionLogConfig {
            enabled: true,
            log_dir: Some(dir.clone()),
            log_input: true,
            max_file_size: None,
        };
        let mut logger = SessionLog::new("test_session", &cfg);
        assert!(logger.file.is_some());

        logger.log_output(b"output data");
        logger.log_input(b"input data");
        logger.flush();

        // Verify file exists and has content
        drop(logger);
        let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotation_works() {
        let dir = std::env::temp_dir().join("pureterm_test_rotate");
        let _ = fs::remove_dir_all(&dir);
        let cfg = SessionLogConfig {
            enabled: true,
            log_dir: Some(dir.clone()),
            log_input: false,
            max_file_size: Some(100), // very small to force rotation
        };
        let mut logger = SessionLog::new("rot", &cfg);

        // Write enough to exceed 100 bytes
        for i in 0..20 {
            logger.log_output(format!("line {i} with some padding data").as_bytes());
        }
        logger.flush();
        drop(logger);

        let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert!(entries.len() > 1, "expected multiple rotated files");
        let _ = fs::remove_dir_all(&dir);
    }
}
