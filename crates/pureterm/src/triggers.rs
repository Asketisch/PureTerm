//! GAP-10：模式匹配触发器。
//!
//! 一条 `[[triggers]]` 规则用正则匹配终端的*输出*文本，匹配后触发一个动作
//!（高亮 / 通知 / 发送文本 / 执行命令等）。本模块包含纯逻辑、可单元测试的核心——
//! 配置类型、行扫描器、模板展开、每 pane 扫描窗口状态机、高亮/折叠区域的
//! 生命周期——刻意与终端 `Term` 解耦。

use std::collections::HashMap;

use regex::Regex;

/// 每次扫描每行记录匹配数的上限，用于约束病态正则。
pub const MAX_MATCHES_PER_LINE: usize = 8;
/// 单次 Wakeup 中扫描行数的上限；剩余部分在后续 Wakeup 中补齐
///（输入未处理完时，终端会持续发出 Wakeup）。
pub const MAX_LINES_PER_WAKE: usize = 200;
/// 每个 pane 保留高亮区域的上限（保留最新，淘汰最旧）。
pub const MAX_HIGHLIGHTS: usize = 256;
/// 每个 pane 保留折叠区域的上限。
pub const MAX_FOLDS: usize = 64;
/// 截断事件 / MCP 中携带的匹配文本，保持负载有界。
pub const MATCHED_TEXT_MAX: usize = 200;

/// 默认高亮浮层颜色（半透明绿色），与 GAP-4.1 搜索匹配的琥珀色
///（`[0.85, 0.65, 0.15, 0.35]`）以及选区蓝色相区分。
pub const DEFAULT_HIGHLIGHT_COLOR: [f32; 4] = [0.45, 0.80, 0.45, 0.32];

/// `exec` 触发器子进程的墙钟时间硬上限。否则一条挂死的命令（例如对死主机
/// 执行 `curl`）会永久泄漏进程，其 `Task` 永远无法完成。超时后子进程被杀死
///（见 [`spawn_trigger_exec`] 中的 `kill_on_drop`）并弹出通知。
pub const EXEC_TIMEOUT_SECS: u64 = 60;

/// 一条已配置的触发器规则（加载完成后；正则单独在 [`CompiledTrigger`] 中编译）。
#[derive(Clone, Debug, PartialEq)]
pub struct TriggerConfig {
    pub name: String,
    /// 原始模式文本，保留用于事件 / MCP 的 `trigger.list`。
    pub pattern: String,
    pub action: TriggerAction,
}

/// 触发器匹配时执行的动作。
#[derive(Clone, Debug, PartialEq)]
pub enum TriggerAction {
    /// 用 `color` 覆盖匹配区间（`None` 时默认绿色）。
    Highlight { color: Option<[f32; 4]> },
    /// 弹出通知（并且无论何种情形都发出 `trigger.matched` 事件）。
    Notify { title: Option<String> },
    /// 向某个 pane 发送 `text`。`pane_id = None` 表示产生匹配的那个 pane
    ///（输出驱动，例如 sudo 密码回复）；显式 id 则指向其他 pane。
    /// `text` 可以包含捕获组 / 变量模板。
    SendText { text: String, pane_id: Option<u64> },
    /// 以 `args` 参数启动 `command`（argv 直接启动，绝不经过 shell——安全说明见
    /// [`spawn_trigger_exec`]）。输出按 `route` 分发。
    Exec {
        command: String,
        args: Vec<String>,
        route: ExecRoute,
    },
    /// 将展开后的 `value` 赋给某个 pane 的用户变量（供 `${var:}` 使用）。
    SetVariable { name: String, value: String },
    /// 将匹配行记录为命名标记（供后续 `fold` 使用）。
    SetMark { mark: String },
    /// 折叠从命名标记（或 `lines_back` 行）到匹配行的输出行。`collapsed` 时在
    /// 渲染器中整行隐藏。
    Fold {
        mark: Option<String>,
        lines_back: Option<u32>,
    },
}

impl TriggerAction {
    /// 供事件 / MCP `trigger.list` 使用的小写判别值（不含任何秘密——
    /// `text`/`args`/`value` 有意省略）。
    pub fn discriminant(&self) -> &'static str {
        match self {
            TriggerAction::Highlight { .. } => "highlight",
            TriggerAction::Notify { .. } => "notify",
            TriggerAction::SendText { .. } => "send_text",
            TriggerAction::Exec { .. } => "exec",
            TriggerAction::SetVariable { .. } => "set_variable",
            TriggerAction::SetMark { .. } => "set_mark",
            TriggerAction::Fold { .. } => "fold",
        }
    }
}

/// GAP-10 安全：从远程（SSH）pane 触发的匹配，其动作是否应被拦截？
///
/// 远程输出可能受攻击者影响：捕获组（`$1`）可能把任意文本带进本地命令的
/// argv（`exec`）或另一个 pane 的 PTY（跨 pane `send_text`），构成本地代码
/// 执行 / 按键注入的向量。argv 不经过 shell 只能防住 shell 元字符，防不住
/// 这种注入。默认两种情况都被拦截，除非用户通过顶层配置
/// `allow_exec_on_remote = true` 显式开启。
///
/// 同 pane 的 `send_text`（`pane_id = None`）有意不拦截：它对应规范中的
/// `reply-sudo` 场景（用用户自己配置的文本自动回复用户自己的远程会话）。
/// `highlight` / `notify` / `set_variable` / `set_mark` / `fold` 对本地机器
/// 无副作用，从不拦截。
pub fn should_block_remote_action(
    action: &TriggerAction,
    is_ssh: bool,
    allow_exec_on_remote: bool,
) -> bool {
    if !is_ssh || allow_exec_on_remote {
        return false;
    }
    matches!(
        action,
        TriggerAction::Exec { .. } | TriggerAction::SendText { pane_id: Some(_), .. }
    )
}

/// `exec` 触发器的 stdout 去向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecRoute {
    /// 将 stdout（首行）以通知形式展示。
    Toast,
    /// 将 stdout 写入当前活动 pane 的 PTY。
    Pane,
}

impl Default for ExecRoute {
    fn default() -> Self {
        ExecRoute::Toast
    }
}

/// 一条触发器，其正则已在配置加载时一次性编译完成。
#[derive(Clone, Debug)]
pub struct CompiledTrigger {
    pub cfg: TriggerConfig,
    pub re: Regex,
}

/// [`scan_lines`] 找到的一个匹配。`line` 是原始的网格 `Line(i32)`；列跨度以
/// *字符*计（宽字符的注意事项见 [`scan_lines`]）。
#[derive(Clone, Debug, PartialEq)]
pub struct TriggerMatch {
    pub trigger_idx: usize,
    pub line: i32,
    pub col_start: usize,
    /// 结束列（不含）。
    pub col_end: usize,
    pub matched_text: String,
    /// 正则捕获组 1..n（组 0 = 完整匹配 = `matched_text`）。
    pub captures: Vec<String>,
}

/// 一段高亮单元格区间，存储于 pane 上并以浮层渲染。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HighlightRegion {
    /// 原始网格 `Line(i32)`；随滚动跟踪内容。
    pub line: i32,
    pub col_start: usize,
    pub col_end: usize,
    /// 预先解析好的 RGBA（触发器的颜色或 [`DEFAULT_HIGHLIGHT_COLOR`]）。
    pub color: [f32; 4],
}

/// 一段折叠的行范围——内部行在渲染器中被隐藏。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoldRegion {
    pub top_line: i32,
    pub bottom_line: i32,
    pub collapsed: bool,
}

/// [`expand_trigger_template`] 的输入。
pub struct TemplateContext<'a> {
    pub matched_text: &'a str,
    pub captures: &'a [String],
    pub variables: &'a HashMap<String, String>,
}

/// 展开主流终端风格的触发器参数模板。
///
/// 支持的替换：
/// - `{match}`、`$0`、`\0`——完整匹配
/// - `$1`..`$9`、`{1}`..`{9}`——捕获组（缺失 → 空串）
/// - `${var:NAME}`、`{var:NAME}`——每 pane 用户变量
/// - `\n`、`\r`、`\t`、`\xNN`——字面控制 / 十六进制字节
pub fn expand_trigger_template(template: &str, ctx: &TemplateContext<'_>) -> String {
    let mut out = String::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if template[i..].starts_with("{match}") {
            out.push_str(ctx.matched_text);
            i += 7;
            continue;
        }
        if template[i..].starts_with("{var:") {
            if let Some(end) = template[i + 5..].find('}') {
                let name = &template[i + 5..i + 5 + end];
                if let Some(v) = ctx.variables.get(name) {
                    out.push_str(v);
                }
                i += 5 + end + 1;
                continue;
            }
        }
        if i + 2 < bytes.len() && bytes[i] == b'{' && bytes[i + 2] == b'}' && bytes[i + 1].is_ascii_digit()
        {
            let d = bytes[i + 1] - b'0';
            if d >= 1 && d <= 9 {
                if let Some(c) = ctx.captures.get(d as usize - 1) {
                    out.push_str(c);
                }
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            if template[i..].starts_with("${var:") {
                if let Some(end) = template[i + 6..].find('}') {
                    let name = &template[i + 6..i + 6 + end];
                    if let Some(v) = ctx.variables.get(name) {
                        out.push_str(v);
                    }
                    i += 6 + end + 1;
                    continue;
                }
            }
            let next = bytes[i + 1];
            if next.is_ascii_digit() {
                let d = next - b'0';
                if d == 0 {
                    out.push_str(ctx.matched_text);
                } else if d <= 9 {
                    if let Some(c) = ctx.captures.get(d as usize - 1) {
                        out.push_str(c);
                    }
                }
                i += 2;
                continue;
            }
        }
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'0' => {
                    out.push_str(ctx.matched_text);
                    i += 2;
                    continue;
                }
                b'1'..=b'9' => {
                    let d = bytes[i + 1] - b'0';
                    if let Some(c) = ctx.captures.get(d as usize - 1) {
                        out.push_str(c);
                    }
                    i += 2;
                    continue;
                }
                b'n' => {
                    out.push('\n');
                    i += 2;
                    continue;
                }
                b'r' => {
                    out.push('\r');
                    i += 2;
                    continue;
                }
                b't' => {
                    out.push('\t');
                    i += 2;
                    continue;
                }
                b'x' if i + 3 < bytes.len() => {
                    let hi = hex_digit(bytes[i + 2]);
                    let lo = hex_digit(bytes[i + 3]);
                    if let (Some(h), Some(l)) = (hi, lo) {
                        out.push(char::from((h << 4) | l));
                        i += 4;
                        continue;
                    }
                }
                _ => {}
            }
        }
        out.push(char::from(bytes[i]));
        i += 1;
    }
    out
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// 每 pane 的触发器状态，在应用中以 `PaneId` 为键（与 `pane_shell_state` 对应）。
#[derive(Clone, Debug, Default)]
pub struct TriggerPaneState {
    pub last_scanned_line: Option<i32>,
    pub highlights: Vec<HighlightRegion>,
    pub folds: Vec<FoldRegion>,
    pub variables: HashMap<String, String>,
    pub named_marks: HashMap<String, i32>,
}

impl TriggerPaneState {
    pub fn push_highlight(&mut self, region: HighlightRegion) {
        self.highlights.push(region);
        if self.highlights.len() > MAX_HIGHLIGHTS {
            let drop_n = self.highlights.len() - MAX_HIGHLIGHTS;
            self.highlights.drain(0..drop_n);
        }
    }

    pub fn push_fold(&mut self, region: FoldRegion) {
        self.folds.push(region);
        if self.folds.len() > MAX_FOLDS {
            let drop_n = self.folds.len() - MAX_FOLDS;
            self.folds.drain(0..drop_n);
        }
    }

    pub fn evict_offscreen(&mut self, topmost_line: i32) {
        self.highlights.retain(|r| r.line >= topmost_line);
        self.folds.retain(|r| r.bottom_line >= topmost_line);
        self.named_marks.retain(|_, line| *line >= topmost_line);
    }
}

/// 为 `fold` 动作计算包含边界的 `[top_line, bottom_line]`。
pub fn fold_window(
    match_line: i32,
    mark_name: Option<&str>,
    lines_back: Option<u32>,
    named_marks: &HashMap<String, i32>,
    topmost_line: i32,
) -> (i32, i32) {
    let bottom = match_line;
    let top = if let Some(name) = mark_name {
        if let Some(&mark_line) = named_marks.get(name) {
            if mark_line >= topmost_line {
                mark_line
            } else if let Some(back) = lines_back {
                match_line - back as i32
            } else {
                match_line
            }
        } else if let Some(back) = lines_back {
            match_line - back as i32
        } else {
            match_line
        }
    } else if let Some(back) = lines_back {
        match_line - back as i32
    } else {
        match_line
    };
    (top.min(bottom), bottom)
}

pub fn next_scan_window(prev: Option<i32>, cursor: i32, max: usize) -> Option<(i32, i32)> {
    let prev = match prev {
        None => return None,
        Some(p) => p,
    };
    let start = prev + 1;
    if start > cursor {
        return None;
    }
    let span = (max.max(1) as i32).max(1);
    let end = (start + span - 1).min(cursor);
    debug_assert!(start <= end, "scan window must be non-empty");
    Some((start, end))
}

pub fn scan_lines(
    lines: &[(i32, &str)],
    triggers: &[CompiledTrigger],
    max_per_line: usize,
) -> Vec<TriggerMatch> {
    let cap = max_per_line.max(1);
    let mut out = Vec::new();
    for &(line, text) in lines {
        let mut count = 0;
        'triggers: for (tidx, trig) in triggers.iter().enumerate() {
            for caps in trig.re.captures_iter(text) {
                if count >= cap {
                    break 'triggers;
                }
                let m = caps.get(0).expect("captures_iter always has group 0");
                let col_start = text[..m.start()].chars().count();
                let col_end = text[..m.end()].chars().count();
                let matched_text = truncate_chars(m.as_str(), MATCHED_TEXT_MAX);
                let captures: Vec<String> = caps
                    .iter()
                    .skip(1)
                    .map(|g| {
                        g.map(|mat| truncate_chars(mat.as_str(), MATCHED_TEXT_MAX))
                            .unwrap_or_default()
                    })
                    .collect();
                out.push(TriggerMatch {
                    trigger_idx: tidx,
                    line,
                    col_start,
                    col_end,
                    matched_text,
                    captures,
                });
                count += 1;
            }
        }
    }
    out
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}

/// 在 GUI 线程之外启动 `exec` 触发器的命令。`pane_id` 是输出被匹配的那个
/// pane（这样 `ExecRoute::Pane` 能把 stdout 送回该 pane，而不是送回子进程结束
/// 时恰好获得焦点的 pane）。
///
/// 子进程受 [`EXEC_TIMEOUT_SECS`] 限时；超时后 future 被丢弃，`kill_on_drop`
/// 会 SIGKILL 子进程，防止泄漏。
pub fn spawn_trigger_exec(
    command: String,
    args: Vec<String>,
    route: ExecRoute,
    pane_id: u64,
) -> iced::Task<crate::iced_app::Message> {
    use std::process::Stdio;
    use std::time::Duration;
    use tokio::process::Command;

    iced::Task::perform(
        async move {
            let result: Result<String, String> = async {
                let out = match tokio::time::timeout(
                    Duration::from_secs(EXEC_TIMEOUT_SECS),
                    Command::new(&command)
                        .args(&args)
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .kill_on_drop(true)
                        .output(),
                )
                .await
                {
                    Ok(Ok(o)) => o,
                    Ok(Err(e)) => return Err(format!("spawn {command}: {e}")),
                    Err(_) => return Err(format!("exec timed out after {EXEC_TIMEOUT_SECS}s")),
                };
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    let detail = if stderr.trim().is_empty() {
                        format!("exit {}", out.status.code().unwrap_or(-1))
                    } else {
                        stderr
                    };
                    return Ok(format!("[trigger exec failed] {detail}"));
                }
                Ok(stdout)
            }
            .await;
            (pane_id, route, result)
        },
        |(pane_id, route, result)| crate::iced_app::Message::TriggerExecResult { pane_id, route, result },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiled(name: &str, pattern: &str, action: TriggerAction) -> CompiledTrigger {
        CompiledTrigger {
            cfg: TriggerConfig {
                name: name.into(),
                pattern: pattern.into(),
                action,
            },
            re: Regex::new(pattern).unwrap(),
        }
    }

    fn tpl_ctx<'a>(
        matched: &'a str,
        captures: &'a [String],
        vars: &'a HashMap<String, String>,
    ) -> TemplateContext<'a> {
        TemplateContext {
            matched_text: matched,
            captures,
            variables: vars,
        }
    }

    #[test]
    fn expand_match_and_capture() {
        let vars = HashMap::new();
        let captures = vec!["foo".into(), "bar".into()];
        let ctx = tpl_ctx("foobar", &captures, &vars);
        assert_eq!(expand_trigger_template("{match}", &ctx), "foobar");
        assert_eq!(expand_trigger_template("$1", &ctx), "foo");
        assert_eq!(expand_trigger_template("{2}", &ctx), "bar");
        assert_eq!(expand_trigger_template("\\0", &ctx), "foobar");
    }

    #[test]
    fn expand_variable() {
        let mut vars = HashMap::new();
        vars.insert("url".into(), "https://x".into());
        let ctx = tpl_ctx("x", &[], &vars);
        assert_eq!(expand_trigger_template("${var:url}", &ctx), "https://x");
        assert_eq!(expand_trigger_template("{var:url}", &ctx), "https://x");
    }

    #[test]
    fn expand_escapes() {
        let vars = HashMap::new();
        let ctx = tpl_ctx("a", &[], &vars);
        assert_eq!(expand_trigger_template("hi\\nthere", &ctx), "hi\nthere");
        assert_eq!(expand_trigger_template("\\x41", &ctx), "A");
    }

    #[test]
    fn scan_captures_groups() {
        let t = compiled("g", "(foo)(bar)", TriggerAction::Notify { title: None });
        let m = scan_lines(&[(0, "foobar")], &[t], MAX_MATCHES_PER_LINE);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].captures, vec!["foo", "bar"]);
    }

    #[test]
    fn fold_window_from_mark() {
        let mut marks = HashMap::new();
        marks.insert("start".into(), -10);
        let (top, bottom) = fold_window(-3, Some("start"), None, &marks, -20);
        assert_eq!(top, -10);
        assert_eq!(bottom, -3);
    }

    #[test]
    fn fold_window_mark_offscreen_falls_back() {
        let mut marks = HashMap::new();
        marks.insert("start".into(), -50);
        let (top, bottom) = fold_window(-3, Some("start"), Some(2), &marks, -10);
        assert_eq!(top, -5);
        assert_eq!(bottom, -3);
    }

    #[test]
    fn scan_basic_single_match() {
        let t = compiled("err", r"(?i)error", TriggerAction::Notify { title: None });
        let m = scan_lines(&[(0, "some error here")], &[t], MAX_MATCHES_PER_LINE);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].trigger_idx, 0);
        assert_eq!(m[0].line, 0);
        assert_eq!(m[0].col_start, 5);
        assert_eq!(m[0].col_end, 10);
        assert_eq!(m[0].matched_text, "error");
        assert!(m[0].captures.is_empty());
    }

    #[test]
    fn scan_multi_trigger_independent() {
        let t1 = compiled("a", "foo", TriggerAction::Notify { title: None });
        let t2 = compiled("b", "bar", TriggerAction::Notify { title: None });
        let m = scan_lines(&[(-3, "foo and bar")], &[t1, t2], MAX_MATCHES_PER_LINE);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].trigger_idx, 0);
        assert_eq!(m[0].col_start, 0);
        assert_eq!(m[1].trigger_idx, 1);
        assert_eq!(m[1].col_start, 8);
        assert_eq!(m[0].line, -3);
    }

    #[test]
    fn scan_no_match_empty() {
        let t = compiled("x", "zzz", TriggerAction::Notify { title: None });
        let m = scan_lines(&[(0, "hello"), (1, "world")], &[t], MAX_MATCHES_PER_LINE);
        assert!(m.is_empty());
    }

    #[test]
    fn scan_caps_matches_per_line() {
        let t = compiled("a", "a", TriggerAction::Notify { title: None });
        let m = scan_lines(&[(0, "aaaaa")], &[t], 3);
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn scan_columns_are_char_count_not_bytes() {
        let t = compiled("b", "b", TriggerAction::Highlight { color: None });
        let m = scan_lines(&[(0, "a你好b")], &[t], MAX_MATCHES_PER_LINE);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].col_start, 3);
        assert_eq!(m[0].col_end, 4);
    }

    #[test]
    fn window_first_wake_is_none() {
        assert_eq!(next_scan_window(None, 5, 200), None);
    }

    #[test]
    fn highlights_cap_evicts_oldest() {
        let mut s = TriggerPaneState::default();
        for i in 0..(MAX_HIGHLIGHTS + 10) {
            s.push_highlight(HighlightRegion {
                line: i as i32,
                col_start: 0,
                col_end: 1,
                color: DEFAULT_HIGHLIGHT_COLOR,
            });
        }
        assert_eq!(s.highlights.len(), MAX_HIGHLIGHTS);
        assert_eq!(s.highlights[0].line, 10);
    }

    #[test]
    fn action_discriminant_omits_payload() {
        assert_eq!(
            TriggerAction::SendText { text: "secret".into(), pane_id: None }.discriminant(),
            "send_text"
        );
        assert_eq!(
            TriggerAction::SetVariable {
                name: "x".into(),
                value: "secret".into()
            }
            .discriminant(),
            "set_variable"
        );
        assert_eq!(
            TriggerAction::Fold {
                mark: Some("m".into()),
                lines_back: None
            }
            .discriminant(),
            "fold"
        );
    }

    #[test]
    fn should_block_remote_action_guards_exec_and_cross_pane_send() {
        let exec = TriggerAction::Exec {
            command: "open".into(),
            args: vec!["$1".into()],
            route: ExecRoute::Toast,
        };
        let send_same = TriggerAction::SendText { text: "p\n".into(), pane_id: None };
        let send_cross = TriggerAction::SendText { text: "p\n".into(), pane_id: Some(7) };

        // 远程（SSH）+ 未显式开启 → 拦截 exec 与跨 pane send_text。
        assert!(should_block_remote_action(&exec, true, false));
        assert!(should_block_remote_action(&send_cross, true, false));
        // 同 pane 的 send_text 属于 reply-sudo 场景 → 从不拦截。
        assert!(!should_block_remote_action(&send_same, true, false));

        // 已显式开启 → 从不拦截。
        assert!(!should_block_remote_action(&exec, true, true));
        assert!(!should_block_remote_action(&send_cross, true, true));

        // 本地 pane → 从不拦截。
        assert!(!should_block_remote_action(&exec, false, false));
        assert!(!should_block_remote_action(&send_cross, false, false));

        // 无副作用的动作 → 从不拦截，即使是远程 + 未显式开启。
        assert!(!should_block_remote_action(
            &TriggerAction::Highlight { color: None },
            true,
            false
        ));
        assert!(!should_block_remote_action(
            &TriggerAction::Notify { title: None },
            true,
            false
        ));
        assert!(!should_block_remote_action(
            &TriggerAction::SetVariable { name: "x".into(), value: "$1".into() },
            true,
            false
        ));
        assert!(!should_block_remote_action(
            &TriggerAction::Fold { mark: None, lines_back: None },
            true,
            false
        ));
    }
}
