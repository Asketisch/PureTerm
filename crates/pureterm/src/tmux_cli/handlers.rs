//! 各 tmux 子命令的实现 —— 一个 `run_*` 函数对应 `TmuxSub` 的一个变体。
//!
//! 每个 handler 拿到 `socket`(可选的自定义 Socket API 路径)与该变体的
//! 命令行参数,通过 [`super::target`] 解析 target、通过 [`super::format`]
//! 翻译键名 / 渲染格式串,最终调用底层 RPC。它们都是 `pub(super)`:只被
//! [`super::sub::run`] 分发与 [`super::tests`] 的 round-trip e2e 使用。
//!
//! 分阶段标注与原文件一致:phase 1(capture/list/send_keys/split/kill/select)
//! → phase 2(布局操作:resize/swap/break/select-layout)→ phase 3(join-pane)
//! → phase 4(pipe-pane/wait-for,由守护进程支撑)。

use std::path::PathBuf;

use serde_json::{json, Value};

use super::format::{render_format, map_key, DEFAULT_LIST_FORMAT};
use super::target::{parse_target, resolve_optional_target, resolve_panes, rpc};

// ── 命令实现(phase 1) ────────────────────────────────────────────────────────

pub(super) fn run_capture(
    socket: &Option<PathBuf>,
    target: Option<String>,
    print: bool,
    start: Option<String>,
    _end: Option<String>,
    lines: Option<usize>,
) -> Result<(), String> {
    let panes = resolve_panes(socket)?;
    let pane_id = parse_target(&target, &panes)?;
    // `-S 0` ⇒ 仅可视视口;否则为完整回滚。`-p` ⇒ 保留
    // 最后 50 行(DoD 要求);显式 `--lines N` 可覆盖。
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
    // tmux 不带 `-p` 的 `capture-pane` 会存入粘贴缓冲;我们没有
    // 该缓冲,因此总是输出到 stdout,并在缺少 `-p` 时说明。
    if !print {
        eprintln!(
            "[pureterm tmux] (capture-pane without -p: written to stdout; paste buffer unsupported)"
        );
    }
    print!("{text}");
    Ok(())
}

pub(super) fn run_list(socket: &Option<PathBuf>, format: Option<String>) -> Result<(), String> {
    let panes = resolve_panes(socket)?;
    let fmt = format.unwrap_or_else(|| DEFAULT_LIST_FORMAT.to_string());
    for p in &panes {
        println!("{}", render_format(&fmt, p));
    }
    Ok(())
}

pub(super) fn run_send_keys(
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

pub(super) fn run_split(socket: &Option<PathBuf>, horizontal: bool) -> Result<(), String> {
    // tmux 默认为 `-v`(上下堆叠);`-h` = 并排。PureTerm 的
    // `pane.split` "horizontal" = 并排,"vertical" = 上下堆叠,
    // 因此映射是直接的。
    let dir = if horizontal { "horizontal" } else { "vertical" };
    rpc(socket, "pane.split", Some(json!({ "direction": dir })))?;
    Ok(())
}

pub(super) fn run_kill(socket: &Option<PathBuf>, target: Option<String>) -> Result<(), String> {
    // Phase 1:`pane.close` 只关闭活动窗格。定向关闭
    // 在 phase 2(pane.close pane_id)实现;目前警告并关闭活动窗格。
    if target.is_some() {
        eprintln!(
            "[pureterm tmux] kill-pane -t needs pane.close target support (phase 2); closing active"
        );
    }
    rpc(socket, "pane.close", None)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_select(
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

// ── 命令实现(phase 2:布局操作) ──────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn run_resize(
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
    // 优先绝对 -x/-y;其次方向标志;最后默认微调。
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

// `down` 属于有文档的 CLI 面(tmux `swap-pane -D`),但现在
// 逻辑不读取它(仅 `up` 决定探测方向);保留
// 以保持原签名。
#[allow(unused_variables)]
pub(super) fn run_swap(
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
        // -U/-D:把活动窗格与其前 / 后邻居交换(DFS 顺序)。
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

pub(super) fn run_break(socket: &Option<PathBuf>, source: Option<String>) -> Result<(), String> {
    let pane_id = resolve_optional_target(socket, &source)?;
    let mut params = serde_json::Map::new();
    if let Some(id) = pane_id {
        params.insert("pane_id".into(), json!(id));
    }
    rpc(socket, "pane.detach", Some(Value::Object(params)))?;
    Ok(())
}

pub(super) fn run_join(
    socket: &Option<PathBuf>,
    source: Option<String>,
    target: Option<String>,
    horizontal: bool,
) -> Result<(), String> {
    let panes = resolve_panes(socket)?;
    let active_id = panes.iter().find(|p| p.active).map(|p| p.pane_id);
    let active_tab = panes.iter().find(|p| p.active).map(|p| p.tab);
    // -s 指定源(默认:活动窗格)。
    let src = match &source {
        Some(_) => parse_target(&source, &panes)?,
        None => active_id.ok_or("no active pane")?,
    };
    // -t 目标:标签页索引,或 `%N`(该窗格所在标签页);默认活动标签页。
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

pub(super) fn run_pipe(
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

/// GAP-6.6:`wait-for` 直接连接守护进程并阻塞 —— `wait` / `lock`
/// 挂起直到被信号唤醒。我们绕过 `daemon_client::rpc`(5 秒读
/// 超时),改用原始阻塞 Unix socket,使挂起无界,与
/// tmux 一致。
pub(super) fn run_wait_for(channel: String, signal: bool, lock: bool, unlock: bool) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Write};

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
    let mut stream = crate::local_socket::connect(&socket).map_err(|e| format!("connect: {e}"))?;
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
