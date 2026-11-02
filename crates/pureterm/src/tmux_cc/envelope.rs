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
    /// `%begin <timestamp> <version> <flags>` —— 帧头；载荷持续到匹配的
    /// `%end`。
    ///
    /// 注意 tmux 的 `%begin` **不带 pane id** —— 它是一次全局的帧边界,
    /// 帧内可以包含多个 pane 的 `%output`。早前这里臆造了一个
    /// `pane_id` 字段并按 `%0 <ts> <W> x <H>` 解析,结果永远解析失败,
    /// 帧边界对 GUI 完全不可见。
    Begin {
        timestamp: u32,
        version: u32,
        flags: u32,
    },
    /// `%end` —— 关闭最近的 `%begin`。
    End,
    /// `%output %<pane_id> <bytes>` —— 喂给终端模拟器的原始 PTY 字节。
    /// 字节是 tmux 的 `\ooo` 八进制转义形式（见 [`decode_tmux_escapes`]）。
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
    /// `%window-add @<id>` —— 新窗口加入当前会话。
    WindowAdd { window_id: String },
    /// `%unlinked-window-add @<id>` —— 新窗口加入(未链接到任何会话)。
    UnlinkedWindowAdd { window_id: String },
    /// `%unlinked-window-close @<id>` —— 未链接窗口关闭。
    UnlinkedWindowClose { window_id: String },
    /// `%sessions-changed` —— 会话集合发生变化。
    SessionsChanged,
    /// `%client-session-changed <tty> $<sid> <name>` —— 客户端的会话绑定变化。
    ClientSessionChanged {
        tty: String,
        session_id: String,
        name: String,
    },
    /// `%layout-change @<id> <layout> <visible_layout> <flags>` —— 窗口布局变化。
    LayoutChange {
        window_id: String,
        layout: String,
        visible_layout: String,
        flags: String,
    },
    /// `%refresh-client [-C <width>x<height>] [flags]` —— **客户端 →
    /// 服务端**方向:告诉 tmux 客户端窗口的尺寸变了。
    ///
    /// 控制模式没有 PTY 的 `TIOCSWINSZ` 通道,尺寸只能这样传。
    /// `size` 为 `None` 表示不带 `-C` 参数。
    RefreshClient {
        /// `Some((cols, rows))` → `-C <cols>x<rows>`。
        size: Option<(u16, u16)>,
        /// 可选标志(如 `-f`)。
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
            Self::Begin { timestamp, version, flags } => {
                format!("%begin {timestamp} {version} {flags}\n")
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
            // 这些通知是**服务端 → 客户端**方向的;客户端不需要主动发送。
            // 原样序列化回去(带换行)可让它们往返解析器而不丢信息。
            Self::WindowAdd { window_id } => format!("%window-add {window_id}\n"),
            Self::UnlinkedWindowAdd { window_id } => {
                format!("%unlinked-window-add {window_id}\n")
            },
            Self::UnlinkedWindowClose { window_id } => {
                format!("%unlinked-window-close {window_id}\n")
            },
            Self::SessionsChanged => "%sessions-changed\n".to_string(),
            Self::ClientSessionChanged { tty, session_id, name } => {
                format!("%client-session-changed {tty} {session_id} {name}\n")
            },
            Self::LayoutChange { window_id, layout, visible_layout, flags } => {
                format!("%layout-change {window_id} {layout} {visible_layout} {flags}\n")
            },
            Self::RefreshClient { size, flags } => {
                let mut s = String::from("%refresh-client");
                if let Some((cols, rows)) = size {
                    s.push_str(&format!(" -C {cols}x{rows}"));
                }
                if let Some(flags) = flags {
                    s.push(' ');
                    s.push_str(flags);
                }
                s.push('\n');
                s
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
    let mut trimmed = line.trim_end_matches(['\r', '\n']);
    if !trimmed.starts_with('%') {
        // tmux 启动时会先发一条终端能力查询(`ESC P 1000 p`,即
        // XTGETTCAP),它**没有终止符**就紧接着第一条信封 —— 于是第一条
        // 信封被这串转义序列污染,整条被当成 `Unknown` 丢掉。
        //
        // 处理:从第一个 `%` 起重新解析(纯噪声行仍然原样返回 Unknown)。
        if let Some(pos) = trimmed.find('%') {
            if pos > 0 {
                trimmed = &trimmed[pos..];
            }
        } else {
            return Some(TmuxEnvelope::Unknown(trimmed.to_string()));
        }
    }
    let mut it = trimmed.splitn(3, ' ');
    let head = it.next()?;
    let head = &head[1..]; // 去掉 '%'
    let arg1 = it.next().unwrap_or("");
    let rest = it.next().unwrap_or("");

    match head {
        "begin" => {
            // tmux 真实格式:`%begin <timestamp> <version> <flags>`。
            // 没有 pane id —— 帧边界是全局的。
            let mut it = trimmed.split_whitespace();
            it.next()?; // "%begin"
            let timestamp = it.next()?.parse().ok()?;
            let version = it.next()?.parse().ok()?;
            let flags = it.next()?.parse().ok()?;
            Some(TmuxEnvelope::Begin { timestamp, version, flags })
        },
        "end" => Some(TmuxEnvelope::End),
        "output" => {
            // "%output %<pane_id> <payload>"
            let pane_id = arg1.trim_start_matches('%').parse().ok()?;
            // 载荷是第一个空格之后的所有内容;tmux 会把长输出拆成多条
            // "%output" 行(GUI 会把同一 pane_id 的连续 Output 信封重新组装)。
            let bytes = decode_tmux_escapes(rest.as_bytes());
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
        "window-add" => {
            Some(TmuxEnvelope::WindowAdd { window_id: arg1.to_string() })
        },
        "unlinked-window-add" => {
            Some(TmuxEnvelope::UnlinkedWindowAdd { window_id: arg1.to_string() })
        },
        "unlinked-window-close" => {
            Some(TmuxEnvelope::UnlinkedWindowClose { window_id: arg1.to_string() })
        },
        "sessions-changed" => Some(TmuxEnvelope::SessionsChanged),
        "client-session-changed" => {
            // `%client-session-changed <tty> $<sid> <name>`
            let tty = arg1.to_string();
            let mut it = rest.split_whitespace();
            let session_id = it.next().unwrap_or("").to_string();
            let name = it.collect::<Vec<_>>().join(" ");
            Some(TmuxEnvelope::ClientSessionChanged { tty, session_id, name })
        },
        "layout-change" => {
            // `%layout-change @<id> <layout> <visible_layout> <flags>`
            let mut it = rest.split_whitespace();
            let layout = it.next().unwrap_or("").to_string();
            let visible_layout = it.next().unwrap_or("").to_string();
            let flags = it.collect::<Vec<_>>().join(" ");
            Some(TmuxEnvelope::LayoutChange {
                window_id: arg1.to_string(),
                layout,
                visible_layout,
                flags,
            })
        },
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
/// 解码 tmux 控制模式对不可打印字符的转义。
///
/// tmux 的 `control_write_string` 把**非**可打印 ASCII(以及反斜杠
/// 本身)编码成 `\ooo` —— 三位八进制;其余字符原样输出。例如
/// ESC(0x1b) → `\033`、反斜杠 → `\134`。
///
/// 回归背景:早前这里按 **base64** 解码,于是每一帧终端输出都解码失败
/// → 信封被整条丢弃(返回 `None`)—— 控制模式后端因此永远显示不出
/// 任何终端内容。base64 只是**客户端发给服务端**时用的格式
/// (`%input %<id> <base64>`),服务端回传用的是八进制转义。
fn decode_tmux_escapes(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'\\' && i + 4 <= input.len() {
            let digits = &input[i + 1..i + 4];
            if digits.iter().all(|b| (b'0'..=b'7').contains(b)) {
                if let Ok(text) = std::str::from_utf8(digits) {
                    if let Ok(value) = u8::from_str_radix(text, 8) {
                        out.push(value);
                        i += 4;
                        continue;
                    }
                }
            }
        }
        out.push(input[i]);
        i += 1;
    }
    out
}

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
    // tmux 的 `%begin` / `%end` 是**全局**帧边界(帧内可含多个 pane 的
    // `%output`),不带 pane id。因此「当前面板」只能由 `%output` 行
    // 自己的 `%<pane_id>` 决定 —— 这里只跟踪「是否处在帧内」。
    let mut in_frame = false;
    for line in buf.lines() {
        if let Some(env) = parse_envelope(line) {
            match env {
                TmuxEnvelope::Begin { .. } => {
                    in_frame = true;
                },
                TmuxEnvelope::End => {
                    in_frame = false;
                },
                TmuxEnvelope::Output { pane_id, bytes } => {
                    if in_frame {
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

    /// tmux 真实的 `%begin` 格式:`%begin <timestamp> <version> <flags>`,
    /// **不带 pane id**(早前的实现臆造了 pane id,导致帧边界永远解析不出来)。
    #[test]
    fn parse_begin_end_envelope() {
        let env = parse_envelope("%begin 1700000000 318 0").unwrap();
        assert_eq!(
            env,
            TmuxEnvelope::Begin { timestamp: 1700000000, version: 318, flags: 0 }
        );
    }

    #[test]
    fn parse_end_envelope() {
        assert_eq!(parse_envelope("%end").unwrap(), TmuxEnvelope::End);
    }

    #[test]
    fn parse_output_envelope_decodes_octal_escapes() {
        // tmux 的 `%output` 载荷是 `\ooo` 八进制转义(不是 base64):
        // ESC(0x1b) → `\033`、反斜杠 → `\134`。
        let env = parse_envelope("%output %0 hello\\134world").unwrap();
        assert_eq!(
            env,
            TmuxEnvelope::Output { pane_id: 0, bytes: b"hello\\world".to_vec() }
        );

        // 真实抓包里的一条输出(ESC[1m …)。
        let env = parse_envelope("%output %7 \\033[1m\\033[0m").unwrap();
        assert_eq!(
            env,
            TmuxEnvelope::Output { pane_id: 7, bytes: vec![0x1b, b'[', b'1', b'm', 0x1b, b'[', b'0', b'm'] }
        );
    }

    /// base64 是**客户端 → 服务端**方向的格式(`%input %<id> <base64>`),
    /// 服务端回传一律用八进制转义。早前把两者搞混,导致每一帧终端
    /// 输出都被整条丢弃。
    #[test]
    fn input_envelope_still_uses_base64() {
        let env = parse_envelope("%input %0 bHMK").unwrap();
        assert_eq!(env, TmuxEnvelope::Input { pane_id: 0, bytes: b"ls\n".to_vec() });
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
        // 两个 `Begin…End` 帧。tmux 的 `%begin` 是**全局**帧边界
        // (`%begin <ts> <ver> <flags>`),帧内可以含多个 pane 的 `%output`。
        let buf = "%begin 1 318 0\n%output %0 foo\n%end\n%begin 2 318 0\n%output %1 bar\n%end\n";
        let payloads = collect_replay_payloads(buf);
        assert_eq!(payloads.get(&0), Some(&b"foo".to_vec()));
        assert_eq!(payloads.get(&1), Some(&b"bar".to_vec()));
    }

    #[test]
    fn iter_envelopes_yields_each_line() {
        let buf = "%begin 1 318 0\n%end\n";
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
