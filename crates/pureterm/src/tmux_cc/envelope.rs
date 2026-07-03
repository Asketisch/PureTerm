//! T-core-tmux-cc: tmux control mode (-CC) envelope parser.
//!
//! tmux's control mode (`tmux -CC`) speaks a line-oriented protocol on
//! stdout. Each notification is one of:
//!
//! - `%begin <pane-id> <seconds-since-epoch> <width> x <height>` … `%end`
//!   — frame-bounded command output (when the server is replaying a
//!   reattach).
//! - `%output <pane-id> <bytes>` — live PTY bytes (the primary "feed" path).
//! - `%input <pane-id> <bytes>` — input sent to the pane (server → client).
//! - `%session-changed <session-id> <session-name>` — reattach to a
//!   different session.
//! - `%windows <w> <s>` … `%end` — window list snapshot.
//! - `%error <msg>` — server-side error.
//! - `%pause` / `%continue` — flow control.
//! - `%subscription-changed <name> <value> <?>` — capability flag flipped.
//!
//! v1 parses the eight core envelopes and forwards each to
//! [`TmuxControlEvent`]. The PTY layer in [`super::pty`] routes the
//! `%output` bytes into an `alacritty_terminal::Term` (so the existing
//! rendering / grid code path is reused verbatim) and serializes
//! keystrokes back as `%input` envelopes.

use std::collections::HashMap;

/// One parsed tmux control-mode notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxEnvelope {
    /// `%begin %<pane_id> <ts> <W> x <H>` — frame header; payloads follow
    /// until the matching `%end`.
    Begin {
        pane_id: u32,
        timestamp: u32,
        cols: u16,
        rows: u16,
    },
    /// `%end` — close the most recent `%begin`.
    End,
    /// `%output %<pane_id> <bytes>` — raw PTY bytes to feed to the
    /// terminal emulator. The bytes are base64-encoded; we decode them
    /// here.
    Output { pane_id: u32, bytes: Vec<u8> },
    /// `%input %<pane_id> <bytes>` — echoed input the server captured
    /// (informational; the GUI doesn't need to act on it).
    Input { pane_id: u32, bytes: Vec<u8> },
    /// `%session-changed <sid> <name>` — reattach.
    SessionChanged { session_id: String, name: String },
    /// `%windows <w> <s>` — server is sending the window list. The
    /// payload is parsed by [`parse_windows_payload`].
    WindowsHeader { w: u32, s: u32 },
    /// `%error <message>` — server-side error.
    Error(String),
    /// `%pause` / `%continue` — flow control. The GUI should stop / resume
    /// forwarding `%output` bytes (we don't act on it today; the parser
    /// still surfaces the event so future code can).
    Pause,
    Continue,
    /// `%subscription-changed <name> <value> [<flags>]` — capability flag.
    SubscriptionChanged {
        name: String,
        value: String,
        flags: Option<String>,
    },
    /// Anything else — a future tmux version added it. Forwarded as raw
    /// text so the caller can decide whether to log it.
    Unknown(String),
}

impl TmuxEnvelope {
    /// Serialize this envelope to the line form tmux -CC expects on
    /// stdin. Only envelopes the GUI needs to *send* (Input, Output
    /// mirror, etc.) round-trip cleanly; the rest return `Unknown`
    /// payloads for the caller to inspect.
    pub fn serialize(&self) -> String {
        match self {
            Self::Input { pane_id, bytes } => {
                // tmux -CC accepts `%input %<id> <base64-of-bytes>\n`.
                use std::io::Write;
                let mut buf = Vec::with_capacity(bytes.len() * 4 / 3 + 16);
                buf.extend_from_slice(format!("%input %{pane_id} ").as_bytes());
                // Minimal base64 encoder (avoid pulling in extra dep).
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

/// Parse one line of tmux control-mode output. Lines that don't start
/// with `%` are returned as `Unknown` (raw line).
///
/// The byte stream from tmux is UTF-8 (we treat it as such); the
/// `%output` payload is base64 — we decode that here.
pub fn parse_envelope(line: &str) -> Option<TmuxEnvelope> {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    if !trimmed.starts_with('%') {
        return Some(TmuxEnvelope::Unknown(trimmed.to_string()));
    }
    let mut it = trimmed.splitn(3, ' ');
    let head = it.next()?;
    let head = &head[1..]; // drop '%'
    let arg1 = it.next().unwrap_or("");
    let rest = it.next().unwrap_or("");

    match head {
        "begin" => {
            // "%begin %0 1700000000 200 x 50"
            // Pane id can be "%N" or "pane_id:…" ; we take the raw token.
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
            // "%output %0 <base64>"
            let pane_id = arg1.trim_start_matches('%').parse().ok()?;
            // The payload is everything after the first space; tmux
            // splits the base64 across multiple "%output" lines when long.
            // We accept the entire remainder (the chunking is handled at
            // the framing layer; the GUI reassembles consecutive Output
            // envelopes for the same pane_id).
            let bytes = base64_decode(rest.as_bytes())?;
            Some(TmuxEnvelope::Output { pane_id, bytes })
        },
        "input" => {
            let pane_id = arg1.trim_start_matches('%').parse().ok()?;
            let bytes = base64_decode(rest.as_bytes())?;
            Some(TmuxEnvelope::Input { pane_id, bytes })
        },
        "session-changed" | "session_changed" => {
            // "%session-changed $0 main" — sid in arg1, name in rest.
            let sid = arg1.to_string();
            let name = rest.to_string();
            Some(TmuxEnvelope::SessionChanged { session_id: sid, name })
        },
        "windows" => {
            // "%windows <w> <s>" — w in arg1, s in rest.
            let w = arg1.parse().ok()?;
            let s = rest.parse().ok()?;
            Some(TmuxEnvelope::WindowsHeader { w, s })
        },
        "error" => {
            // "%error <message>" — message spans arg1 + rest.
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
            // "%subscription-changed <name> <value> [<flags>]"
            let name = arg1.to_string();
            // `rest` is "<value> [<flags>]"
            let mut it = rest.splitn(2, ' ');
            let value = it.next().unwrap_or("").to_string();
            let flags = it.next().map(|s| s.to_string());
            Some(TmuxEnvelope::SubscriptionChanged { name, value, flags })
        },
        _ => Some(TmuxEnvelope::Unknown(trimmed.to_string())),
    }
}

/// Parse the payload of a `%windows` envelope. Each line is a single
/// `window-id:window-index` pair, e.g. `@0:0`.
pub fn parse_windows_payload(payload: &str) -> Vec<(String, u32)> {
    payload
        .lines()
        .filter_map(|line| {
            let (id, idx) = line.split_once(':')?;
            Some((id.to_string(), idx.parse().ok()?))
        })
        .collect()
}

/// Parse a multi-line tmux -CC blob into a flat sequence of envelopes.
/// One envelope per line (the protocol is line-oriented). Lines that
/// don't start with `%` are returned as `Unknown`.
pub fn parse_envelopes(blob: &str) -> Vec<TmuxEnvelope> {
    blob.lines()
        .filter(|l| !l.is_empty())
        .filter_map(parse_envelope)
        .collect()
}

/// Serialize a tmux command. The result is the line to write to the
/// `tmux` child's stdin (control-mode client → server). The mirror layer
/// uses this to forward resize / send-keys / new-window events.
pub fn serialize_command(cmd: &TmuxCommand<'_>) -> String {
    match cmd {
        TmuxCommand::RefreshClient => "refresh-client".into(),
        TmuxCommand::ResizePane { id, cols, rows } => {
            format!("resize-pane -t {id} -x {cols} -y {rows}")
        },
        TmuxCommand::SendKeys { id, keys } => {
            // Send-keys takes a list of literal keys. Base64-encode any
            // binary payload so a `\x1b` in the key bytes doesn't corrupt
            // the line.
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

/// A tmux control-mode command (client → server).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxCommand<'a> {
    RefreshClient,
    ResizePane { id: &'a str, cols: u16, rows: u16 },
    SendKeys { id: &'a str, keys: &'a [u8] },
    NewWindow { name: Option<&'a str>, cwd: Option<&'a str> },
}

/// RFC 4648 base64 encoder (alphabet `A-Za-z0-9+/`, `=` padding).
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

/// RFC 4648 base64 decoder. Returns `None` on invalid length, padding
/// position, or non-alphabet bytes. Mirrors the one in
/// `alacritty_terminal::event_loop` (kept private there to avoid a
/// cross-crate dep just for base64).
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

/// Iterate over the envelopes in a (possibly multi-line) tmux stdout
/// buffer. Lines that start with `%` produce an envelope; others are
/// returned as `Unknown`. The iterator yields `None` on EOF.
pub fn iter_envelopes(buf: &str) -> impl Iterator<Item = TmuxEnvelope> + '_ {
    buf.lines().filter_map(parse_envelope)
}

/// Split a tmux stdout stream into the per-pane payload buffer. Each
/// `%begin` opens a frame; everything until the matching `%end` is
/// concatenated as the pane's "initial replay" payload (used for
/// scrollback recovery on reattach).
///
/// Returns a `pane_id → Vec<u8>` map of concatenated replay payloads.
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
        // Base64 of "hello" = "aGVsbG8="
        let env = parse_envelope("%output %0 aGVsbG8=").unwrap();
        assert_eq!(env, TmuxEnvelope::Output { pane_id: 0, bytes: b"hello".to_vec() });
    }

    #[test]
    fn parse_input_envelope() {
        // Base64 of "ls\n" = "bHMK"
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
        // Two `Begin…End` frames; the first holds "foo" for pane 0, the
        // second "bar" for pane 1.
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
        // b"\x1b[A" base64 = "G1tB"
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
