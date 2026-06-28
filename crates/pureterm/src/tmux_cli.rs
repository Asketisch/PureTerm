//! GAP-6.6: `pureterm tmux <command>` — a tmux-compatible command subset.
//!
//! Each command maps onto the existing pane/surface Socket API (and the daemon
//! for `pipe-pane` / `wait-for`). Target syntax (the `-t` / `-s` flags) is a
//! deliberately small subset of tmux's:
//!   - omitted            → active pane
//!   - `%N`               → pane whose id is `N`; if none has id `N`, falls back
//!                           to the N-th pane in DFS order (so `%0` always hits
//!                           the first pane, matching the DoD)
//!   - `:+` / `:-`        → next / previous pane (DFS order)
//!   - `session:win.pane` → out of scope (the full tmux grammar is not parsed)
//!
//! Phase 1 (implemented arms): capture-pane, list-panes, send-keys,
//! split-window, kill-pane, select-pane (next/prev), display-message.
//! Phase 2 (resize/swap/break/select-layout), phase 3 (join-pane) and phase 4
//! (pipe-pane/wait-for, daemon-backed) are wired into the CLI parser but return
//! a "not yet implemented" error until their phases land.

use std::path::PathBuf;

use clap::Subcommand;
use serde_json::{json, Value};

use crate::ipc::client::send_rpc;

/// `pureterm tmux <command>` — tmux-compatible subset (GAP-6.6).
#[derive(Subcommand)]
pub enum TmuxSub {
    /// Capture a pane's terminal buffer to stdout (tmux `capture-pane`).
    CapturePane {
        /// Target pane (`%N` / `:+` / `:-` / omitted=active).
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// Print to stdout (tmux `-p`). Defaults to the last 50 lines.
        #[arg(short = 'p', long)]
        print: bool,
        /// Start line (tmux `-S`). `-` = full scrollback; `0` = viewport top.
        #[arg(short = 'S', long = "start", allow_hyphen_values = true)]
        start: Option<String>,
        /// End line (tmux `-E`). `-` = viewport bottom.
        #[arg(short = 'E', long = "end", allow_hyphen_values = true)]
        end: Option<String>,
        /// Keep only the last N lines.
        #[arg(long = "lines")]
        lines: Option<usize>,
    },
    /// List panes (tmux `list-panes`).
    ListPanes {
        /// Format string (tmux `-F`). Variables: `#{pane_id}` `#{pane_index}`
        /// `#{tab_index}` `#{tab_title}` `#{pane_active}` `#{pane_current_path}`.
        #[arg(short = 'F', long = "format")]
        format: Option<String>,
        #[arg(short = 't', long = "target")]
        target: Option<String>,
    },
    /// Send keys to a pane's PTY (tmux `send-keys`).
    SendKeys {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// Keys to send. Known names (Enter/Tab/Escape/Space/BSpace/Up/Down/...)
        /// are translated; everything else is sent literally and concatenated.
        keys: Vec<String>,
    },
    /// Split the active pane (tmux `split-window`). `-h` = side by side.
    SplitWindow {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// Split horizontally (side by side) — tmux `-h`.
        #[arg(short = 'h', long)]
        horizontal: bool,
        /// Split vertically (stacked) — tmux default / `-v`.
        #[arg(short = 'v', long)]
        vertical: bool,
        /// Optional command (ignored in phase 1 — the new pane is a shell).
        command: Option<String>,
    },
    /// Close the active pane (tmux `kill-pane`).
    KillPane {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
    },
    /// Move focus between panes (tmux `select-pane`).
    SelectPane {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// Geometry focus (phase 2).
        #[arg(short = 'L', long = "left")]
        left: bool,
        #[arg(short = 'R', long = "right")]
        right: bool,
        #[arg(short = 'U', long = "up")]
        up: bool,
        #[arg(short = 'D', long = "down")]
        down: bool,
        /// Toggle to the last pane (not yet supported).
        #[arg(short = 'l', long = "last")]
        last: bool,
    },
    /// Resize a pane (tmux `resize-pane`) — phase 2.
    ResizePane {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        #[arg(short = 'L', long = "left")]
        left: Option<u32>,
        #[arg(short = 'R', long = "right")]
        right: Option<u32>,
        #[arg(short = 'U', long = "up")]
        up: Option<u32>,
        #[arg(short = 'D', long = "down")]
        down: Option<u32>,
        #[arg(short = 'x', long)]
        x: Option<usize>,
        #[arg(short = 'y', long)]
        y: Option<usize>,
    },
    /// Swap two panes (tmux `swap-pane`) — phase 2.
    SwapPane {
        #[arg(short = 's', long = "source")]
        source: Option<String>,
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        #[arg(short = 'D', long = "down")]
        down: bool,
        #[arg(short = 'U', long = "up")]
        up: bool,
    },
    /// Break a pane out into its own tab (tmux `break-pane`) — phase 2.
    BreakPane {
        #[arg(short = 's', long = "source")]
        source: Option<String>,
        #[arg(short = 't', long = "target")]
        target: Option<String>,
    },
    /// Apply a preset layout (tmux `select-layout`) — phase 2.
    SelectLayout {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        layout: String,
    },
    /// Move a pane from another tab/split here (tmux `join-pane`) — phase 3.
    JoinPane {
        #[arg(short = 's', long = "source")]
        source: Option<String>,
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        #[arg(short = 'h', long)]
        horizontal: bool,
        #[arg(short = 'v', long)]
        vertical: bool,
    },
    /// Pipe a pane's output to a shell command (tmux `pipe-pane`) — phase 4.
    PipePane {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// Only pipe output, not input (tmux `-o`).
        #[arg(short = 'o', long)]
        output: bool,
        command: Option<String>,
    },
    /// Wait for / signal a named channel or lock (tmux `wait-for`) — phase 4.
    WaitFor {
        /// Channel or lock name.
        channel: String,
        /// Signal the channel, waking all waiters (tmux `-S`).
        #[arg(short = 'S', long = "signal")]
        signal: bool,
        /// Acquire a lock (tmux `-L`).
        #[arg(short = 'L', long = "lock")]
        lock: bool,
        /// Release a lock (tmux `-U`).
        #[arg(short = 'U', long = "unlock")]
        unlock: bool,
    },
    /// Print a message (tmux `display-message`).
    DisplayMessage {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        #[arg(short = 'p', long)]
        print: bool,
        #[arg(short = 'F', long = "format")]
        format: Option<String>,
    },
}

/// Entry point — mirrors `daemon_cli::dispatch` / `tunnel_cli::start`.
pub fn dispatch(cmd: TmuxSub, socket: &Option<PathBuf>, _json: bool) {
    if let Err(msg) = run(cmd, socket) {
        eprintln!("[pureterm tmux] {msg}");
        std::process::exit(1);
    }
}

fn run(cmd: TmuxSub, socket: &Option<PathBuf>) -> Result<(), String> {
    match cmd {
        TmuxSub::CapturePane { target, print, start, end, lines } => {
            run_capture(socket, target, print, start, end, lines)
        }
        TmuxSub::ListPanes { format, target: _ } => run_list(socket, format),
        TmuxSub::SendKeys { target, keys } => run_send_keys(socket, target, keys),
        TmuxSub::SplitWindow { horizontal, .. } => run_split(socket, horizontal),
        TmuxSub::KillPane { target } => run_kill(socket, target),
        TmuxSub::SelectPane { target, left, right, up, down, last } => {
            run_select(socket, target, left, right, up, down, last)
        }
        TmuxSub::DisplayMessage { format, .. } => {
            if let Some(m) = format {
                println!("{m}");
            }
            Ok(())
        }
        TmuxSub::ResizePane { target, left, right, up, down, x, y } => {
            run_resize(socket, target, left, right, up, down, x, y)
        }
        TmuxSub::SwapPane { source, target, down, up } => {
            run_swap(socket, source, target, down, up)
        }
        TmuxSub::BreakPane { source, .. } => run_break(socket, source),
        TmuxSub::SelectLayout { .. } => {
            // MVP: every preset layout (even-*/tiled/main-*) maps to equalize.
            rpc(socket, "pane.equalize", None)?;
            Ok(())
        }
        TmuxSub::JoinPane { source, target, horizontal, vertical: _ } => {
            run_join(socket, source, target, horizontal)
        }
        TmuxSub::PipePane { target, output: _, command } => run_pipe(socket, target, command),
        TmuxSub::WaitFor { channel, signal, lock, unlock } => {
            run_wait_for(channel, signal, lock, unlock)
        }
    }
}

// ── helpers ──────────────────────────────────────────────────────────────

/// One row of `pane.list`, flattened across all tabs (DFS order).
struct PaneInfo {
    pane_id: u64,
    pane_index: usize,
    tab: usize,
    tab_title: String,
    active: bool,
    cwd: Option<String>,
}

/// Call a Socket API method and unwrap the result value (error on transport /
/// JSON-RPC error or missing result field).
fn rpc(socket: &Option<PathBuf>, method: &str, params: Option<Value>) -> Result<Value, String> {
    send_rpc(method, params, socket.clone())?.ok_or_else(|| format!("{method}: empty result"))
}

fn resolve_panes(socket: &Option<PathBuf>) -> Result<Vec<PaneInfo>, String> {
    let v = rpc(socket, "pane.list", None)?;
    let arr = v.as_array().ok_or("pane.list returned non-array")?;
    Ok(arr
        .iter()
        .map(|p| PaneInfo {
            pane_id: p.get("pane_id").and_then(|x| x.as_u64()).unwrap_or(0),
            pane_index: p.get("pane_index").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
            tab: p.get("tab").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
            tab_title: p.get("tab_title").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            active: p.get("active").and_then(|x| x.as_bool()).unwrap_or(false),
            cwd: p.get("cwd").and_then(|x| x.as_str()).map(String::from),
        })
        .collect())
}

/// Resolve a tmux `-t` target to a pane id. See the module docs for the
/// supported grammar.
fn parse_target(target: &Option<String>, panes: &[PaneInfo]) -> Result<u64, String> {
    if panes.is_empty() {
        return Err("no panes".into());
    }
    let active = panes
        .iter()
        .find(|p| p.active)
        .or_else(|| panes.first())
        .map(|p| p.pane_id)
        .unwrap();
    let Some(t) = target else { return Ok(active); };
    let t = t.trim();
    match t {
        ":+" => {
            let pos = panes.iter().position(|p| p.active).unwrap_or(0);
            Ok(panes[(pos + 1) % panes.len()].pane_id)
        }
        ":-" => {
            let pos = panes.iter().position(|p| p.active).unwrap_or(0);
            Ok(panes[(pos + panes.len() - 1) % panes.len()].pane_id)
        }
        _ if t.starts_with('%') => {
            let n: u64 = t[1..].parse().map_err(|_| format!("bad pane id {t:?}"))?;
            // Exact pane-id match wins; otherwise fall back to DFS index so
            // `%0` always resolves to the first pane (PureTerm ids start at 1).
            if let Some(p) = panes.iter().find(|p| p.pane_id == n) {
                Ok(p.pane_id)
            } else if (n as usize) < panes.len() {
                Ok(panes[n as usize].pane_id)
            } else {
                Err(format!("no pane matches {t:?}"))
            }
        }
        _ => Err(format!(
            "unsupported target {t:?} (use %N / :+ / :- / omit for active)"
        )),
    }
}

/// Translate a tmux key name to bytes; unknown names are sent literally.
fn map_key(k: &str) -> String {
    match k {
        "Enter" | "C-m" | "Return" => "\r".into(),
        "Tab" | "C-i" => "\t".into(),
        "Escape" | "C-[" => "\x1b".into(),
        "Space" => " ".into(),
        "BSpace" | "C-h" | "C-?" => "\x7f".into(),
        "Up" => "\x1b[A".into(),
        "Down" => "\x1b[B".into(),
        "Right" => "\x1b[C".into(),
        "Left" => "\x1b[D".into(),
        "Home" => "\x1bOH".into(),
        "End" => "\x1bOF".into(),
        "DC" | "Delete" => "\x1b[3~".into(),
        _ => k.to_string(),
    }
}

/// Render a tmux `-F` format string against a pane row.
fn render_format(fmt: &str, p: &PaneInfo) -> String {
    let active = if p.active { "1" } else { "0" };
    let cwd = p.cwd.clone().unwrap_or_default();
    fmt.replace("#{pane_id}", &p.pane_id.to_string())
        .replace("#{pane_index}", &p.pane_index.to_string())
        .replace("#{window_index}", &p.tab.to_string())
        .replace("#{tab_index}", &p.tab.to_string())
        .replace("#{session_name}", &p.tab_title)
        .replace("#{window_name}", &p.tab_title)
        .replace("#{pane_active}", active)
        .replace("#{pane_current_path}", &cwd)
}

const DEFAULT_LIST_FORMAT: &str =
    "#{pane_id}: [#{pane_index}] #{tab_index}:#{tab_title} #{pane_active} #{pane_current_path}";

// ── command implementations (phase 1) ────────────────────────────────────

fn run_capture(
    socket: &Option<PathBuf>,
    target: Option<String>,
    print: bool,
    start: Option<String>,
    _end: Option<String>,
    lines: Option<usize>,
) -> Result<(), String> {
    let panes = resolve_panes(socket)?;
    let pane_id = parse_target(&target, &panes)?;
    // `-S 0` ⇒ visible viewport only; otherwise full scrollback. `-p` ⇒ keep
    // the last 50 lines (the DoD); an explicit `--lines N` overrides.
    let visible_only = start.as_deref() == Some("0");
    let lines_cap = match lines {
        Some(n) => Some(n),
        None if print => Some(50),
        None => None,
    };
    let mut params = serde_json::Map::new();
    params.insert("pane_id".into(), json!(pane_id));
    if visible_only {
        params.insert("visible_only".into(), json!(true));
    }
    if let Some(n) = lines_cap {
        params.insert("lines".into(), json!(n));
    }
    let resp = rpc(socket, "surface.snapshot", Some(Value::Object(params)))?;
    let text = resp.get("text").and_then(|v| v.as_str()).unwrap_or("");
    // tmux `capture-pane` without `-p` stashes into the paste buffer; we have
    // none, so always emit to stdout and note it when `-p` was absent.
    if !print {
        eprintln!(
            "[pureterm tmux] (capture-pane without -p: written to stdout; paste buffer unsupported)"
        );
    }
    print!("{text}");
    Ok(())
}

fn run_list(socket: &Option<PathBuf>, format: Option<String>) -> Result<(), String> {
    let panes = resolve_panes(socket)?;
    let fmt = format.unwrap_or_else(|| DEFAULT_LIST_FORMAT.to_string());
    for p in &panes {
        println!("{}", render_format(&fmt, p));
    }
    Ok(())
}

fn run_send_keys(
    socket: &Option<PathBuf>,
    target: Option<String>,
    keys: Vec<String>,
) -> Result<(), String> {
    let panes = resolve_panes(socket)?;
    let pane_id = parse_target(&target, &panes)?;
    let text: String = keys.iter().map(|k| map_key(k)).collect();
    rpc(socket, "surface.send_text", Some(json!({ "pane_id": pane_id, "text": text })))?;
    Ok(())
}

fn run_split(socket: &Option<PathBuf>, horizontal: bool) -> Result<(), String> {
    // tmux default = `-v` (stacked); `-h` = side by side. PureTerm's
    // `pane.split` "horizontal" = side by side, "vertical" = stacked, so the
    // mapping is direct.
    let dir = if horizontal { "horizontal" } else { "vertical" };
    rpc(socket, "pane.split", Some(json!({ "direction": dir })))?;
    Ok(())
}

fn run_kill(socket: &Option<PathBuf>, target: Option<String>) -> Result<(), String> {
    // Phase 1: `pane.close` only closes the active pane. Targeted close lands
    // in phase 2 (pane.close pane_id); for now warn + close active.
    if target.is_some() {
        eprintln!(
            "[pureterm tmux] kill-pane -t needs pane.close target support (phase 2); closing active"
        );
    }
    rpc(socket, "pane.close", None)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_select(
    socket: &Option<PathBuf>,
    target: Option<String>,
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    last: bool,
) -> Result<(), String> {
    if left || right || up || down {
        return Err("geometry select-pane (-L/-R/-U/-D) lands in phase 2".into());
    }
    if last {
        return Err("select-pane -l (last pane) not yet supported".into());
    }
    let dir = match target.as_deref() {
        Some(":+") => "next",
        Some(":-") => "prev",
        None => "next",
        Some(t) => {
            return Err(format!(
                "select-pane -t {t:?} (absolute target) lands in phase 2"
            ))
        }
    };
    rpc(socket, "pane.focus", Some(json!({ "direction": dir })))?;
    Ok(())
}

// ── command implementations (phase 2: layout ops) ───────────────────────

/// Resolve an optional `-t`/`-s` target to a pane id (None ⇒ active / unset).
fn resolve_optional_target(
    socket: &Option<PathBuf>,
    target: &Option<String>,
) -> Result<Option<u64>, String> {
    match target {
        None => Ok(None),
        Some(_) => {
            let panes = resolve_panes(socket)?;
            Ok(Some(parse_target(target, &panes)?))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_resize(
    socket: &Option<PathBuf>,
    target: Option<String>,
    left: Option<u32>,
    right: Option<u32>,
    up: Option<u32>,
    down: Option<u32>,
    x: Option<usize>,
    y: Option<usize>,
) -> Result<(), String> {
    let pane_id = resolve_optional_target(socket, &target)?;
    let mut params = serde_json::Map::new();
    if let Some(id) = pane_id {
        params.insert("pane_id".into(), json!(id));
    }
    // Absolute -x/-y win; else the direction flags; else a default nudge.
    if let Some(c) = x {
        params.insert("cols".into(), json!(c));
    } else if let Some(r) = y {
        params.insert("rows".into(), json!(r));
    } else {
        let (dir, amount) = if let Some(n) = left {
            ("left", n)
        } else if let Some(n) = right {
            ("right", n)
        } else if let Some(n) = up {
            ("up", n)
        } else if let Some(n) = down {
            ("down", n)
        } else {
            ("right", 1)
        };
        params.insert("direction".into(), json!(dir));
        params.insert("amount".into(), json!(amount));
    }
    rpc(socket, "pane.resize", Some(Value::Object(params)))?;
    Ok(())
}

fn run_swap(
    socket: &Option<PathBuf>,
    source: Option<String>,
    target: Option<String>,
    down: bool,
    up: bool,
) -> Result<(), String> {
    let panes = resolve_panes(socket)?;
    let (src, dst) = if source.is_some() && target.is_some() {
        (parse_target(&source, &panes)?, parse_target(&target, &panes)?)
    } else {
        // -U/-D: swap the active pane with its prev/next neighbor (DFS order).
        let active = panes
            .iter()
            .find(|p| p.active)
            .map(|p| p.pane_id)
            .ok_or("no active pane")?;
        let probe = if up { Some(":-".into()) } else { Some(":+".into()) };
        let other = parse_target(&probe, &panes)?;
        (active, other)
    };
    rpc(socket, "pane.swap", Some(json!({ "src": src, "dst": dst })))?;
    Ok(())
}

fn run_break(socket: &Option<PathBuf>, source: Option<String>) -> Result<(), String> {
    let pane_id = resolve_optional_target(socket, &source)?;
    let mut params = serde_json::Map::new();
    if let Some(id) = pane_id {
        params.insert("pane_id".into(), json!(id));
    }
    rpc(socket, "pane.detach", Some(Value::Object(params)))?;
    Ok(())
}

fn run_join(
    socket: &Option<PathBuf>,
    source: Option<String>,
    target: Option<String>,
    horizontal: bool,
) -> Result<(), String> {
    let panes = resolve_panes(socket)?;
    let active_id = panes.iter().find(|p| p.active).map(|p| p.pane_id);
    let active_tab = panes.iter().find(|p| p.active).map(|p| p.tab);
    // -s source (default: active pane).
    let src = match &source {
        Some(_) => parse_target(&source, &panes)?,
        None => active_id.ok_or("no active pane")?,
    };
    // -t target: a tab index, or `%N` (that pane's tab); default active tab.
    let dst_tab = match &target {
        Some(t) => {
            if let Ok(n) = t.parse::<usize>() {
                n
            } else if t.starts_with('%') {
                let id = parse_target(&target, &panes)?;
                panes
                    .iter()
                    .find(|p| p.pane_id == id)
                    .map(|p| p.tab)
                    .ok_or_else(|| format!("pane {id} not found"))?
            } else {
                return Err(format!("join-pane -t {t:?}: expected tab index or %N"));
            }
        }
        None => active_tab.ok_or("no active tab")?,
    };
    let dir = if horizontal { "horizontal" } else { "vertical" };
    rpc(
        socket,
        "pane.attach",
        Some(json!({ "src_pane_id": src, "dst_tab_index": dst_tab, "direction": dir })),
    )?;
    Ok(())
}

fn run_pipe(
    socket: &Option<PathBuf>,
    target: Option<String>,
    command: Option<String>,
) -> Result<(), String> {
    let panes = resolve_panes(socket)?;
    let pane_id = parse_target(&target, &panes)?;
    let mut params = serde_json::Map::new();
    params.insert("pane_id".into(), json!(pane_id));
    if let Some(c) = command {
        params.insert("command".into(), json!(c));
    }
    rpc(socket, "surface.pipe", Some(Value::Object(params)))?;
    Ok(())
}

/// GAP-6.6: `wait-for` connects to the daemon directly and blocks — `wait`/
/// `lock` park until signaled. We bypass `daemon_client::rpc` (5s read
/// timeout) with a raw blocking Unix socket so the park is unbounded, matching
/// tmux.
fn run_wait_for(channel: String, signal: bool, lock: bool, unlock: bool) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let socket = crate::daemon_client::ensure_daemon()
        .map_err(|e| format!("daemon unavailable: {e}"))?;
    let method = match (signal, lock, unlock) {
        (true, _, _) => "wait_for.signal",
        (_, true, _) => "wait_for.lock",
        (_, _, true) => "wait_for.unlock",
        _ => "wait_for.wait",
    };
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let req = json!({
        "jsonrpc": "2.0", "id": id, "method": method,
        "params": { "channel": channel }
    });
    let mut line = serde_json::to_string(&req).map_err(|e| format!("serialize: {e}"))?;
    line.push('\n');
    let mut stream = UnixStream::connect(&socket).map_err(|e| format!("connect: {e}"))?;
    stream.write_all(line.as_bytes()).map_err(|e| format!("write: {e}"))?;
    stream.shutdown(std::net::Shutdown::Write).ok();
    let mut buf = String::new();
    BufReader::new(&stream)
        .read_line(&mut buf)
        .map_err(|e| format!("read: {e}"))?;
    let v: Value = serde_json::from_str(buf.trim()).map_err(|e| format!("parse: {e}"))?;
    if let Some(err) = v.get("error") {
        return Err(format!("wait-for: {err}"));
    }
    println!("{}", v.get("result").map(|r| r.to_string()).unwrap_or_else(|| "ok".into()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use crate::ipc::methods::{AppSnapshot, IpcSideEffect, SocketRouter};
    #[cfg(unix)]
    use std::io::{Read, Write};

    fn pane(id: u64, idx: usize, active: bool) -> PaneInfo {
        PaneInfo {
            pane_id: id,
            pane_index: idx,
            tab: 0,
            tab_title: "tab".into(),
            active,
            cwd: Some("/home/u".into()),
        }
    }

    fn sample() -> Vec<PaneInfo> {
        vec![pane(1, 0, true), pane(2, 1, false), pane(3, 2, false)]
    }

    #[test]
    fn target_omitted_returns_active() {
        let panes = sample();
        assert_eq!(parse_target(&None, &panes).unwrap(), 1);
    }

    #[test]
    fn target_percent_exact_pane_id() {
        let panes = sample();
        assert_eq!(parse_target(&Some("%3".into()), &panes).unwrap(), 3);
    }

    #[test]
    fn target_percent_zero_falls_back_to_first() {
        // pane ids start at 1, so `%0` has no exact match → DFS index 0.
        let panes = sample();
        assert_eq!(parse_target(&Some("%0".into()), &panes).unwrap(), 1);
    }

    #[test]
    fn target_percent_out_of_range_errors() {
        let panes = sample(); // ids 1,2,3 — no id 9 and DFS index 9 is out of range
        assert!(parse_target(&Some("%9".into()), &panes).is_err());
    }

    #[test]
    fn target_next_wraps() {
        let panes = sample(); // active = pane 1 (idx 0)
        assert_eq!(parse_target(&Some(":+".into()), &panes).unwrap(), 2);
    }

    #[test]
    fn target_next_wraps_around() {
        let mut panes = sample();
        panes[0].active = false;
        panes[2].active = true; // pane 3 active (idx 2, last)
        assert_eq!(parse_target(&Some(":+".into()), &panes).unwrap(), 1);
    }

    #[test]
    fn target_prev_wraps() {
        let panes = sample(); // active = pane 1 (idx 0)
        assert_eq!(parse_target(&Some(":-".into()), &panes).unwrap(), 3);
    }

    #[test]
    fn target_unsupported_errors() {
        let panes = sample();
        assert!(parse_target(&Some("session:0.1".into()), &panes).is_err());
    }

    #[test]
    fn map_key_translates_names() {
        assert_eq!(map_key("Enter"), "\r");
        assert_eq!(map_key("Tab"), "\t");
        assert_eq!(map_key("Escape"), "\x1b");
        assert_eq!(map_key("Up"), "\x1b[A");
        assert_eq!(map_key("ls"), "ls");
        assert_eq!(map_key("-la"), "-la");
    }

    #[test]
    fn render_format_substitutes() {
        let p = pane(7, 2, true);
        let out =
            render_format("#{pane_id} #{pane_index} #{pane_active} #{pane_current_path}", &p);
        assert_eq!(out, "7 2 1 /home/u");
    }

    #[test]
    fn render_format_inactive() {
        let p = pane(7, 2, false);
        assert_eq!(render_format("#{pane_active}", &p), "0");
    }

    // ── GAP-6.6: CLI ↔ SocketRouter round-trip (cross-module seam) ──────────
    //
    // Unit tests above cover parse_target / map_key / render_format in
    // isolation. These drive `run()` (the private entry under `dispatch`) end
    // to end through a fake-GUI Unix socket that runs the REAL `SocketRouter`,
    // proving the CLI marshals the right method+params and the router emits the
    // right `IpcSideEffect`. Mirrors the `agent_e2e.rs` fake-socket pattern.
    #[cfg(unix)]
    mod round_trip {
        use super::*;
        use std::os::unix::net::UnixListener;
        use std::path::PathBuf;
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::{Arc, Mutex, OnceLock};
        use std::thread;
        use std::time::Duration;

        /// What the fake GUI observed: each (method, params) it dispatched and
        /// every side effect the router emitted.
        #[derive(Default)]
        struct Recorded {
            calls: Vec<(String, Value)>,
            effects: Vec<IpcSideEffect>,
        }

        fn unique_socket_path(label: &str) -> PathBuf {
            static COUNTER: OnceLock<AtomicU64> = OnceLock::new();
            let n = COUNTER
                .get_or_init(|| AtomicU64::new(0))
                .fetch_add(1, Ordering::Relaxed);
            let mut p = std::env::temp_dir();
            p.push(format!("pureterm_tmux_{label}_{n}_{}.sock", std::process::id()));
            let _ = std::fs::remove_file(&p);
            p
        }

        /// 2 tabs: tab 0 = panes {10(active), 11}; tab 1 = pane {20(active)}.
        /// `%0` resolves to the first pane (id 10); tab index 1 is valid.
        fn tmux_snap() -> AppSnapshot {
            AppSnapshot {
                window_count: 1,
                tab_titles: vec!["a".into(), "b".into()],
                tab_ids: vec![1, 2],
                pane_counts: vec![2, 1],
                active_tab_index: 0,
                version: "0.1.0".into(),
                ssh_profile_names: vec![],
                config_path: None,
                tab_cwds: vec![None, None],
                active_pane_ids: vec![10, 20],
                pane_ids: vec![vec![10, 11], vec![20]],
                ssh_pane_states: Vec::new(),
                pane_shell_contexts: std::collections::HashMap::new(),
                triggers: Vec::new(),
            }
        }

        /// Fake GUI: binds the socket, runs the real `SocketRouter` over every
        /// connection, records methods + side effects. Each `send_rpc` opens a
        /// fresh connection (the client half-closes its write side after the
        /// request, so `read_to_string` returns exactly one request). Polls
        /// non-blocking so the test can stop it once `run()` returns.
        #[allow(clippy::type_complexity)]
        fn spawn_fake_gui(
            path: PathBuf,
            snap: AppSnapshot,
        ) -> (
            Arc<Mutex<Recorded>>,
            Arc<AtomicBool>,
            thread::JoinHandle<()>,
        ) {
            let rec = Arc::new(Mutex::new(Recorded::default()));
            let stop = Arc::new(AtomicBool::new(false));
            let (rec_cl, stop_cl) = (Arc::clone(&rec), Arc::clone(&stop));
            let handle = thread::spawn(move || {
                let listener = match UnixListener::bind(&path) {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("fake_gui bind failed: {e}");
                        return;
                    }
                };
                let _ = listener.set_nonblocking(true);
                let router = SocketRouter::new();
                while !stop_cl.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                            let mut buf = String::new();
                            if Read::read_to_string(&mut stream, &mut buf).is_err() {
                                continue;
                            }
                            let req: Value = match serde_json::from_str(buf.trim()) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };
                            let id = req.get("id").and_then(|v| v.as_u64()).unwrap_or(1);
                            let method = req
                                .get("method")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let params = req.get("params").cloned();
                            let (resp, fx) = router.dispatch(id, &method, params, &snap);
                            {
                                let mut r = rec_cl.lock().unwrap();
                                r.calls
                                    .push((method, req.get("params").cloned().unwrap_or(Value::Null)));
                                if let Some(fx) = fx {
                                    r.effects.push(fx);
                                }
                            }
                            let mut line = serde_json::to_string(&resp).unwrap_or_default();
                            line.push('\n');
                            let _ = Write::write_all(&mut stream, line.as_bytes());
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
            });
            (rec, stop, handle)
        }

        /// `capture-pane -p` (no `--lines`) defaults `lines=50` (the DoD) and
        /// targets the resolved pane.
        #[test]
        fn capture_pane_round_trip_marshals_snapshot_with_lines_default() {
            let path = unique_socket_path("cap");
            let (rec, stop, handle) = spawn_fake_gui(path.clone(), tmux_snap());
            thread::sleep(Duration::from_millis(60)); // let the listener bind
            let cmd = TmuxSub::CapturePane {
                target: Some("%0".into()),
                print: true,
                start: None,
                end: None,
                lines: None,
            };
            let res = run(cmd, &Some(path));
            stop.store(true, Ordering::Relaxed);
            let _ = handle.join();
            assert!(res.is_ok(), "capture-pane round-trip failed: {res:?}");

            let calls = rec.lock().unwrap().calls.clone();
            let snap_call = calls
                .iter()
                .find(|(m, _)| m == "surface.snapshot")
                .expect("surface.snapshot was called");
            assert_eq!(snap_call.1["lines"], 50, "-p defaults to 50 lines (DoD)");
            assert_eq!(snap_call.1["pane_id"], 10, "%0 → first pane id");
        }

        /// `send-keys ping` marshals `surface.send_text` with the resolved pane
        /// and the (map_key-translated) text.
        #[test]
        fn send_keys_round_trip_marshals_target_and_text() {
            let path = unique_socket_path("sk");
            let (rec, stop, handle) = spawn_fake_gui(path.clone(), tmux_snap());
            thread::sleep(Duration::from_millis(60));
            let cmd = TmuxSub::SendKeys { target: Some("%0".into()), keys: vec!["ping".into()] };
            let res = run(cmd, &Some(path));
            stop.store(true, Ordering::Relaxed);
            let _ = handle.join();
            assert!(res.is_ok(), "{res:?}");

            let calls = rec.lock().unwrap().calls.clone();
            let st = calls
                .iter()
                .find(|(m, _)| m == "surface.send_text")
                .expect("surface.send_text was called");
            assert_eq!(st.1["pane_id"], 10);
            assert_eq!(st.1["text"], "ping");
        }

        /// `join-pane -s %0 -t 1` marshals `pane.attach` and the router emits
        /// `AttachPane { src_pane_id: 10, dst_tab_index: 1, Vertical }`.
        #[test]
        fn join_pane_round_trip_emits_attach_pane_effect() {
            let path = unique_socket_path("jp");
            let (rec, stop, handle) = spawn_fake_gui(path.clone(), tmux_snap());
            thread::sleep(Duration::from_millis(60));
            let cmd = TmuxSub::JoinPane {
                source: Some("%0".into()),
                target: Some("1".into()),
                horizontal: false,
                vertical: false,
            };
            let res = run(cmd, &Some(path));
            stop.store(true, Ordering::Relaxed);
            let _ = handle.join();
            assert!(res.is_ok(), "{res:?}");

            let guard = rec.lock().unwrap();
            let call = guard
                .calls
                .iter()
                .find(|(m, _)| m == "pane.attach")
                .expect("pane.attach was called");
            assert_eq!(call.1["src_pane_id"], 10);
            assert_eq!(call.1["dst_tab_index"], 1);
            let (src, dst, dir) = guard
                .effects
                .iter()
                .find_map(|fx| match fx {
                    IpcSideEffect::AttachPane { src_pane_id, dst_tab_index, direction } =>
                        Some((*src_pane_id, *dst_tab_index, *direction)),
                    _ => None,
                })
                .expect("AttachPane side effect emitted");
            assert_eq!(src, 10);
            assert_eq!(dst, 1);
            assert_eq!(dir, crate::split_tree::SplitDirection::Vertical);
        }
    }
}
