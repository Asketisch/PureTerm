//! GAP-7.10：MCP stdio 服务器（`pureterm mcp serve`），由 `mcp-server`
//! 特性门控。每个工具都是现有 GUI JSON-RPC Socket API 之上的薄适配器——
//! 它构建参数对象并经 `rpc()` 转发；`rpc()` 把**阻塞的**
//! `ipc::client::send_rpc` 包在 `spawn_blocking` 中，使 rmcp 异步工作线程
//! 永远不会阻塞在 socket I/O 上。
//!
//! 日志**只写 stderr**——stdout 是 MCP 传输通道。

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

/// 把 MCP 工具调用桥接到 GUI Socket API。`send_rpc` 是阻塞的
/// （在 Unix socket 上 write_all + shutdown + read_line），因此它必须离开
/// rmcp 异步工作线程运行——`spawn_blocking` 把它停放在阻塞池上。
/// `socket_override = None` 时遵循 `PURETERM_SOCKET` / 默认发现（面板内
/// agent 场景：生成的 `pureterm mcp serve` 继承环境变量，把自己钉到
/// 正确的 GUI 实例上）。
async fn rpc(method: &str, params: Option<Value>, socket: Option<PathBuf>) -> Result<Option<Value>, String> {
    let m = method.to_string();
    tokio::task::spawn_blocking(move || send_rpc(&m, params, socket))
        .await
        .map_err(|e| format!("rpc task failed: {e}"))?
}

/// 把 Socket-API 结果字符串化后返回给 MCP 客户端。`Ok(None)`（没有结果
/// 字段的方法）变为忠实的 JSON `null`，而不是空字符串，使 agent 能区分
/// “无载荷”与“空载荷”。
fn to_text(res: Option<Value>) -> String {
    serde_json::to_string(&res).unwrap_or_else(|_| "null".into())
}

/// `desktop_screenshot` 的 Feed 审批提示在失败关闭前等待人工选择多久。
/// 与 GUI Feed 面板的 120s 自动拒绝软超时（`feed_panel.rs`）一致，
/// 使两侧同时超时。
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

pub struct PureTermServer {
    cfg: McpConfig,
    socket: Option<PathBuf>,
}

// ── 工具参数模式 ───────────────────────────────────────────────
// `#[serde(default)]` 使可选字段真正可选（agent 可以省略它们）；
// 像 `SendTextParams::text` 这样缺失的必填字段会导致反序列化失败，
// rmcp 返回结构化错误。

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct CapturePaneParams {
    /// 目标面板 id（默认：活动面板）。
    #[serde(default)]
    pane_id: Option<u64>,
    /// 只捕获可见视口（默认：完整回滚，受 `[mcp].default_capture_lines`
    /// 限制）。
    #[serde(default)]
    visible_only: Option<bool>,
    /// 只保留最后 N 行（默认：`[mcp].default_capture_lines`）。
    #[serde(default)]
    lines: Option<usize>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct GetShellContextParams {
    /// 目标面板 id（默认：活动面板）。
    #[serde(default)]
    pane_id: Option<u64>,
    /// 把 recent_commands 列表限制到最后 N 条（默认：10）。
    #[serde(default)]
    recent: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SendTextParams {
    /// 要输入到面板的文本。用两个字符 \r 表示回车。
    text: String,
    /// 目标面板 id（默认：活动面板）。
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct PaneIdParams {
    /// 目标面板 id（默认：活动面板）。
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SplitPaneParams {
    /// 拆分方向：“horizontal”（并排）或 “vertical”（堆叠）。
    #[serde(default)]
    direction: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct FocusPaneParams {
    /// “prev” 或 “next”（默认：“next”）。
    #[serde(default)]
    direction: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserNavigateParams {
    /// 要把网页面板导航到的 URL。
    url: String,
    /// 目标网页面板 id（默认：第一个网页面板）。
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserExtractParams {
    /// 要匹配的 CSS 选择器。
    selector: String,
    /// 要提取的元素属性（默认：可见文本内容）。
    #[serde(default)]
    attribute: Option<String>,
    /// 目标网页面板 id（默认：第一个网页面板）。
    #[serde(default)]
    pane_id: Option<u64>,
}

// GAP-7.8：其余 agent 浏览器操作。`pane_id` 在 GUI 侧解析
// （handle_browser_request 在省略时选择第一个网页面板），因此每个工具
// 只转发其操作专属参数——这里不做逐操作的面板解析。

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserClickParams {
    /// 要点击的元素的 CSS 选择器。
    selector: String,
    /// 目标网页面板 id（默认：第一个网页面板）。
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserTypeParams {
    /// 要输入的目标输入框/元素的 CSS 选择器。
    selector: String,
    /// 要输入到元素中的文本。
    text: String,
    /// 目标网页面板 id（默认：第一个网页面板）。
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserScrollParams {
    /// 水平滚动增量（像素）（默认 0；设置了 `selector` 时忽略）。
    #[serde(default)]
    x: Option<i64>,
    /// 垂直滚动增量（像素）（默认 0；设置了 `selector` 时忽略）。
    #[serde(default)]
    y: Option<i64>,
    /// 要滚动到视图中的 CSS 选择器（设置时覆盖 x/y）。
    #[serde(default)]
    selector: Option<String>,
    /// 目标网页面板 id（默认：第一个网页面板）。
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserWaitParams {
    /// 要等待的 CSS 选择器。
    selector: String,
    /// 超时（毫秒）（默认 5000）。超时前最多阻塞约 6.5s。
    #[serde(default)]
    timeout_ms: Option<u64>,
    /// 目标网页面板 id（默认：第一个网页面板）。
    #[serde(default)]
    pane_id: Option<u64>,
}

/// 仅面板 id 的浏览器操作（get_url / get_title / screenshot）共享的参数。
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BrowserPaneIdParams {
    /// 目标网页面板 id（默认：第一个网页面板）。
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SearchCodeParams {
    /// 关键词查询（FTS5 前缀 token）。
    query: String,
    /// 最大命中数（默认 20）。
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct GetFileContextParams {
    /// 工作区相对或绝对路径。
    path: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct FindSymbolParams {
    /// 符号名子串（例如函数或类名）。
    name: String,
    /// 最大命中数（默认 20）。
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct OpenUrlParams {
    /// 一个 `pureterm://` URL（tab/workspace/ssh/command 动作）。
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
        // 调用方未限定上限时应用配置的默认捕获上限，使无界全回滚捕获只在
        // agent 通过大的 `lines` 显式选择时发生。
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
            // 匹配操作的事件载荷形状：缺失的 x/y 默认为 0。
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
            // 密码 / 2FA / DM 给 agent。捕获前需要通过 Feed 审批面板获得人工
            // 明确同意；在拒绝 / 无 GUI / 超时时失败关闭。允许/拒绝决策位于
            // `computer_use::approval_allows_screenshot`（纯函数，已单元测试）。
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
            // SECURITY：实现输入注入时，调用前要像 desktop_screenshot 那样用
            // request_approval 把关。
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
            // SECURITY：实现输入注入时，调用前要像 desktop_screenshot 那样
            // 用 request_approval 把关。
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
            // SECURITY：实现输入注入时，调用前要像 desktop_screenshot 那样
            // 用 request_approval 把关。
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
        // `InitializeResult` 是 `#[non_exhaustive]`，所以使用其构造函数
        // （而不是结构体字面量）。`from_build_env()` 会读取 rmcp 源码内部的
        // CARGO_CRATE_NAME → “rmcp”；显式设置名称，使客户端看到
        // “pureterm” + 本 crate 的版本。
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(INSTRUCTIONS)
            .with_server_info(Implementation::new(
                "pureterm",
                env!("CARGO_PKG_VERSION"),
            ))
    }
}

/// 运行 MCP stdio 服务器，直到 stdin 关闭（客户端断开）或传输出错。
/// 只向 stderr 记录。构建自己的当前线程 tokio 运行时：工具调用由
/// agent 驱动（低频），`rpc()` 把阻塞的 socket 工作卸载到阻塞池。
pub fn run_server(cfg: McpConfig, socket: Option<PathBuf>) {
    // stdout 是 MCP 传输通道——只有 stderr 可用于诊断。
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
