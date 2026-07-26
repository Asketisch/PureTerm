//! Agent-browser 子命令组（GAP-7.8）。
//!
//! 每个变体映射到一个 `browser.<verb>` JSON-RPC 方法（Phase D），外加
//! Phase C 代理绑定操作 `bind-tunnel` / `clear-tunnel`。`--pane-id` 可选
//!（缺省指向首个 web pane）；所有 op 都把 pane_id 透传给 GUI。

use clap::Subcommand;
use serde_json::{json, Value};

#[derive(Subcommand)]
pub enum BrowserSub {
    /// Navigate the pane to `url`.
    Navigate {
        url: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Click the element matching a CSS `selector`.
    Click {
        selector: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Type `text` into the element matching `selector`.
    Type {
        selector: String,
        text: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Extract text (or an `attribute`) from `selector`.
    Extract {
        selector: String,
        #[arg(long)]
        attribute: Option<String>,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Scroll by `--x`/`--y` pixels, or scroll `--selector` into view.
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
    /// Wait until `selector` appears (default 5s, override with `--timeout-ms`).
    Wait {
        selector: String,
        #[arg(long)]
        timeout_ms: Option<u64>,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Print the pane's current URL.
    GetUrl {
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Print the pane's page title.
    GetTitle {
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Native WKWebView snapshot (PNG data URL); macOS-only.
    Screenshot {
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Phase C: route the pane's traffic through a daemon SSH tunnel (rebuilds
    /// the webview with a SOCKS5 proxy; reaches hosts only the gateway sees).
    BindTunnel {
        /// Tunnel id from `ssh tunnel-list`.
        tunnel_id: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Phase C: drop the tunnel binding, restoring direct connection.
    ClearTunnel {
        #[arg(long)]
        pane_id: Option<u64>,
    },
}

impl BrowserSub {
    pub(crate) fn method_and_params(&self) -> (&'static str, Option<Value>) {
        // Op-specific params first; `pane_id` is layered on at the end so every
        // op (even the no-arg ones) can target a pane.
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

    /// Every variant carries an optional `pane_id`; collect it in one place so
    /// the match above stays focused on op params.
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
