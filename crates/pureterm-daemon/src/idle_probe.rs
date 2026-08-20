//! GAP-7.3:供休眠空闲检测用的子进程 CPU 采样。
//!
//! `sample_cpu_times(pid)` 返回子进程的累计(utime + stime)CPU
//! 时间。`SessionManager::sample_cpu_pass` 比较相邻两次采样:若
//! 累计时间在采样间隔内不再前进,子进程即为
//! CPU 空闲。结合 PTY 输出空闲(见 `session::run_pump`),构成
//! 休眠的双重空闲条件。
//!
//! macOS 读 `ps -o time= -p <pid>`;Linux 读 `/proc/<pid>/stat` 的
//! 第 14/15 字段。非 Unix 返回 `None`(休眠仅限 unix)。只有采样间的
//! *增量*有意义,因此单位(clock tick 还是厘秒)
//! 无关紧要,只要进程生命周期内保持一致。

/// 子进程累计 CPU 时间(用户 + 系统)的采样值。单位
/// 因平台而异(Linux jiffies / macOS 厘秒);只有增量有意义。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SampledCpuTimes {
    pub utime: u64,
    pub stime: u64,
}

impl SampledCpuTimes {
    /// 累计 CPU 总量(utime + stime)。
    pub fn total(self) -> u64 {
        self.utime.saturating_add(self.stime)
    }
}

/// 采样子进程的累计 CPU 时间。进程已消失或不可读时返回
/// `None`(死进程的 CPU 时间不会前进,因此调用方把
/// `None` 视作“没有新活动”——即不重置空闲时钟)。
#[cfg(target_os = "macos")]
pub fn sample_cpu_times(pid: u32) -> Option<SampledCpuTimes> {
    // `ps -o time= -p <pid>` → "[[dd-]hh:]mm:ss[.cc]"。把各部分加总成
    // 单一总量;单位无关紧要,因为只比较增量。
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "time="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let secs = parse_macos_time(s.trim());
    // macOS 的 `ps -o time=` 把 user+sys 折算成一个累计值。
    Some(SampledCpuTimes { utime: secs, stime: 0 })
}

/// 从 `/proc/<pid>/stat` 采样子进程的累计 CPU 时间。
#[cfg(target_os = "linux")]
pub fn sample_cpu_times(pid: u32) -> Option<SampledCpuTimes> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // comm(第 2 字段)在括号内且可能含空格;取最后一个 ')'
    // 之后的所有内容。其余字段从第 3 字段(state)起 1 基编号,
    // 因此 fields[k] 对应字段 (k+3)。utime 是第 14 字段 → fields[11]。
    let after_comm = stat.rsplit_once(')').map(|(_, rest)| rest)?;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    let utime = fields.get(11).and_then(|f| f.parse::<u64>().ok())?;
    let stime = fields.get(12).and_then(|f| f.parse::<u64>().ok())?;
    Some(SampledCpuTimes { utime, stime })
}

/// 非 Unix:不支持休眠;不做 CPU 采样。
#[cfg(not(unix))]
pub fn sample_cpu_times(_pid: u32) -> Option<SampledCpuTimes> {
    None
}

/// 把 macOS `ps -o time=` 的输出(`mm:ss.cc` / `hh:mm:ss` / `dd-hh:mm:ss`)
/// 解析成单一累计整数(单位任意;只用增量)。
#[cfg(any(target_os = "macos", test))]
fn parse_macos_time(s: &str) -> u64 {
    let mut total = 0u64;
    for part in s.split(':') {
        // 丢弃小数厘秒("ss.cc" → "ss");同时处理
        // `dd-hh` 形式,取 '-' 之后的部分。
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
        // 沉睡的子进程几乎不耗 CPU;采样仍应成功
        //(进程存在),尽管总量约为 0。
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
        // 收殓一个短命子进程,再采样其(已回收的)pid——不得
        // panic。结果是尽力而为(None 或其它进程)。
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
