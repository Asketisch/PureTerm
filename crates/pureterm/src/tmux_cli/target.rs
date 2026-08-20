//! Target 解析与底层 Socket RPC helper —— tmux `-t` / `-s` 语义的核心。
//!
//! 这里的函数是 [`super::handlers`] 与 [`super::format`] 的共享基础设施:
//! - [`PaneInfo`] 是 `pane.list` 返回的每一行(摊平所有 tab 的 DFS 顺序)。
//! - [`rpc`] 把一个 JSON-RPC 方法发到 Socket API,展开 `Result` 字段。
//! - [`resolve_panes`] 拉一次 `pane.list` 并反序列化成 `Vec<PaneInfo>`。
//! - [`parse_target`] 把 tmux 的 `-t` 文法(`%N` / `:+` / `:-` / 省略=active)
//!   解析成具体的 pane id。完整文法见根模块文档。
//! - [`resolve_optional_target`] 是 `parse_target` 的「None ⇒ None」包装,给
//!   `-s`/`-t` 这类可选 target 的命令用。

use std::path::PathBuf;

use serde_json::Value;

use crate::ipc::client::send_rpc;

/// `pane.list` 的一行,摊平所有 tab(DFS 顺序)。
///
/// 字段以 `pub(crate)` 暴露:handlers / format / tests 都需要直接读 pane_id /
/// active / tab / cwd 等;保持在 crate 内可见即可,不对外稳定。
#[derive(Debug, Clone)]
pub(crate) struct PaneInfo {
    pub(crate) pane_id: u64,
    pub(crate) pane_index: usize,
    pub(crate) tab: usize,
    pub(crate) tab_title: String,
    pub(crate) active: bool,
    pub(crate) cwd: Option<String>,
}

/// 调用一个 Socket API 方法并展开结果值(传输错误 /
/// JSON-RPC 错误或缺 result 字段时报错)。
pub(super) fn rpc(socket: &Option<PathBuf>, method: &str, params: Option<Value>) -> Result<Value, String> {
    send_rpc(method, params, socket.clone())?.ok_or_else(|| format!("{method}: empty result"))
}

pub(super) fn resolve_panes(socket: &Option<PathBuf>) -> Result<Vec<PaneInfo>, String> {
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

/// 把 tmux 的 `-t` target 解析成 pane id。支持的
/// 文法见模块文档。
pub(super) fn parse_target(target: &Option<String>, panes: &[PaneInfo]) -> Result<u64, String> {
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
            // 精确匹配 pane-id 优先;否则回退到 DFS 索引,
            // 使 `%0` 总是解析到第一个窗格(PureTerm 的 id 从 1 起)。
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

/// 把可选的 `-t`/`-s` target 解析成 pane id(None ⇒ 活动窗格/未设置)。
pub(super) fn resolve_optional_target(
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
