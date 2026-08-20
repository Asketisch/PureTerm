//! Agent-browser 子命令组（GAP-7.8）。
//!
//! 每个变体映射到一个 `browser.<verb>` JSON-RPC 方法（Phase D），外加
//! Phase C 代理绑定操作 `bind-tunnel` / `clear-tunnel`。`--pane-id` 可选
//!（缺省指向首个 web pane）；所有 op 都把 pane_id 透传给 GUI。

use clap::Subcommand;
use serde_json::{json, Value};

#[derive(Subcommand)]
pub enum BrowserSub {
    /// 让窗格跳转到 `url`。
    Navigate {
        url: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// 点击匹配 CSS `selector` 的元素。
    Click {
        selector: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// 向匹配 `selector` 的元素输入 `text`。
    Type {
        selector: String,
        text: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// 从 `selector` 中提取文本(或某个 `attribute`)。
    Extract {
        selector: String,
        #[arg(long)]
        attribute: Option<String>,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// 按 `--x`/`--y` 像素滚动,或把 `--selector` 滚动到可见区域。
    Scroll {
        #[arg(long, default_value = "0")]
        x: i64,
        #[arg(long, default_value = "0")]
        y: i64,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// 等待 `selector` 出现(默认 5 秒,可用 `--timeout-ms` 覆盖)。
    Wait {
        selector: String,
        #[arg(long)]
        timeout_ms: Option<u64>,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// 打印窗格的当前 URL。
    GetUrl {
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// 打印窗格的页面标题。
    GetTitle {
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// 原生 WKWebView 快照(PNG data URL);仅 macOS。
    Screenshot {
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Phase C:让窗格流量经守护进程 SSH 隧道转发(重建
    /// webview 并注入 SOCKS5 代理;可访问仅网关可见的主机)。
    BindTunnel {
        /// 来自 `ssh tunnel-list` 的隧道 id。
        tunnel_id: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Phase C:解除隧道绑定,恢复直连。
    ClearTunnel {
        #[arg(long)]
        pane_id: Option<u64>,
    },
}

impl BrowserSub {
    pub(crate) fn method_and_params(&self) -> (&'static str, Option<Value>) {
        // 先放各操作独有参数;`pane_id` 最后统一叠加,使每个
        // 操作(即使无参)都能指定目标窗格。
        let (method, mut params): (&'static str, Value) = match self {
            BrowserSub::Navigate { url, .. } => ("browser.navigate", json!({ "url": url })),
            BrowserSub::Click { selector, .. } => ("browser.click", json!({ "selector": selector })),
            BrowserSub::Type { selector, text, .. } => {
                ("browser.type", json!({ "selector": selector, "text": text }))
            }
            BrowserSub::Extract { selector, attribute, .. } => {
                let mut o = serde_json::Map::new();
                o.insert("selector".into(), json!(selector));
                if let Some(a) = attribute {
                    o.insert("attribute".into(), json!(a));
                }
                ("browser.extract", Value::Object(o))
            }
            BrowserSub::Scroll { x, y, selector, .. } => {
                let mut o = serde_json::Map::new();
                o.insert("x".into(), json!(x));
                o.insert("y".into(), json!(y));
                if let Some(s) = selector {
                    o.insert("selector".into(), json!(s));
                }
                ("browser.scroll", Value::Object(o))
            }
            BrowserSub::Wait { selector, timeout_ms, .. } => {
                let mut o = serde_json::Map::new();
                o.insert("selector".into(), json!(selector));
                if let Some(t) = timeout_ms {
                    o.insert("timeout_ms".into(), json!(t));
                }
                ("browser.wait", Value::Object(o))
            }
            BrowserSub::GetUrl { .. } => ("browser.get_url", json!({})),
            BrowserSub::GetTitle { .. } => ("browser.get_title", json!({})),
            BrowserSub::Screenshot { .. } => ("browser.screenshot", json!({})),
            BrowserSub::BindTunnel { tunnel_id, .. } => {
                ("browser.bind_tunnel", json!({ "tunnel_id": tunnel_id }))
            }
            BrowserSub::ClearTunnel { .. } => ("browser.clear_tunnel", json!({})),
        };
        if let Some(id) = self.pane_id() {
            if let Some(obj) = params.as_object_mut() {
                obj.insert("pane_id".into(), json!(id));
            }
        }
        (method, Some(params))
    }

    /// 每个变体都带有可选的 `pane_id`;集中在此处提取,
    /// 让上面的 match 只专注各操作的参数。
    fn pane_id(&self) -> Option<u64> {
        match self {
            BrowserSub::Navigate { pane_id, .. }
            | BrowserSub::Click { pane_id, .. }
            | BrowserSub::Type { pane_id, .. }
            | BrowserSub::Extract { pane_id, .. }
            | BrowserSub::Scroll { pane_id, .. }
            | BrowserSub::Wait { pane_id, .. }
            | BrowserSub::GetUrl { pane_id }
            | BrowserSub::GetTitle { pane_id }
            | BrowserSub::Screenshot { pane_id }
            | BrowserSub::BindTunnel { pane_id, .. }
            | BrowserSub::ClearTunnel { pane_id } => *pane_id,
        }
    }
}
