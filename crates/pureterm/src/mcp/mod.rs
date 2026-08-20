//! GAP-7.10:模型上下文协议(MCP)服务器——把 PureTerm 的终端
//! 界面以原生、结构化的工具形式暴露给 AI agent(Claude Code、Codex)。
//!
//! `pureterm mcp serve` 运行一个 **stdio MCP 服务器**。每次 MCP 工具调用都是
//! 对现有 GUI JSON-RPC Socket API 的薄适配器
//! (`ipc::client::send_rpc`)——见下方 `server`。服务器由特性开关门控:
//! 需启用 `mcp-server`(引入 `rmcp`);此处的工具表/白名单/配置是
//! 纯代码且始终编译,未启用特性也能单元测试。
//!
//! 新工具归入 `mcp.*` 命名空间,绝不触碰外部 CLI 的
//! `agent.*` 命名空间(GAP-7.1–7.7)。

use crate::app_config::McpConfig;

// rmcp server 由特性开关门控(`pureterm mcp serve`)。仅在启用
// `mcp-server` 时编译;上方的工具表/白名单/配置辅助保持常开,
// 从而无需引入 rmcp 即可单元测试。
#[cfg(feature = "mcp-server")]
mod server;
#[cfg(feature = "mcp-server")]
pub use server::run_server;

/// GAP-7.10(Phase C):agent 客户端安装适配器(`pureterm mcp install`)。
/// 始终编译(无 rmcp)——纯配置 RMW,镜像 `agent_adapters`。
pub mod adapter;

/// 本服务器暴露的一个 MCP 工具。`rpc_method` 是该工具转发到的 GUI Socket API 方法
/// —— 少数工具(GAP-04 桌面操作)则为 `"(in-process)"`,表示
/// 在 MCP 服务器进程内本地运行、不经 Socket;`description`
/// 是 agent 在其工具列表中看到的内容。
#[derive(Clone, Copy, Debug)]
pub struct ToolSpec {
    pub name: &'static str,
    pub rpc_method: &'static str,
    pub description: &'static str,
}

/// PureTerm 可暴露的 MCP 工具全集。v1 覆盖窗格读写、
/// 布局驱动以及内置浏览器。在此处新增条目;`server` 中的 `#[tool]`
/// 函数体通过各自的 `rpc_method` 转发,标记为
/// `"(in-process)"` 的条目则在服务器内本地运行(GAP-04 桌面操作)。
pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "capture_pane",
        rpc_method: "surface.snapshot",
        description: "Read a terminal pane's buffer as text (full scrollback or the visible viewport). Returns {pane_id, text, lines, truncated}.",
    },
    ToolSpec {
        name: "get_shell_context",
        rpc_method: "shell.context",
        description: "Return the shell context for a pane (default: active): cwd, last_command {text, exit_code, duration_us}, and recent_commands. Requires shell integration (zsh/bash). Non-integrated panes (SSH/fish) return integrated=false. Use this to understand what the user just ran before responding.",
    },
    ToolSpec {
        name: "trigger_list",
        rpc_method: "trigger.list",
        description: "List configured [[triggers]] rules (name, pattern, action). Does not return send_text secrets.",
    },
    ToolSpec {
        name: "send_text",
        rpc_method: "surface.send_text",
        description: "Write text into a pane's PTY (default: the active pane). Use \\r for Enter. Fire-and-forget; pair with capture_pane to read the result.",
    },
    ToolSpec {
        name: "list_surfaces",
        rpc_method: "surface.list",
        description: "List every terminal pane with its id, tab, and cwd. Use a returned pane_id with capture_pane/send_text.",
    },
    ToolSpec {
        name: "new_tab",
        rpc_method: "tab.new",
        description: "Open a new tab and return its id.",
    },
    ToolSpec {
        name: "split_pane",
        rpc_method: "pane.split",
        description: "Split the active pane (direction: \"horizontal\" | \"vertical\") and return the new pane id.",
    },
    ToolSpec {
        name: "focus_pane",
        rpc_method: "pane.focus",
        description: "Move focus to an adjacent pane (direction: \"prev\" | \"next\").",
    },
    ToolSpec {
        name: "browser_navigate",
        rpc_method: "browser.navigate",
        description: "Navigate an embedded web pane to a URL (pane_id from pane list; omit for the first web pane).",
    },
    ToolSpec {
        name: "browser_extract",
        rpc_method: "browser.extract",
        description: "Extract text or an attribute from a web pane via CSS selector. Deferred — returns once the page settles.",
    },
    ToolSpec {
        name: "browser_click",
        rpc_method: "browser.click",
        description: "Click an element in a web pane by CSS selector.",
    },
    ToolSpec {
        name: "browser_type",
        rpc_method: "browser.type",
        description: "Type text into an input/contenteditable element in a web pane by CSS selector.",
    },
    ToolSpec {
        name: "browser_scroll",
        rpc_method: "browser.scroll",
        description: "Scroll a web pane by (x, y) deltas or scroll a CSS-selected element into view.",
    },
    ToolSpec {
        name: "browser_wait",
        rpc_method: "browser.wait",
        description: "Wait for a CSS-selected element to appear in a web pane (blocks until found or timeout).",
    },
    ToolSpec {
        name: "browser_get_url",
        rpc_method: "browser.get_url",
        description: "Get the current URL (location.href) of a web pane.",
    },
    ToolSpec {
        name: "browser_get_title",
        rpc_method: "browser.get_title",
        description: "Get the page title (document.title) of a web pane.",
    },
    ToolSpec {
        name: "browser_screenshot",
        rpc_method: "browser.screenshot",
        description: "Capture a PNG screenshot of a web pane (returns a base64 data URL; macOS-only).",
    },
    ToolSpec {
        name: "search_code",
        rpc_method: "codeindex.search",
        description: "Keyword search over the indexed workspace (Tree-sitter chunks + SQLite FTS). Query the workspace root via PURETERM_CODEINDEX_ROOT or cwd.",
    },
    ToolSpec {
        name: "get_file_context",
        rpc_method: "codeindex.file_context",
        description: "Return semantic chunks for a source file (functions/classes/modules) from the codebase index.",
    },
    ToolSpec {
        name: "find_symbol",
        rpc_method: "codeindex.find_symbol",
        description: "Find definitions by symbol name (substring match) in the indexed workspace.",
    },
    ToolSpec {
        name: "open_url",
        rpc_method: "automation.open_url",
        description: "Open a pureterm:// URL (tab/workspace/ssh/command actions) against the running GUI.",
    },
    ToolSpec {
        name: "desktop_screenshot",
        rpc_method: "(in-process)",
        description: "GAP-04: capture the desktop as a PNG data URL (macOS screencapture MVP). Requires user approval (Feed panel) before capture.",
    },
    ToolSpec {
        name: "desktop_click",
        rpc_method: "(in-process)",
        description: "GAP-04 MVP stub: click at screen coordinates (not yet implemented).",
    },
    ToolSpec {
        name: "desktop_type",
        rpc_method: "(in-process)",
        description: "GAP-04 MVP stub: type text into the focused app (not yet implemented).",
    },
    ToolSpec {
        name: "desktop_key",
        rpc_method: "(in-process)",
        description: "GAP-04 MVP stub: press a key by name (not yet implemented).",
    },
    ToolSpec {
        name: "script_get_cwd",
        rpc_method: "shell.context",
        description: "GAP-47: return cwd for the active (or specified) pane — AppleScript/MCP alias for shell.context.",
    },
];

/// 按名称解析工具规格(区分大小写)。未知工具返回 `None`——
/// 调用方将其视为服务器缺陷(工具表已枚举全部)。
pub fn tool_spec(name: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|t| t.name == name)
}

/// 把 `[mcp].enabled_tools` 白名单应用到工具表。语义:
/// - `None`(键缺失)或**空**数组 → 全部工具(友好默认:
///   空数组表示「不限制」,而非「无工具」)。
/// - 否则 → 只保留名称出现在 `enabled` 中的工具,按工具表顺序。
///   未知条目被静默丢弃(那是配置笔误,不是崩溃)。
pub fn enabled_tools(whitelist: Option<&[String]>) -> Vec<&'static ToolSpec> {
    match whitelist {
        None => TOOLS.iter().collect(),
        Some(names) if names.is_empty() => TOOLS.iter().collect(),
        Some(names) => TOOLS
            .iter()
            .filter(|t| names.iter().any(|n| n == t.name))
            .collect(),
    }
}

/// 应用 `cfg.enabled_tools` 之后,本服务器将通告的工具名。
/// 供 `mcp list-tools` 与 ServerHandler 信息使用。
pub fn enabled_tool_names(cfg: &McpConfig) -> Vec<&'static str> {
    let wl: Option<&[String]> = if cfg.enabled_tools.is_empty() {
        None
    } else {
        Some(&cfg.enabled_tools)
    };
    enabled_tools(wl)
        .into_iter()
        .map(|t| t.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_includes_core_read_write_tools() {
        let names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
        assert!(names.contains(&"capture_pane"), "missing capture_pane: {names:?}");
        assert!(names.contains(&"send_text"), "missing send_text: {names:?}");
        assert!(names.contains(&"list_surfaces"), "missing list_surfaces: {names:?}");
        assert!(names.contains(&"search_code"), "missing search_code: {names:?}");
    }

    #[test]
    fn capture_pane_forwards_to_surface_snapshot() {
        assert_eq!(tool_spec("capture_pane").unwrap().rpc_method, "surface.snapshot");
        assert_eq!(tool_spec("send_text").unwrap().rpc_method, "surface.send_text");
        assert_eq!(tool_spec("list_surfaces").unwrap().rpc_method, "surface.list");
    }

    #[test]
    fn browser_tools_forward_to_browser_namespace() {
        assert_eq!(tool_spec("browser_navigate").unwrap().rpc_method, "browser.navigate");
        assert_eq!(tool_spec("browser_extract").unwrap().rpc_method, "browser.extract");
        assert_eq!(tool_spec("browser_click").unwrap().rpc_method, "browser.click");
        assert_eq!(tool_spec("browser_type").unwrap().rpc_method, "browser.type");
        assert_eq!(tool_spec("browser_scroll").unwrap().rpc_method, "browser.scroll");
        assert_eq!(tool_spec("browser_wait").unwrap().rpc_method, "browser.wait");
        assert_eq!(tool_spec("browser_get_url").unwrap().rpc_method, "browser.get_url");
        assert_eq!(tool_spec("browser_get_title").unwrap().rpc_method, "browser.get_title");
        assert_eq!(tool_spec("browser_screenshot").unwrap().rpc_method, "browser.screenshot");
    }

    #[test]
    fn desktop_tools_use_in_process_sentinel() {
        // GAP-04 桌面工具在 MCP 服务器进程内运行(不经 Socket),
        // 因此 `rpc_method` 是进程内哨兵——而非虚构的
        // `computer_use.*` Socket 方法(该类方法不存在 GUI 分发器)。
        // 通告不存在的方法会误导 agent 与 `mcp`
        // list-tools 输出;此断言锁定了真实取值。
        for name in ["desktop_screenshot", "desktop_click", "desktop_type", "desktop_key"] {
            let spec = tool_spec(name).unwrap_or_else(|| panic!("{name} must be in roster"));
            assert_eq!(
                spec.rpc_method,
                "(in-process)",
                "{name}: rpc_method must be the in-process sentinel"
            );
        }
    }

    #[test]
    fn browser_tools_cover_full_op_set() {
        // GAP-7.8 完成标准:agent 可驱动全部功能浏览器操作。完整
        // 集合(navigate/click/type/extract/scroll/wait/get_url/get_title +
        // macOS 截图)必须全部暴露给 MCP。
        let browser: Vec<&str> = TOOLS
            .iter()
            .filter(|t| t.rpc_method.starts_with("browser."))
            .map(|t| t.name)
            .collect();
        assert_eq!(
            browser,
            vec![
                "browser_navigate",
                "browser_extract",
                "browser_click",
                "browser_type",
                "browser_scroll",
                "browser_wait",
                "browser_get_url",
                "browser_get_title",
                "browser_screenshot",
            ]
        );
    }

    #[test]
    fn unknown_tool_resolves_to_none() {
        assert!(tool_spec("does_not_exist").is_none());
    }

    #[test]
    fn absent_whitelist_yields_all_tools() {
        assert_eq!(enabled_tools(None).len(), TOOLS.len());
    }

    #[test]
    fn empty_whitelist_yields_all_tools_not_none() {
        // 空数组表示「不限制」,而非「全部禁用」。
        assert_eq!(enabled_tools(Some(&[])).len(), TOOLS.len());
    }

    #[test]
    fn whitelist_keeps_listed_tools_in_roster_order() {
        // 传入顺序打乱;结果遵循 TOOLS 顺序,而非输入顺序。
        let names = vec!["send_text".to_string(), "capture_pane".to_string()];
        let got: Vec<&str> = enabled_tools(Some(&names))
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(got, vec!["capture_pane", "send_text"]);
    }

    #[test]
    fn whitelist_drops_unknown_entries() {
        let names = vec!["capture_pane".to_string(), "bogus".to_string()];
        let got: Vec<&str> = enabled_tools(Some(&names))
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(got, vec!["capture_pane"]);
    }
}
