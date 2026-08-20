//! GAP-7.6:agent 进程检测。
//!
//! 枚举主机上运行的进程,与 `[[vault.agents]]` 里声明的 `detect`
//! 子串进行匹配。`pureterm agents detect` 用它发现运行在 *PureTerm*
//! *外部* 的 agent(例如在另一个终端里启动的 `claude`),
//! 以便用户在新的窗格中恢复它们。
//!
//! 平台模型与 `status_bar.rs` / `idle_probe.rs` 一致:macOS 使用
//! `ps`,Linux 读取 `/proc`。不引入 `sysinfo` 依赖——保持
//! 与既有代码一致的进程枚举;检测属于低频操作
//! (启动 + 手动 CLI),不是热路径。
//!
//! `detect` 是**子串**(不是 shell 命令,与 cmux 的风格不同),
//! 以避免注入与跨 shell 兼容性问题。

use crate::app_config::VaultAgentEntry;

/// 匹配了某个 `[[vault.agents]]` `detect` 子串的主机进程。
#[derive(Debug, Clone)]
pub struct DetectedAgent {
    /// 匹配到的 `[[vault.agents]]` kind。
    pub kind: String,
    pub pid: u32,
    /// 匹配进程的 `comm + " " + args`(用于展示 / 诊断)。
    pub cmdline: String,
}

/// 条目的有效 `detect` 子串:配置值,
/// 若缺省则为小写的 `kind`。
fn detect_substring(entry: &VaultAgentEntry) -> String {
    entry
        .detect
        .clone()
        .unwrap_or_else(|| entry.kind.to_lowercase())
}

/// 几乎从不 *是* agent、却常把 agent 名当参数的进程基名
/// (例如 `grep claude`)。排除它们以减少误报;
/// 同时覆盖 shell / 多路复用器以保证安全。
fn is_filter_process(comm_base: &str) -> bool {
    matches!(
        comm_base,
        "grep" | "egrep" | "fgrep"
            | "rg"
            | "ps"
            | "lsof"
            | "fish"
            | "bash"
            | "zsh"
            | "sh"
            | "tmux"
            | "screen"
    )
}

/// 枚举全部主机进程为 `(pid, comm, cmdline)`。
#[cfg(target_os = "macos")]
fn enumerate_processes() -> Vec<(u32, String, String)> {
    // 每个字段末尾的 `=` 用于抑制表头行。
    let output = match std::process::Command::new("ps")
        .args(["-axo", "pid=,comm=,args="])
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    parse_ps_table(&String::from_utf8_lossy(&output.stdout))
}

/// 枚举全部主机进程为 `(pid, comm, cmdline)`。
#[cfg(target_os = "linux")]
fn enumerate_processes() -> Vec<(u32, String, String)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        // `/proc/<pid>/cmdline` 以 NUL 分隔;用空格拼接 argv。
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline"))
            .ok()
            .map(|bytes| {
                String::from_utf8_lossy(&bytes)
                    .split('\0')
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        if !comm.is_empty() || !cmdline.is_empty() {
            out.push((pid, comm, cmdline));
        }
    }
    out
}

/// 非 macOS / 非 Linux 目标(如 Windows):检测不可用,返回
/// 空列表。冷恢复(GAP-7.5)在那里仍可用;只有
/// 全主机扫描为空操作。
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn enumerate_processes() -> Vec<(u32, String, String)> {
    Vec::new()
}

/// 解析 `ps -axo pid=,comm=,args=` 输出行:前导数字 = pid,
/// 下一个 token = comm(路径),其余 = args。解析时排除我们自己的 pid。
fn parse_ps_table(stdout: &str) -> Vec<(u32, String, String)> {
    let self_pid = std::process::id();
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (pid_str, rest) = match line.find(char::is_whitespace) {
            Some(i) => (&line[..i], line[i..].trim_start()),
            None => continue,
        };
        let Ok(pid) = pid_str.parse::<u32>() else { continue };
        if pid == self_pid {
            continue;
        }
        let (comm, args) = match rest.find(char::is_whitespace) {
            Some(i) => (rest[..i].trim().to_string(), rest[i..].trim().to_string()),
            None => (rest.trim().to_string(), String::new()),
        };
        out.push((pid, comm, args));
    }
    out
}

/// 大小写不敏感的子串匹配。空 needle 永不匹配。
fn matches_detect(haystack_lower: &str, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    !needle.is_empty() && haystack_lower.contains(&needle)
}

/// 扫描主机进程,返回匹配任意 `[[vault.agents]]` `detect` 子串的进程。
/// 每个进程归属于第一个匹配的 kind。过滤进程
/// (grep/rg/ps/shell/...)以及自身 pid 都会被排除,
/// 以减少误报(例如 `detect = "claude"` 匹配到 `grep claude`)。
pub fn scan(agents: &[VaultAgentEntry]) -> Vec<DetectedAgent> {
    if agents.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (pid, comm, args) in enumerate_processes() {
        let comm_base = comm.rsplit('/').next().unwrap_or(&comm);
        if is_filter_process(comm_base) {
            continue;
        }
        // 对 `comm` + `args` 做匹配(大小写不敏感)。
        let haystack_lower = format!("{comm} {args}").to_lowercase();
        for entry in agents {
            if matches_detect(&haystack_lower, &detect_substring(entry)) {
                out.push(DetectedAgent {
                    kind: entry.kind.clone(),
                    pid,
                    cmdline: format!("{comm} {args}").trim().to_string(),
                });
                break; // 第一个匹配的 kind 生效
            }
        }
    }
    out
}

/// 将检测到的 agent 与持久化的 `agent_sessions` 记录交叉引用,
/// 每个 kind 尽可能附加最近(`last_seen`)的 session_id。
///
/// 注入 `load` 使“kind 最新”启发式无需磁盘即可单测;公开的
/// `with_sessions` 传入 `agent_sessions::load`。
///
/// **局限**:检测到的主机进程不带 pid↔session_id 关联
/// (会话存储按 kind 而非 pid 建键),因此附加的 id 是
/// 该 kind 最近已知的会话——是 *恢复提示*,并非确凿的对应。
pub fn with_sessions_from(
    detected: Vec<DetectedAgent>,
    load: impl Fn(&str) -> Vec<crate::agent_sessions::AgentSessionRecord>,
) -> Vec<(DetectedAgent, Option<String>)> {
    let mut cache: std::collections::HashMap<String, Vec<crate::agent_sessions::AgentSessionRecord>> =
        std::collections::HashMap::new();
    detected
        .into_iter()
        .map(|d| {
            let recs = cache
                .entry(d.kind.clone())
                .or_insert_with(|| load(&d.kind));
            let sid = recs.iter().max_by_key(|r| r.last_seen).map(|r| r.session_id.clone());
            (d, sid)
        })
        .collect()
}

/// 将检测到的 agent 与磁盘上的 `agent_sessions` 存储交叉引用,
/// 每个 kind 尽可能附加最近(`last_seen`)的 session_id。
/// 局限见 [`with_sessions_from`](kind-LATEST 启发式)。
pub fn with_sessions(
    detected: Vec<DetectedAgent>,
) -> Vec<(DetectedAgent, Option<String>)> {
    with_sessions_from(detected, crate::agent_sessions::load)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_sessions::AgentSessionRecord;

    fn detected(kind: &str, pid: u32) -> DetectedAgent {
        DetectedAgent { kind: kind.into(), pid, cmdline: format!("{kind} --pid {pid}") }
    }

    fn rec(sid: &str, last_seen: u64) -> AgentSessionRecord {
        AgentSessionRecord {
            session_id: sid.into(),
            kind: "claude-code".into(),
            cwd: None,
            last_seen,
        }
    }

    #[test]
    fn with_sessions_attaches_kind_latest_session_id() {
        // 注入一个返回种子记录的加载器;断言“kind 最新”启发式
        // (取最大 last_seen)。文档中的局限:附加的 id 是该 kind
        // 最近已知的会话——是恢复提示,并非确凿的
        // pid↔session 对应(主机 pid 不带会话关联)。
        let detected_vec = vec![detected("claude-code", 1)];
        let out = with_sessions_from(detected_vec, |_| vec![rec("old", 1), rec("new", 2)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1.as_deref(), Some("new"));
    }

    #[test]
    fn with_sessions_no_records_returns_none_sid() {
        let detected_vec = vec![detected("codex", 1)];
        let out = with_sessions_from(detected_vec, |_| vec![]);
        assert_eq!(out.len(), 1);
        assert!(out[0].1.is_none());
    }

    fn entry(kind: &str, detect: Option<&str>) -> VaultAgentEntry {
        VaultAgentEntry {
            kind: kind.to_string(),
            detect: detect.map(String::from),
            resume_command: vec![],
            ..Default::default()
        }
    }

    #[test]
    fn detect_substring_uses_config_or_kind_lowercase() {
        assert_eq!(detect_substring(&entry("Claude-Code", None)), "claude-code");
        assert_eq!(detect_substring(&entry("x", Some("custom-needle"))), "custom-needle");
    }

    #[test]
    fn filter_processes_recognised() {
        assert!(is_filter_process("grep"));
        assert!(is_filter_process("rg"));
        assert!(is_filter_process("zsh"));
        assert!(!is_filter_process("claude"));
        assert!(!is_filter_process("node"));
    }

    #[test]
    fn matches_detect_case_insensitive_substring() {
        assert!(matches_detect("claude --resume", "Claude"));
        assert!(matches_detect("/usr/bin/claude-code foo", "claude-code"));
        assert!(!matches_detect("vim readme.md", "claude"));
        assert!(!matches_detect("anything", ""));
    }

    #[test]
    fn parse_ps_table_skips_self_and_headerless() {
        let stdout = "  123 /usr/local/bin/claude claude --resume abc\n  456 /bin/grep grep claude\n";
        let rows = parse_ps_table(stdout);
        // 非自身 pid 都被解析;过滤发生在 scan() 中,而非这里。
        assert!(rows.iter().any(|(p, _, _)| *p == 123));
        assert!(rows.iter().any(|(p, _, _)| *p == 456));
    }

    #[test]
    fn scan_empty_agents_returns_empty() {
        assert!(scan(&[]).is_empty());
    }

    /// 集成测试:启动一个已知的长生命周期进程,确认 `scan` 能找到
    /// 它。最多轮询约 2 秒(宿主 `ps`/`/proc` 快照可能滞后于启动)
    /// 并通过 Drop 守卫回收子进程,panic 也不会留下孤儿进程。
    #[test]
    fn scan_finds_spawned_sleep() {
        use std::time::{Duration, Instant};

        struct Reap(std::process::Child);
        impl Drop for Reap {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        // `sleep` 不是过滤进程,且存活时间足够长可被采样。
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        let _reap = Reap(child);

        let agents = vec![entry("sleep-agent", Some("sleep"))];
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut found = false;
        while Instant::now() < deadline {
            if scan(&agents)
                .iter()
                .any(|d| d.pid == pid && d.kind == "sleep-agent")
            {
                found = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        assert!(found, "spawned sleep (pid {pid}) not detected within 2s");
    }
}
