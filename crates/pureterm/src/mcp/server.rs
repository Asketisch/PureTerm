//! GAP-7.10: the MCP stdio server (`pureterm mcp serve`), feature-gated behind
//! `mcp-server`. Each tool is a thin adapter over the existing GUI JSON-RPC
//! Socket API — it builds the params object and forwards via `rpc()`, which
//! wraps the **blocking** `ipc::client::send_rpc` in `spawn_blocking` so the
//! rmcp async worker thread never blocks on socket I/O.
//!
//! Logging goes to **stderr only** — stdout is the MCP transport.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use serde_json::{json, Value};

use crate::app_config::McpConfig;
use crate::ipc::client::send_rpc;

use super::enabled_tool_names;

const INSTRUCTIONS: &str = "\
PureTerm terminal surface. Workflow: call list_surfaces to enumerate panes \
(returned pane_id targets a specific pane; omit it for the active pane), \
capture_pane to read a pane's current output, send_text to type into a pane \
(use the literal two characters \\r — backslash-r — for Enter). Pair \
send_text with capture_pane to run a command and read its result.";

/// Bridge an MCP tool call to the GUI Socket API. `send_rpc` is blocking
/// (write_all + shutdown + read_line over the Unix socket), so it MUST run off
/// the rmcp async worker — `spawn_blocking` parks it on the blocking pool.
/// `socket_override = None` honors `PURETERM_SOCKET` / default discovery (the
/// agent-in-pane case: the spawned `pureterm mcp serve` inherits the env var
/// and nails itself to the right GUI instance).
async fn rpc(method: &str, params: Option<Value>, socket: Option<PathBuf>) -> Result<Option<Value>, String> {
    let m = method.to_string();
    tokio::task::spawn_blocking(move || send_rpc(&m, params, socket))
        .await
        .map_err(|e| format!("rpc task failed: {e}"))?
}

/// Stringify a Socket-API result for return to the MCP client. `Ok(None)` (a
/// method with no result field) becomes a faithful JSON `null` rather than an
/// empty string, so the agent can tell "no payload" from "empty payload".
fn to_text(res: Option<Value>) -> String {
    serde_json::to_string(&res).unwrap_or_else(|_| "null".into())
}

pub struct PureTermServer {
    cfg: McpConfig,
    socket: Option<PathBuf>,
}

// ── Tool parameter schemas ───────────────────────────────────────────────
// `#[serde(default)]` makes optional fields truly optional (the agent may omit
// them); a required field like `SendTextParams::text` that's missing fails
// deserialization and rmcp returns a structured error.

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct CapturePaneParams {
    /// Target pane id (default: active pane).
    #[serde(default)]
    pane_id: Option<u64>,
    /// Capture only the visible viewport (default: full scrollback, capped by
    /// `[mcp].default_capture_lines`).
    #[serde(default)]
    visible_only: Option<bool>,
    /// Keep only the last N lines (default: `[mcp].default_capture_lines`).
    #[serde(default)]
    lines: Option<usize>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SendTextParams {
    /// Text to type into the pane. Use the two characters \\r for Enter.
    text: String,
    /// Target pane id (default: active pane).
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct PaneIdParams {
    /// Target pane id (default: active pane).
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SplitPaneParams {
    /// Split direction: "horizontal" (side-by-side) or "vertical" (stacked).
    #[serde(default)]
    direction: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct FocusPaneParams {
    /// "prev" or "next" (default: "next").
    #[serde(default)]
    direction: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserNavigateParams {
    /// URL to navigate the web pane to.
    url: String,
    /// Target web pane id (default: first web pane).
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserExtractParams {
    /// CSS selector to match.
    selector: String,
    /// Element attribute to extract (default: visible text content).
    #[serde(default)]
    attribute: Option<String>,
    /// Target web pane id (default: first web pane).
    #[serde(default)]
    pane_id: Option<u64>,
}

#[tool_router]
impl PureTermServer {
    #[tool(description = "Read a terminal pane's buffer as text. Returns JSON {pane_id, text, lines, truncated}. Defaults to the active pane; set visible_only=true for just the viewport.")]
    async fn capture_pane(&self, params: Parameters<CapturePaneParams>) -> Result<String, String> {
        let p = params.0;
        let mut obj = serde_json::Map::new();
        if let Some(id) = p.pane_id {
            obj.insert("pane_id".into(), json!(id));
        }
        if let Some(v) = p.visible_only {
            obj.insert("visible_only".into(), json!(v));
        }
        // Apply the config default capture cap when the caller didn't bound it,
        // so an unbounded full-scrollback capture only happens when the agent
        // explicitly opts in via a large `lines`.
        obj.insert(
            "lines".into(),
            json!(p.lines.unwrap_or(self.cfg.default_capture_lines)),
        );
        let res = rpc("surface.snapshot", Some(Value::Object(obj)), self.socket.clone()).await?;
        Ok(to_text(res))
    }

    #[tool(description = "Type text into a pane's PTY (fire-and-forget). Use the two characters \\r for Enter. Defaults to the active pane. Pair with capture_pane to read the result.")]
    async fn send_text(&self, params: Parameters<SendTextParams>) -> Result<String, String> {
        let p = params.0;
        let mut obj = serde_json::Map::new();
        obj.insert("text".into(), json!(p.text));
        if let Some(id) = p.pane_id {
            obj.insert("pane_id".into(), json!(id));
        }
        let res = rpc("surface.send_text", Some(Value::Object(obj)), self.socket.clone()).await?;
        Ok(to_text(res))
    }

    #[tool(description = "List every terminal pane with its id, tab, title, and cwd. Use a returned pane_id with capture_pane/send_text.")]
    async fn list_surfaces(&self) -> Result<String, String> {
        let res = rpc("surface.list", None, self.socket.clone()).await?;
        Ok(to_text(res))
    }

    #[tool(description = "Open a new tab. Returns the new tab/pane id.")]
    async fn new_tab(&self) -> Result<String, String> {
        let res = rpc("tab.new", None, self.socket.clone()).await?;
        Ok(to_text(res))
    }

    #[tool(description = "Split the active pane. direction: \"horizontal\" | \"vertical\" (default horizontal). Returns the new pane id.")]
    async fn split_pane(&self, params: Parameters<SplitPaneParams>) -> Result<String, String> {
        let dir = params
            .0
            .direction
            .unwrap_or_else(|| "horizontal".to_string());
        let res = rpc(
            "pane.split",
            Some(json!({ "direction": dir })),
            self.socket.clone(),
        )
        .await?;
        Ok(to_text(res))
    }

    #[tool(description = "Move focus to an adjacent pane. direction: \"prev\" | \"next\" (default next).")]
    async fn focus_pane(&self, params: Parameters<FocusPaneParams>) -> Result<String, String> {
        let dir = params.0.direction.unwrap_or_else(|| "next".to_string());
        let res = rpc(
            "pane.focus",
            Some(json!({ "direction": dir })),
            self.socket.clone(),
        )
        .await?;
        Ok(to_text(res))
    }

    #[tool(description = "Navigate an embedded web pane to a URL. pane_id from pane list; omit for the first web pane.")]
    async fn browser_navigate(
        &self,
        params: Parameters<BrowserNavigateParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let mut obj = serde_json::Map::new();
        obj.insert("url".into(), json!(p.url));
        if let Some(id) = p.pane_id {
            obj.insert("pane_id".into(), json!(id));
        }
        let res = rpc("browser.navigate", Some(Value::Object(obj)), self.socket.clone()).await?;
        Ok(to_text(res))
    }

    #[tool(description = "Extract text or an attribute from a web pane via CSS selector. selector required; attribute optional (default: visible text). pane_id optional (default: first web pane).")]
    async fn browser_extract(
        &self,
        params: Parameters<BrowserExtractParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let mut obj = serde_json::Map::new();
        obj.insert("selector".into(), json!(p.selector));
        if let Some(a) = p.attribute {
            obj.insert("attribute".into(), json!(a));
        }
        if let Some(id) = p.pane_id {
            obj.insert("pane_id".into(), json!(id));
        }
        let res = rpc("browser.extract", Some(Value::Object(obj)), self.socket.clone()).await?;
        Ok(to_text(res))
    }
}

#[tool_handler]
impl ServerHandler for PureTermServer {
    fn get_info(&self) -> ServerInfo {
        // `InitializeResult` is `#[non_exhaustive]`, so use its constructor
        // (not a struct literal). `from_build_env()` would read
        // CARGO_CRATE_NAME inside rmcp's source → "rmcp"; set the name
        // explicitly so the client sees "pureterm" + this crate's version.
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(INSTRUCTIONS)
            .with_server_info(Implementation::new(
                "pureterm",
                env!("CARGO_PKG_VERSION"),
            ))
    }
}

/// Run the MCP stdio server until stdin closes (the client disconnects) or the
/// transport errors. Logs to stderr only. Builds its own current-thread tokio
/// runtime: tool calls are agent-driven (low frequency), and `rpc()` offloads
/// the blocking socket work to the blocking pool.
pub fn run_server(cfg: McpConfig, socket: Option<PathBuf>) {
    // stdout is the MCP transport — only stderr is safe for diagnostics.
    let _ = env_logger::try_init();

    eprintln!(
        "pureterm mcp: serving {} tool(s): [{}]",
        enabled_tool_names(&cfg).len(),
        enabled_tool_names(&cfg).join(", ")
    );

    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("pureterm mcp: failed to build runtime: {e}");
            std::process::exit(1);
        }
    };

    runtime.block_on(async move {
        let server = PureTermServer { cfg, socket };
        match server
            .serve((tokio::io::stdin(), tokio::io::stdout()))
            .await
        {
            Ok(running) => {
                let _ = running.waiting().await;
            }
            Err(e) => {
                eprintln!("pureterm mcp: serve failed: {e}");
            }
        }
    });
}
