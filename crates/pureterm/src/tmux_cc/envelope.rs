//! T-core-tmux-cc：tmux 控制模式（-CC）信封解析器。
//!
//! tmux 的控制模式（`tmux -CC`）在 stdout 上使用面向行的协议。
//! 每条通知是以下之一：
//!
//! - `%begin <pane-id> <秒自纪元> <width> x <height>` … `%end`
//!   —— 帧界定的命令输出（服务器重放重连时）。
//! - `%output <pane-id> <bytes>` —— 实时 PTY 字节（主要的“喂养”路径）。
//! - `%input <pane-id> <bytes>` —— 发送到面板的输入（服务器 → 客户端）。
//! - `%session-changed <session-id> <session-name>` —— 重连到不同会话。
//! - `%windows <w> <s>` … `%end` —— 窗口列表快照。
//! - `%error <msg>` —— 服务器侧错误。
//! - `%pause` / `%continue` —— 流控。
//! - `%subscription-changed <name> <value> <?>` —— 能力标志翻转。
//!
//! v1 解析八种核心信封并把每种转发给 [`TmuxControlEvent`]。[`super::pty`]
//! 中的 PTY 层把 `%output` 字节路由进 `alacritty_terminal::Term`
//! （从而原样复用现有的渲染 / 网格代码路径），并把按键序列化回
//! `%input` 信封。

use std::collections::HashMap;

/// 一条已解析的 tmux 控制模式通知。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxEnvelope {
    /// `%begin %<pane_id> <ts> <W> x <H>` —— 帧头；载荷持续到匹配的
    /// `%end`。
    Begin {
        pane_id: u32,
        timestamp: u32,
        cols: u16,
        rows: u16,
    },
    /// `%end` —— 关闭最近的 `%begin`。
    End,
    /// `%output %<pane_id> <bytes>` —— 喂给终端模拟器的原始 PTY 字节。
    /// 字节是 base64 编码的；我们在这里解码。
    Output { pane_id: u32, bytes: Vec<u8> },
    /// `%input %<pane_id> <bytes>` —— 服务器捕获的回显输入
    /// （信息性；GUI 无需对其采取行动）。
    Input { pane_id: u32, bytes: Vec<u8> },
    /// `%session-changed <sid> <name>` —— 重连。
    SessionChanged { session_id: String, name: String },
    /// `%windows <w> <s>` —— 服务器正在发送窗口列表。载荷由
    /// [`parse_windows_payload`] 解析。
    WindowsHeader { w: u32, s: u32 },
    /// `%error <message>` —— 服务器侧错误。
    Error(String),
    /// `%pause` / `%continue` —— 流控。GUI 应停止 / 恢复转发 `%output`
    /// 字节（我们今天不处理它；解析器仍呈现该事件，便于未来代码处理）。
    Pause,
    Continue,
    /// `%subscription-changed <name> <value> [<flags>]` —— 能力标志。
    SubscriptionChanged {
        name: String,
        value: String,
        flags: Option<String>,
    },
    /// 其他任何内容——未来的 tmux 版本添加的。以原始文本转发，便于调用方
    /// 决定是否记录。
    Unknown(String),
}

impl TmuxEnvelope {
    /// 把此信封序列化为 tmux -CC 在 stdin 上期望的行形式。只有 GUI 需要
    /// *发送* 的信封（Input、Output 镜像等）能干净往返；其余返回 `Unknown`
    /// 载荷供调用方检查。
    pub fn serialize(&self) -> String {
        match self {
            Self::Input { pane_id, bytes } => {
                // tmux -CC 接受 `%input %<id> <base64-of-bytes>\n`。
                use std::io::Write;
                let mut buf = Vec::with_capacity(bytes.len() * 4 / 3 + 16);
                buf.extend_from_slice(format!("%input %{pane_id} ").as_bytes());
                // 最小化的 base64 编码器（避免引入额外依赖）。
                const TBL: &[u8; 64] =
                    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
                let mut i = 0;
                while i + 3 <= bytes.len() {
                    let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
                    buf.push(TBL[((n >> 18) & 0x3f) as usize]);
                    buf.push(TBL[((n >> 12) & 0x3f) as usize]);
                    buf.push(TBL[((n >> 6) & 0x3f) as usize]);
                    buf.push(TBL[(n & 0x3f) as usize]);
                    i += 3;
                }
                let rem = bytes.len() - i;
                if rem == 1 {
                    let n = (bytes[i] as u32) << 16;
                    buf.push(TBL[((n >> 18) & 0x3f) as usize]);
                    buf.push(TBL[((n >> 12) & 0x3f) as usize]);
                    buf.push(b'=');
                    buf.push(b'=');
                } else if rem == 2 {
                    let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
                    buf.push(TBL[((n >> 18) & 0x3f) as usize]);
                    buf.push(TBL[((n >> 12) & 0x3f) as usize]);
                    buf.push(TBL[((n >> 6) & 0x3f) as usize]);
                    buf.push(b'=');
                }
                let mut s = String::from_utf8(buf).unwrap_or_default();
                s.push('\n');
                s
            },
            Self::Output { pane_id, bytes } => {
                let mut s = format!("%output %{pane_id} {}\n", bytes.len());
                s.push_str(std::str::from_utf8(bytes).unwrap_or(""));
                s.push('\n');
                s
            },
            Self::Begin { pane_id, timestamp, cols, rows } => {
                format!("%begin %{pane_id} {timestamp} {cols} x {rows}\n")
            },
            Self::End => "%end\n".to_string(),
            Self::Pause => "%pause\n".to_string(),
            Self::Continue => "%continue\n".to_string(),
            Self::Error(msg) => format!("%error {msg}\n"),
            Self::WindowsHeader { w, s } => format!("%windows {w} {s}\n"),
            Self::SessionChanged { session_id, name } => {
                format!("%session-changed {session_id} {name}\n")
            },
            Self::SubscriptionChanged { name, value, flags } => {
                if let Some(flags) = flags {
                    format!("%subscription-changed {name} {value} {flags}\n")
                } else {
                    format!("%subscription-changed {name} {value}\n")
                }
            },
            Self::Unknown(s) => format!("{s}\n"),
        }
    }
}

/// 解析一行 tmux 控制模式输出。不以 `%` 开头的行返回为 `Unknown`
/// （原始行）。
///
/// 来自 tmux 的字节流是 UTF-8（我们按此处理）；`%output` 载荷是
/// base64——我们在这里解码。
pub fn parse_envelope(line: &str) -> Option<TmuxEnvelope> {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    if !trimmed.starts_with('%') {
        return Some(TmuxEnvelope::Unknown(trimmed.to_string()));
    }
    let mut it = trimmed.splitn(3, ' ');
    let head = it.next()?;
    let head = &head[1..]; // 去掉 '%'
    let arg1 = it.next().unwrap_or("");
    let rest = it.next().unwrap_or("");

    match head {
        "begin" => {
            // 帧头形如 “%begin %0 1700000000 200 x 50”
            // 面板 id 可以是 “%N” 或 “pane_id:…”；我们取原始 token。
            let pane_id = arg1.trim_start_matches('%').parse().ok()?;
            let mut rest_it = rest.split_whitespace();
            let ts = rest_it.next()?.parse().ok()?;
            let w = rest_it.next()?.parse().ok()?;
            rest_it.next()?; // 'x'
            let h = rest_it.next()?.parse().ok()?;
            Some(TmuxEnvelope::Begin {
                pane_id,
                timestamp: ts,
                cols: w,
                rows: h,
            })
        },
        "end" => Some(TmuxEnvelope::End),
        "output" => {
            // "%output %0 <base64>"（示例格式）
            let pane_id = arg1.trim_start_matches('%').parse().ok()?;
            // 载荷是第一个空格之后的所有内容；base64 很长时 tmux 会
            // 把它拆成多条 "%output" 行。我们接受整个剩余部分（分块在
            // 成帧层处理；GUI 会把同一 pane_id 的连续 Output 信封重新组装）。
            let bytes = base64_decode(rest.as_bytes())?;
            Some(TmuxEnvelope::Output { pane_id, bytes })
        },
        "input" => {
            let pane_id = arg1.trim_start_matches('%').parse().ok()?;
            let bytes = base64_decode(rest.as_bytes())?;
            Some(TmuxEnvelope::Input { pane_id, bytes })
        },
        "session-changed" | "session_changed" => {
            // “%session-changed $0 main” —— sid 在 arg1，名称在 rest。
            let sid = arg1.to_string();
            let name = rest.to_string();
            Some(TmuxEnvelope::SessionChanged { session_id: sid, name })
        },
        "windows" => {
            // “%windows <w> <s>” —— w 在 arg1，s 在 rest。
            let w = arg1.parse().ok()?;
            let s = rest.parse().ok()?;
            Some(TmuxEnvelope::WindowsHeader { w, s })
        },
        "error" => {
            // “%error <message>” —— 消息横跨 arg1 + rest。
            let mut msg = String::from(arg1);
            if !rest.is_empty() {
                if !msg.is_empty() {
                    msg.push(' ');
                }
                msg.push_str(rest);
            }
            Some(TmuxEnvelope::Error(msg))
        },
        "pause" => Some(TmuxEnvelope::Pause),
        "continue" => Some(TmuxEnvelope::Continue),
        "subscription-changed" | "subscription_changed" => {
            // "%subscription-changed <name> <value> [<flags>]"（示例格式）
            let name = arg1.to_string();
            // `rest` 即 "<value> [<flags>]"
            let mut it = rest.splitn(2, ' ');
            let value = it.next().unwrap_or("").to_string();
            let flags = it.next().map(|s| s.to_string());
            Some(TmuxEnvelope::SubscriptionChanged { name, value, flags })
        },
        _ => Some(TmuxEnvelope::Unknown(trimmed.to_string())),
    }
}

/// 解析 `%windows` 信封的载荷。每行是一个 `window-id:window-index`
/// 对，例如 `@0:0`。
pub fn parse_windows_payload(payload: &str) -> Vec<(String, u32)> {
    payload
        .lines()
        .filter_map(|line| {
            let (id, idx) = line.split_once(':')?;
            Some((id.to_string(), idx.parse().ok()?))
        })
        .collect()
}

/// 把多行 tmux -CC 数据块解析为扁平的信封序列。每行一个信封
/// （协议是面向行的）。不以 `%` 开头的行返回为 `Unknown`。
pub fn parse_envelopes(blob: &str) -> Vec<TmuxEnvelope> {
    blob.lines()
        .filter(|l| !l.is_empty())
        .filter_map(parse_envelope)
        .collect()
}

/// 序列化一条 tmux 命令。结果是写入 `tmux` 子进程 stdin 的行
/// （控制模式客户端 → 服务器）。镜像层用它转发 resize /
/// send-keys / new-window 事件。
pub fn serialize_command(cmd: &TmuxCommand<'_>) -> String {
    match cmd {
        TmuxCommand::RefreshClient => "refresh-client".into(),
        TmuxCommand::ResizePane { id, cols, rows } => {
            format!("resize-pane -t {id} -x {cols} -y {rows}")
        },
        TmuxCommand::SendKeys { id, keys } => {
            // Send-keys 接受字面键列表。对任何二进制载荷做 base64 编码，
            // 使键字节中的 `\x1b` 不会破坏行。
            let b64 = base64_encode(keys);
            format!("send-keys -t {id} -H {b64}")
        },
        TmuxCommand::NewWindow { name, cwd } => {
            let mut s = format!("new-window -P -F '#{{window_id}}'");
            if let Some(n) = name {
                s.push_str(&format!(" -n {n}"));
            }
            if let Some(c) = cwd {
                s.push_str(&format!(" -c {c}"));
            }
            s
        },
    }
}

/// 一条 tmux 控制模式命令（客户端 → 服务器）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxCommand<'a> {
    RefreshClient,
    ResizePane { id: &'a str, cols: u16, rows: u16 },
    SendKeys { id: &'a str, keys: &'a [u8] },
    NewWindow { name: Option<&'a str>, cwd: Option<&'a str> },
}

/// RFC 4648 base64 编码器（字母表 `A-Za-z0-9+/`，`=` 填充）。
fn base64_encode(input: &[u8]) -> String {
    const ALPH: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out.push(ALPH[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPH[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPH[((n >> 6) & 0x3f) as usize] as char);
        out.push(ALPH[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(ALPH[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPH[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(ALPH[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPH[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPH[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}

/// RFC 4648 base64 解码器。长度无效、填充位置错误或非字母表字节时
/// 返回 `None`。镜像 `alacritty_terminal::event_loop` 中的那个
/// （在那里保持私有，以避免仅为 base64 引入跨 crate 依赖）。
fn base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    fn val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    if input.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut chunk = [0u8; 4];
    for (i, &b) in input.iter().enumerate() {
        let idx = i % 4;
        if b == b'=' {
            if idx < 2 {
                return None;
            }
            chunk[idx] = 0;
        } else {
            chunk[idx] = val(b)?;
        }
        if idx == 3 {
            out.push((chunk[0] << 2) | (chunk[1] >> 4));
            if input[i - 1] != b'=' {
                out.push((chunk[1] << 4) | (chunk[2] >> 2));
            }
            if input[i] != b'=' {
                out.push((chunk[2] << 6) | chunk[3]);
            }
        }
    }
    Some(out)
}

/// 遍历（可能多行的）tmux stdout 缓冲区中的信封。以 `%` 开头的行产生
/// 信封；其余返回为 `Unknown`。迭代器在 EOF 时产出 `None`。
pub fn iter_envelopes(buf: &str) -> impl Iterator<Item = TmuxEnvelope> + '_ {
    buf.lines().filter_map(parse_envelope)
}

/// 把 tmux stdout 流拆分为按面板的载荷缓冲区。每个 `%begin` 打开一个
/// 帧；直到匹配 `%end` 之前的所有内容拼接为该面板的“初始重放”载荷
/// （用于重连时的回滚恢复）。
///
/// 返回拼接后重放载荷的 `pane_id → Vec<u8>` 映射。
pub fn collect_replay_payloads(buf: &str) -> HashMap<u32, Vec<u8>> {
    let mut out: HashMap<u32, Vec<u8>> = HashMap::new();
    let mut current: Option<u32> = None;
    for line in buf.lines() {
        if let Some(env) = parse_envelope(line) {
            match env {
                TmuxEnvelope::Begin { pane_id, .. } => {
                    current = Some(pane_id);
                },
                TmuxEnvelope::End => {
                    current = None;
                },
                TmuxEnvelope::Output { pane_id, bytes } => {
                    if current == Some(pane_id) {
                        out.entry(pane_id).or_default().extend(bytes);
                    }
                },
                _ => {},
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_begin_end_envelope() {
        let env = parse_envelope("%begin %0 1700000000 200 x 50").unwrap();
        assert_eq!(
            env,
            TmuxEnvelope::Begin {
                pane_id: 0,
                timestamp: 1700000000,
                cols: 200,
                rows: 50
            }
        );
    }

    #[test]
    fn parse_end_envelope() {
        assert_eq!(parse_envelope("%end").unwrap(), TmuxEnvelope::End);
    }

    #[test]
    fn parse_output_envelope_decodes_base64() {
        // “hello” 的 base64 = “aGVsbG8=”
        let env = parse_envelope("%output %0 aGVsbG8=").unwrap();
        assert_eq!(env, TmuxEnvelope::Output { pane_id: 0, bytes: b"hello".to_vec() });
    }

    #[test]
    fn parse_input_envelope() {
        // “ls\n” 的 base64 = “bHMK”
        let env = parse_envelope("%input %0 bHMK").unwrap();
        assert_eq!(env, TmuxEnvelope::Input { pane_id: 0, bytes: b"ls\n".to_vec() });
    }

    #[test]
    fn parse_session_changed() {
        let env = parse_envelope("%session-changed $0 main").unwrap();
        assert_eq!(
            env,
            TmuxEnvelope::SessionChanged {
                session_id: "$0".into(),
                name: "main".into()
            }
        );
    }

    #[test]
    fn parse_windows_header() {
        let env = parse_envelope("%windows 3 1").unwrap();
        assert_eq!(env, TmuxEnvelope::WindowsHeader { w: 3, s: 1 });
    }

    #[test]
    fn parse_windows_payload_parses_index_pairs() {
        let payload = "@0:0\n@1:1\n@2:2";
        let pairs = parse_windows_payload(payload);
        assert_eq!(
            pairs,
            vec![
                ("@0".to_string(), 0),
                ("@1".to_string(), 1),
                ("@2".to_string(), 2),
            ]
        );
    }

    #[test]
    fn parse_error() {
        let env = parse_envelope("%error can't find pane: %3").unwrap();
        assert_eq!(env, TmuxEnvelope::Error("can't find pane: %3".into()));
    }

    #[test]
    fn parse_pause_continue() {
        assert_eq!(parse_envelope("%pause").unwrap(), TmuxEnvelope::Pause);
        assert_eq!(parse_envelope("%continue").unwrap(), TmuxEnvelope::Continue);
    }

    #[test]
    fn parse_subscription_changed() {
        let env = parse_envelope("%subscription-changed output-bell off").unwrap();
        assert_eq!(
            env,
            TmuxEnvelope::SubscriptionChanged {
                name: "output-bell".into(),
                value: "off".into(),
                flags: None
            }
        );
    }

    #[test]
    fn parse_non_envelope_line_is_unknown() {
        let env = parse_envelope("regular log line").unwrap();
        assert_eq!(env, TmuxEnvelope::Unknown("regular log line".into()));
    }

    #[test]
    fn nested_begin_end_groups_replay_payloads() {
        // 两个 `Begin…End` 帧；第一个为面板 0 持有 “foo”，第二个为
        // 面板 1 持有 “bar”。
        let buf = "%begin %0 1 80 x 24\n%output %0 Zm9v\n%end\n%begin %1 1 80 x 24\n%output %1 YmFy\n%end\n";
        let payloads = collect_replay_payloads(buf);
        assert_eq!(payloads.get(&0), Some(&b"foo".to_vec()));
        assert_eq!(payloads.get(&1), Some(&b"bar".to_vec()));
    }

    #[test]
    fn iter_envelopes_yields_each_line() {
        let buf = "%begin %0 1 80 x 24\n%end\n";
        let envs: Vec<_> = iter_envelopes(buf).collect();
        assert_eq!(envs.len(), 2);
        assert!(matches!(envs[0], TmuxEnvelope::Begin { .. }));
        assert_eq!(envs[1], TmuxEnvelope::End);
    }

    #[test]
    fn serialize_refresh_client() {
        assert_eq!(serialize_command(&TmuxCommand::RefreshClient), "refresh-client");
    }

    #[test]
    fn serialize_resize_pane() {
        assert_eq!(
            serialize_command(&TmuxCommand::ResizePane { id: "%0", cols: 200, rows: 50 }),
            "resize-pane -t %0 -x 200 -y 50"
        );
    }

    #[test]
    fn serialize_send_keys_base64() {
        let keys = b"\x1b[A";
        let cmd = serialize_command(&TmuxCommand::SendKeys { id: "%0", keys });
        // b“\x1b[A” 的 base64 = “G1tB”
        assert!(cmd.starts_with("send-keys -t %0 -H G1tB"));
    }

    #[test]
    fn serialize_new_window_with_name_and_cwd() {
        let cmd = serialize_command(&TmuxCommand::NewWindow {
            name: Some("editor"),
            cwd: Some("/tmp"),
        });
        assert!(cmd.contains("-n editor"));
        assert!(cmd.contains("-c /tmp"));
    }
}
