//! GAP-3.5: 内置 Browser 面板(WebView)。
//!
//! 让一个 Pane 容纳 WebView —— 终端之外的第二种 pane 内容。详见 plan
//! 「GAP-3.5 + GAP-5.2 + GAP-7.8: Agent Browser 链路」。
//!
//! 模块布局:
//! - `context` — `BrowserContext`,Web pane 的内容载体(持 wry `WebView`)
//! - `platform` — 主线程取 content NSView(macOS objc2 walk;R1)
//! - `commands` — GAP-7.8 agent-browser 执行层(`browser.*` ops)
//! - `wait` — `browser.wait` 的轮询式等待(非阻塞页面 JS 线程)
//!
//! 几何:pane 的像素矩形直接来自 `split_tree::compute_layout`(终端 grid
//! resize 用同一 layout),`iced_app::reposition_web_panes` 据此 `set_frame`
//! 各 webview。无需 iced Widget 报告 bounds 的 side-table —— split_tree 即
//! 真值。

pub mod commands;
pub mod context;
/// GAP-3.5: 取 iced 窗口的原生视图句柄(macOS objc2 walk;R1)。
pub mod platform;
/// GAP-7.8: `browser.wait` 的轮询式等待。
pub mod wait;
/// GAP-3.5 / GAP-7.8: 共享的 URL 规范化核心(`loadable_url`)。
pub mod url;

pub use commands::{BrowserOp, BrowserReply, BrowserResolved};
pub use context::{BrowserContext, BrowserContextId};
pub use wait::WaitHandle;
