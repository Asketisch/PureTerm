//! tmux 键名翻译与 `-F` 格式串渲染 —— 纯函数,无 I/O。
//!
//! - [`map_key`] 把 tmux 的键名(`Enter` / `BSpace` / `Up` ...)翻译成对应字节
//!   序列;未知名称原样返回,由 [`super::handlers::run_send_keys`] 拼接后发往 PTY。
//! - [`render_format`] 把一个 tmux `-F` 格式串(`#{pane_id}` 这类变量)渲染到
//!   单个 [`super::target::PaneInfo`] 上;由 [`super::handlers::run_list`] 使用。
//! - [`DEFAULT_LIST_FORMAT`] 是 `list-panes` 不带 `-F` 时的默认输出格式。

use super::target::PaneInfo;

/// Translate a tmux key name to bytes; unknown names are sent literally.
pub(super) fn map_key(k: &str) -> String {
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
pub(super) fn render_format(fmt: &str, p: &PaneInfo) -> String {
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

pub(super) const DEFAULT_LIST_FORMAT: &str =
    "#{pane_id}: [#{pane_index}] #{tab_index}:#{tab_title} #{pane_active} #{pane_current_path}";
