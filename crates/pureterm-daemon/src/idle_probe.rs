//! GAP-7.3: child-process CPU sampling for hibernation idle detection.
//!
//! `sample_cpu_times(pid)` returns the cumulative (utime + stime) CPU time of a
//! child. `SessionManager::sample_cpu_pass` compares consecutive samples: if the
//! cumulative time stops advancing across the sample interval, the child is
//! CPU-idle. Combined with PTY-output idle (see `session::run_pump`), this is
//! the dual idle condition for hibernation.
//!
//! macOS reads `ps -o time= -p <pid>`; Linux reads `/proc/<pid>/stat` fields
//! 14/15. Non-Unix returns `None` (hibernation is unix-only). Only the *delta*
//! between samples is meaningful, so the unit (clock ticks vs centiseconds)
//! does not matter as long as it's consistent within a process lifetime.

/// Sampled cumulative CPU time of a child (user + system). Unit is
/// platform-specific (Linux jiffies / macOS centiseconds); only deltas matter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SampledCpuTimes {
    pub utime: u64,
    pub stime: u64,
}

impl SampledCpuTimes {
    /// Total cumulative CPU (utime + stime).
    pub fn total(self) -> u64 {
        self.utime.saturating_add(self.stime)
    }
}

/// Sample the child's cumulative CPU time. `None` if the process is gone or
/// unreadable (a dead process won't advance CPU time, so the caller treats
/// `None` as "not newly active" — i.e. it does NOT reset the idle clock).
#[cfg(target_os = "macos")]
pub fn sample_cpu_times(pid: u32) -> Option<SampledCpuTimes> {
    // `ps -o time= -p <pid>` → "[[dd-]hh:]mm:ss[.cc]". Sum the parts into a
    // single total; unit is irrelevant since only deltas are compared.
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "time="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let secs = parse_macos_time(s.trim());
    // macOS `ps -o time=` folds user+sys into one cumulative figure.
    Some(SampledCpuTimes { utime: secs, stime: 0 })
}

/// Sample the child's cumulative CPU time from `/proc/<pid>/stat`.
#[cfg(target_os = "linux")]
pub fn sample_cpu_times(pid: u32) -> Option<SampledCpuTimes> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // comm (field 2) is in parens and may contain spaces; take everything after
    // the last ')'. The remaining fields are 1-indexed from field 3 (state),
    // so fields[k] corresponds to field (k+3). utime is field 14 → fields[11].
    let after_comm = stat.rsplit_once(')').map(|(_, rest)| rest)?;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    let utime = fields.get(11).and_then(|f| f.parse::<u64>().ok())?;
    let stime = fields.get(12).and_then(|f| f.parse::<u64>().ok())?;
    Some(SampledCpuTimes { utime, stime })
}

/// Non-Unix: hibernation is unsupported; no CPU sampling.
#[cfg(not(unix))]
pub fn sample_cpu_times(_pid: u32) -> Option<SampledCpuTimes> {
    None
}

/// Parse macOS `ps -o time=` output (`mm:ss.cc` / `hh:mm:ss` / `dd-hh:mm:ss`)
/// into a single cumulative integer (unit arbitrary; deltas only).
#[cfg(any(target_os = "macos", test))]
fn parse_macos_time(s: &str) -> u64 {
    let mut total = 0u64;
    for part in s.split(':') {
        // Drop any fractional centiseconds ("ss.cc" → "ss"); also handle the
        // `dd-hh` form by taking the part after '-'.
        let core = part.rsplit('-').next().unwrap_or("");
        let int_part = core.split('.').next().unwrap_or("");
        let n: u64 = int_part.trim().parse().unwrap_or(0);
        total = total.saturating_mul(60).saturating_add(n);
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampled_total_sums() {
        let t = SampledCpuTimes { utime: 5, stime: 7 };
        assert_eq!(t.total(), 12);
    }

    #[test]
    fn parse_macos_time_simple() {
        assert_eq!(parse_macos_time("0:00.03"), 0);
        assert_eq!(parse_macos_time("1:23"), 83);
        assert_eq!(parse_macos_time("1:02:03"), 3723);
    }

    #[cfg(unix)]
    #[test]
    fn sample_cpu_times_for_live_process_succeeds() {
        // A sleeping child consumes ~0 CPU; sampling should still succeed
        // (process exists) even though the total is ~0.
        let mut child =
            std::process::Command::new("sleep").arg("30").spawn().expect("spawn sleep");
        let pid = child.id();
        let sample = sample_cpu_times(pid);
        let _ = child.kill();
        let _ = child.wait();
        assert!(sample.is_some(), "sampling a live process should succeed");
    }

    #[cfg(unix)]
    #[test]
    fn sample_cpu_times_does_not_panic_on_dead_pid() {
        // Reap a short-lived child, then sample its (now-recycled) pid — must
        // not panic. The result is best-effort (None or some other process).
        let pid = {
            let mut child =
                std::process::Command::new("true").spawn().expect("spawn true");
            let id = child.id();
            let _ = child.wait();
            id
        };
        let _ = sample_cpu_times(pid);
    }
}
