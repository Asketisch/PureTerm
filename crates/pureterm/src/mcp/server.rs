//! GAP-7.10: the MCP stdio server (`pureterm mcp serve`), feature-gated behind
//! `mcp-server`. Each tool is a thin adapter over the existing GUI JSON-RPC
//! Socket API — it builds the params object and forwards via `rpc()`, which
//! wraps the **blocking** `ipc::client::send_rpc` in `spawn_blocking` so the
//! rmcp async worker thread never blocks on socket I/O.
//!
//! Logging goes to **stderr only** — stdout is the MCP transport.

use std::path::PathBuf;
use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use serde_json::{json, Value};

use crate::app_config::McpConfig;
use crate::feed_client::{request_approval, FeedApprovalRequest};
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

/// How long `desktop_screenshot`'s Feed approval prompt waits for a human
/// choice before failing closed. Matches the GUI Feed panel's 120s auto-Deny
/// soft timeout (`feed_panel.rs`) so both sides time out together.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

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
struct GetShellContextParams {
    /// Target pane id (default: active pane).
    #[serde(default)]
    pane_id: Option<u64>,
    /// Cap the recent_commands list to the last N (default: 10).
    #[serde(default)]
    recent: Option<u64>,
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

// GAP-7.8: the remaining agent-browser ops. `pane_id` is resolved GUI-side
// (handle_browser_request picks the first web pane when omitted), so each tool
// just forwards its op-specific params — no per-op pane resolution here.

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserClickParams {
    /// CSS selector of the element to click.
    selector: String,
    /// Target web pane id (default: first web pane).
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserTypeParams {
    /// CSS selector of the input/element to type into.
    selector: String,
    /// Text to type into the element.
    text: String,
    /// Target web pane id (default: first web pane).
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserScrollParams {
    /// Horizontal scroll delta in pixels (default 0; ignored when `selector` is set).
    #[serde(default)]
    x: Option<i64>,
    /// Vertical scroll delta in pixels (default 0; ignored when `selector` is set).
    #[serde(default)]
    y: Option<i64>,
    /// CSS selector to scroll into view (when set, overrides x/y).
    #[serde(default)]
    selector: Option<String>,
    /// Target web pane id (default: first web pane).
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserWaitParams {
    /// CSS selector to wait for.
    selector: String,
    /// Timeout in ms (default 5000). Blocks up to ~6.5s before timing out.
    #[serde(default)]
    timeout_ms: Option<u64>,
    /// Target web pane id (default: first web pane).
    #[serde(default)]
    pane_id: Option<u64>,
}

/// Shared params for pane-id-only browser ops (get_url / get_title / screenshot).
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserPaneIdParams {
    /// Target web pane id (default: first web pane).
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SearchCodeParams {
    /// Keyword query (FTS5 prefix tokens).
    query: String,
    /// Max hits (default 20).
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct GetFileContextParams {
    /// Workspace-relative or absolute path.
    path: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct FindSymbolParams {
    /// Symbol name substring (e.g. function or class name).
    name: String,
    /// Max hits (default 20).
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct OpenUrlParams {
    /// A `pureterm://` URL (tab/workspace/ssh/command actions).
    url: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct DesktopClickParams {
    x: i32,
    y: i32,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct DesktopTypeParams {
    text: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct DesktopKeyParams {
    key: String,
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

    #[tool(description = "Return the shell context for a pane: cwd, last_command {text, exit_code, duration_us}, and recent_commands. Defaults to the active pane. Requires shell integration (zsh/bash); non-integrated panes (SSH/fish) return integrated=false.")]
    async fn get_shell_context(
        &self,
        params: Parameters<GetShellContextParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let mut obj = serde_json::Map::new();
        if let Some(id) = p.pane_id {
            obj.insert("pane_id".into(), json!(id));
        }
        if let Some(n) = p.recent {
            obj.insert("recent".into(), json!(n));
        }
        let res = rpc("shell.context", Some(Value::Object(obj)), self.socket.clone()).await?;
        Ok(to_text(res))
    }

    #[tool(description = "List configured [[triggers]] rules: name, regex pattern, and action (highlight/notify/send_text/exec). Does not return send_text passwords.")]
    async fn trigger_list(&self) -> Result<String, String> {
        let res = rpc("trigger.list", None, self.socket.clone()).await?;
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

    #[tool(description = "Click an element in a web pane by CSS selector. Returns {ok, clicked, selector} or {ok:false, error:\"element not found\"}. pane_id optional (default: first web pane).")]
    async fn browser_click(
        &self,
        params: Parameters<BrowserClickParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let mut obj = serde_json::Map::new();
        obj.insert("selector".into(), json!(p.selector));
        if let Some(id) = p.pane_id {
            obj.insert("pane_id".into(), json!(id));
        }
        let res = rpc("browser.click", Some(Value::Object(obj)), self.socket.clone()).await?;
        Ok(to_text(res))
    }

    #[tool(description = "Type text into an input/contenteditable element in a web pane by CSS selector. Fires input+change. pane_id optional (default: first web pane).")]
    async fn browser_type(
        &self,
        params: Parameters<BrowserTypeParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let mut obj = serde_json::Map::new();
        obj.insert("selector".into(), json!(p.selector));
        obj.insert("text".into(), json!(p.text));
        if let Some(id) = p.pane_id {
            obj.insert("pane_id".into(), json!(id));
        }
        let res = rpc("browser.type", Some(Value::Object(obj)), self.socket.clone()).await?;
        Ok(to_text(res))
    }

    #[tool(description = "Scroll a web pane: by (x, y) pixel deltas, or scroll a CSS-selected element into view (selector overrides x/y). pane_id optional (default: first web pane).")]
    async fn browser_scroll(
        &self,
        params: Parameters<BrowserScrollParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let mut obj = serde_json::Map::new();
        if let Some(sel) = p.selector {
            obj.insert("selector".into(), json!(sel));
        } else {
            // Match the op's event_payload shape: absent x/y default to 0.
            obj.insert("x".into(), json!(p.x.unwrap_or(0)));
            obj.insert("y".into(), json!(p.y.unwrap_or(0)));
        }
        if let Some(id) = p.pane_id {
            obj.insert("pane_id".into(), json!(id));
        }
        let res = rpc("browser.scroll", Some(Value::Object(obj)), self.socket.clone()).await?;
        Ok(to_text(res))
    }

    #[tool(description = "Wait for a CSS-selected element to appear in a web pane. Blocks until found or timeout_ms (default 5000) elapses. pane_id optional (default: first web pane).")]
    async fn browser_wait(
        &self,
        params: Parameters<BrowserWaitParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let mut obj = serde_json::Map::new();
        obj.insert("selector".into(), json!(p.selector));
        if let Some(t) = p.timeout_ms {
            obj.insert("timeout_ms".into(), json!(t));
        }
        if let Some(id) = p.pane_id {
            obj.insert("pane_id".into(), json!(id));
        }
        let res = rpc("browser.wait", Some(Value::Object(obj)), self.socket.clone()).await?;
        Ok(to_text(res))
    }

    #[tool(description = "Get the current URL of a web pane (location.href). pane_id optional (default: first web pane).")]
    async fn browser_get_url(
        &self,
        params: Parameters<BrowserPaneIdParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let mut obj = serde_json::Map::new();
        if let Some(id) = p.pane_id {
            obj.insert("pane_id".into(), json!(id));
        }
        let res = rpc("browser.get_url", Some(Value::Object(obj)), self.socket.clone()).await?;
        Ok(to_text(res))
    }

    #[tool(description = "Get the page title of a web pane (document.title). pane_id optional (default: first web pane).")]
    async fn browser_get_title(
        &self,
        params: Parameters<BrowserPaneIdParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let mut obj = serde_json::Map::new();
        if let Some(id) = p.pane_id {
            obj.insert("pane_id".into(), json!(id));
        }
        let res = rpc("browser.get_title", Some(Value::Object(obj)), self.socket.clone()).await?;
        Ok(to_text(res))
    }

    #[tool(description = "Capture a PNG screenshot of a web pane. Returns {ok:true, screenshot:\"data:image/png;base64,...\"} — a large payload. macOS-only. pane_id optional (default: first web pane).")]
    async fn browser_screenshot(
        &self,
        params: Parameters<BrowserPaneIdParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let mut obj = serde_json::Map::new();
        if let Some(id) = p.pane_id {
            obj.insert("pane_id".into(), json!(id));
        }
        let res = rpc("browser.screenshot", Some(Value::Object(obj)), self.socket.clone()).await?;
        Ok(to_text(res))
    }

    #[tool(description = "Keyword search the indexed workspace (Rust/Python/JS chunks). Uses SQLite FTS; semantic embeddings TODO. Root: PURETERM_CODEINDEX_ROOT or process cwd. Returns JSON array of hits.")]
    async fn search_code(&self, params: Parameters<SearchCodeParams>) -> Result<String, String> {
        let p = params.0;
        let limit = p.limit.unwrap_or(20);
        let query = p.query;
        tokio::task::spawn_blocking(move || {
            crate::codeindex::with_index(|idx| {
                let hits = idx.search_code(&query, limit).map_err(|e| e.to_string())?;
                serde_json::to_string(&hits).map_err(|e| e.to_string())
            })
        })
        .await
        .map_err(|e| format!("search_code task failed: {e}"))?
    }

    #[tool(description = "Return Tree-sitter semantic chunks for a file from the codebase index. path is workspace-relative or absolute. Returns JSON {file_path, language, chunks}.")]
    async fn get_file_context(
        &self,
        params: Parameters<GetFileContextParams>,
    ) -> Result<String, String> {
        let path = params.0.path;
        tokio::task::spawn_blocking(move || {
            crate::codeindex::with_index(|idx| {
                let ctx = idx.get_file_context(&path).map_err(|e| e.to_string())?;
                serde_json::to_string(&ctx).map_err(|e| e.to_string())
            })
        })
        .await
        .map_err(|e| format!("get_file_context task failed: {e}"))?
    }

    #[tool(description = "Find symbol definitions by name (substring match) in the indexed workspace. Returns JSON array of {file_path, symbol_name, symbol_kind, start_line, end_line}.")]
    async fn find_symbol(&self, params: Parameters<FindSymbolParams>) -> Result<String, String> {
        let p = params.0;
        let limit = p.limit.unwrap_or(20);
        let name = p.name;
        tokio::task::spawn_blocking(move || {
            crate::codeindex::with_index(|idx| {
                let hits = idx.find_symbol(&name, limit).map_err(|e| e.to_string())?;
                serde_json::to_string(&hits).map_err(|e| e.to_string())
            })
        })
        .await
        .map_err(|e| format!("find_symbol task failed: {e}"))?
    }

    #[tool(description = "Open a pureterm:// URL against the running GUI (new tab, switch workspace/tab, ssh, send command). Returns the Socket API JSON result.")]
    async fn open_url(&self, params: Parameters<OpenUrlParams>) -> Result<String, String> {
        let url = params.0.url;
        let socket = self.socket.clone();
        tokio::task::spawn_blocking(move || {
            let action = crate::automation::parse_pureterm_url(&url)?;
            let res = crate::automation::execute_action(&action, socket)?;
            Ok(to_text(res))
        })
        .await
        .map_err(|e| format!("open_url task failed: {e}"))?
    }

    #[tool(description = "GAP-04: capture the full desktop screen as a PNG data URL. macOS uses `screencapture`; other platforms return an error in MVP. Requires user approval — a Feed panel prompt is raised before any capture; deny/no-GUI/timeout all fail closed.")]
    async fn desktop_screenshot(&self) -> Result<String, String> {
        let socket = self.socket.clone();
        tokio::task::spawn_blocking(move || {
            // SECURITY (spec §安全): a full-desktop screenshot can exfiltrate
            // passwords / 2FA / DMs to the agent. Require affirmative human
            // consent via the Feed approval panel before capturing, and fail
            // closed on deny / no-GUI / timeout. The allow/deny decision lives
            // in `computer_use::approval_allows_screenshot` (pure, unit-tested).
            let req = FeedApprovalRequest {
                session_id: "computer-use",
                event: "computer_use.screenshot",
                tool_name: Some("desktop_screenshot"),
                tool_input: None,
                prompt: None,
                message: None,
                reason: "Computer Use: capture full desktop screenshot",
                cwd: "",
            };
            let outcome = request_approval(&req, APPROVAL_TIMEOUT, socket);
            if !crate::computer_use::approval_allows_screenshot(&outcome) {
                return Err(
                    "desktop_screenshot: not approved (denied / no GUI / timed out)".to_string(),
                );
            }
            let screenshot = crate::computer_use::screenshot_data_url()?;
            serde_json::to_string(&json!({ "ok": true, "screenshot": screenshot }))
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("desktop_screenshot task failed: {e}"))?
    }

    #[tool(description = "GAP-04 MVP stub: click at screen coordinates (x, y). Returns ok or a not-implemented error.")]
    async fn desktop_click(&self, params: Parameters<DesktopClickParams>) -> Result<String, String> {
        let p = params.0;
        tokio::task::spawn_blocking(move || {
            // SECURITY: when input injection is implemented, gate it behind
            // request_approval (as desktop_screenshot does) before invoking.
            crate::computer_use::click(p.x, p.y)?;
            serde_json::to_string(&json!({ "ok": true })).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("desktop_click task failed: {e}"))?
    }

    #[tool(description = "GAP-04 MVP stub: type text into the focused application.")]
    async fn desktop_type(&self, params: Parameters<DesktopTypeParams>) -> Result<String, String> {
        let text = params.0.text;
        tokio::task::spawn_blocking(move || {
            // SECURITY: when input injection is implemented, gate it behind
            // request_approval (as desktop_screenshot does) before invoking.
            crate::computer_use::type_text(&text)?;
            serde_json::to_string(&json!({ "ok": true })).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("desktop_type task failed: {e}"))?
    }

    #[tool(description = "GAP-04 MVP stub: press a key by name (e.g. Return, Tab).")]
    async fn desktop_key(&self, params: Parameters<DesktopKeyParams>) -> Result<String, String> {
        let key = params.0.key;
        tokio::task::spawn_blocking(move || {
            // SECURITY: when input injection is implemented, gate it behind
            // request_approval (as desktop_screenshot does) before invoking.
            crate::computer_use::key_press(&key)?;
            serde_json::to_string(&json!({ "ok": true })).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("desktop_key task failed: {e}"))?
    }

    #[tool(description = "Return cwd for a pane (default: active). GAP-47 AppleScript alias — wraps shell.context.")]
    async fn script_get_cwd(
        &self,
        params: Parameters<GetShellContextParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let mut obj = serde_json::Map::new();
        if let Some(id) = p.pane_id {
            obj.insert("pane_id".into(), json!(id));
        }
        let res = rpc("shell.context", Some(Value::Object(obj)), self.socket.clone()).await?;
        let val = res.unwrap_or(Value::Null);
        let cwd = val.get("cwd").cloned().unwrap_or(Value::Null);
        Ok(to_text(Some(cwd)))
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
