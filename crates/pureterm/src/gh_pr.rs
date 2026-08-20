//! GAP-32:通过 `gh` CLI 获取 GitHub PR 状态。
//!
//! 刻意避免重 HTTP 依赖:调用 `gh`(复用用户已有的
//! `gh auth`),而非引入 `reqwest` / `octocrab`,因此应用内
//! 没有令牌管理,也没有新增网络依赖。若 `gh` 缺失、未认证,
//! 或仓库不在 GitHub 上,该功能退化为「无 PR」—— 绝不会成为
//! GUI 必须向上传播的错误。
//!
//! [`PrInfo`] / [`PrCheckRollup`] 被
//! [`crate::status_bar::git::GitStatus`](其 `pr` 字段)引用,
//! 使状态栏 PR 胶囊与 Git 悬浮面板共享同一事实来源。

use std::process::Stdio;

use tokio::process::Command;

use crate::iced_app::Message;

/// 关联 PR 的 CI 检查状态汇总。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrCheckRollup {
    pub success: u32,
    pub pending: u32,
    pub failure: u32,
    pub total: u32,
}

/// 从 `gh pr view --json number,state,headRefName,url,statusCheckRollup` 解析出的最小 PR 信息。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrInfo {
    pub number: u64,
    /// "OPEN" | "MERGED" | "CLOSED".
    pub state: String,
    pub head_ref: String,
    pub url: String,
    pub checks: PrCheckRollup,
}

/// 为当前分支的 PR 生成 `gh pr view`(GAP-32)。镜像
/// [`crate::status_bar::git::spawn_git_status`]:通过
/// `Task::perform` 在 GUI 线程外运行,映射到 [`Message::GhPrResult`]。优雅降级 ——
/// `gh` 缺失 / 未认证或分支无 PR 时,产出 `None` + 一条横幅字符串,
/// 而不报错。
pub fn spawn_gh_pr(cwd: String) -> iced::Task<Message> {
    iced::Task::perform(
        async move {
            // 1) 廉价探测:`gh` 本身能否调用?
            let probe = std::process::Command::new("gh")
                .arg("--version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if probe.is_err() {
                return (cwd, None, Some("gh not installed".to_string()));
            }
            // 2) `gh pr view` 解析当前分支的 PR(分支不存在、未认证
            //    或不在 GitHub 仓库内时以非零码退出)。
            let out = Command::new("gh")
                .args([
                    "pr", "view", "--json", "number,state,headRefName,url,statusCheckRollup",
                ])
                .current_dir(&cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .await;
            match out {
                Ok(o) if o.status.success() => match parse_gh_pr_json(
                    &String::from_utf8_lossy(&o.stdout),
                ) {
                    Some(pr) => (cwd, Some(pr), None),
                    None => (cwd, None, Some("could not parse gh output".to_string())),
                },
                Ok(_) => (
                    cwd,
                    None,
                    Some("no PR / not a GitHub repo / gh unauthed".to_string()),
                ),
                Err(_) => (cwd, None, Some("gh invocation failed".to_string())),
            }
        },
        |(cwd, pr, err)| Message::GhPrResult(cwd, pr, err),
    )
}

/// 为面板的「最近提交」区块生成 `git log -5`(GAP-32)。映射到
/// [`Message::GitPanelLogResult`]。仅当面板打开时触发。
pub fn spawn_git_log(cwd: String) -> iced::Task<Message> {
    iced::Task::perform(
        async move {
            // %h 短哈希、%x09 制表符、%s 主题 —— 每行一条提交。
            let out = Command::new("git")
                .args(["-C", &cwd, "log", "-5", "--format=%h%x09%s"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .await;
            let log: Vec<(String, String)> = match out {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|l| {
                        l.split_once('\t').map(|(h, s)| (h.to_string(), s.to_string()))
                    })
                    .collect(),
                _ => Vec::new(),
            };
            (cwd, log)
        },
        |(cwd, log)| Message::GitPanelLogResult(cwd, log),
    )
}

/// 把 `gh pr view --json …` 的 JSON 解析为 [`PrInfo`](纯函数,有单元测试)。
/// `statusCheckRollup` 防御式分桶:已完成的 `SUCCESS` / `NEUTRAL` / `SKIPPED`
/// 结论 ⇒ 成功;失败类结论 ⇒ 失败;结论缺失
/// (仍在运行 / 排队)⇒ 待定。缺失字段绝不报错。
pub fn parse_gh_pr_json(s: &str) -> Option<PrInfo> {
    let v: serde_json::Value = serde_json::from_str(s).ok()?;
    let number = v.get("number")?.as_u64()?;
    let state = v.get("state").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let head_ref = v
        .get("headRefName")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let url = v.get("url").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let mut rollup = PrCheckRollup::default();
    if let Some(checks) = v.get("statusCheckRollup").and_then(|x| x.as_array()) {
        for c in checks {
            rollup.total += 1;
            let conclusion = c.get("conclusion").and_then(|x| x.as_str()).unwrap_or("");
            let status = c.get("status").and_then(|x| x.as_str()).unwrap_or("");
            match conclusion {
                "SUCCESS" | "NEUTRAL" | "SKIPPED" => rollup.success += 1,
                "FAILURE" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "STALE" => {
                    rollup.failure += 1
                }
                "" => {
                    if status == "COMPLETED" {
                        rollup.success += 1;
                    } else {
                        rollup.pending += 1;
                    }
                }
                _ => rollup.pending += 1,
            }
        }
    }
    Some(PrInfo { number, state, head_ref, url, checks: rollup })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_open_pr_mixed_checks() {
        let json = r#"{
            "number": 42,
            "state": "OPEN",
            "headRefName": "feature/x",
            "url": "https://github.com/o/r/pull/42",
            "statusCheckRollup": [
                {"name":"a","status":"COMPLETED","conclusion":"SUCCESS"},
                {"name":"b","status":"IN_PROGRESS","conclusion":""},
                {"name":"c","status":"COMPLETED","conclusion":"FAILURE"}
            ]
        }"#;
        let pr = parse_gh_pr_json(json).expect("parsed");
        assert_eq!(pr.number, 42);
        assert_eq!(pr.state, "OPEN");
        assert_eq!(pr.head_ref, "feature/x");
        assert_eq!(pr.url, "https://github.com/o/r/pull/42");
        assert_eq!(pr.checks.total, 3);
        assert_eq!(pr.checks.success, 1);
        assert_eq!(pr.checks.pending, 1);
        assert_eq!(pr.checks.failure, 1);
    }

    #[test]
    fn parse_pr_without_checks() {
        let json = r#"{"number":7,"state":"OPEN","headRefName":"d","url":"u"}"#;
        let pr = parse_gh_pr_json(json).expect("parsed");
        assert_eq!(pr.checks.total, 0);
        assert_eq!((pr.checks.success, pr.checks.pending, pr.checks.failure), (0, 0, 0));
    }

    #[test]
    fn parse_empty_or_garbage_is_none() {
        assert_eq!(parse_gh_pr_json(""), None);
        assert_eq!(parse_gh_pr_json("not json"), None);
        // 缺少必需的 `number` ⇒ None。
        assert_eq!(parse_gh_pr_json(r#"{"state":"OPEN"}"#), None);
    }
}
