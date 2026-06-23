//! GAP-7.10: Model Context Protocol (MCP) server — exposes PureTerm's terminal
//! surface to AI agents (Claude Code, Codex) as native, structured tools.
//!
//! `pureterm mcp serve` runs a **stdio MCP server**. Each MCP tool call is a
//! thin adapter over the existing GUI JSON-RPC Socket API
//! (`ipc::client::send_rpc`) — see `server` below. The server is feature-gated
//! behind `mcp-server` (pulls `rmcp`); the roster / whitelist / config here are
//! pure and always compiled so they stay unit-testable without the feature.
//!
//! New tools belong to the `mcp.*` namespace and never touch the external-CLI
//! `agent.*` namespace (GAP-7.1–7.7).

use crate::app_config::McpConfig;

// Feature-gated rmcp server (`pureterm mcp serve`). Compiled only with
// `mcp-server`; the roster/whitelist/config helpers above stay always-on so
// they stay unit-testable without pulling rmcp.
#[cfg(feature = "mcp-server")]
mod server;
#[cfg(feature = "mcp-server")]
pub use server::run_server;

/// GAP-7.10 (Phase C): agent-client install adapters (`pureterm mcp install`).
/// Always compiled (no rmcp) — pure config RMW, mirrors `agent_adapters`.
pub mod adapter;

/// One MCP tool this server exposes. `rpc_method` is the GUI Socket API method
/// the tool forwards to; `description` is what the agent sees in its tool list.
#[derive(Clone, Copy, Debug)]
pub struct ToolSpec {
    pub name: &'static str,
    pub rpc_method: &'static str,
    pub description: &'static str,
}

/// The full roster of MCP tools PureTerm can expose. v1 covers pane read/write
/// + layout driving + the embedded browser. Add entries here; the `#[tool]`
/// bodies in `server` forward each via its `rpc_method`.
pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "capture_pane",
        rpc_method: "surface.snapshot",
        description: "Read a terminal pane's buffer as text (full scrollback or the visible viewport). Returns {pane_id, text, lines, truncated}.",
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
];

/// Resolve a tool spec by name (case-sensitive). Returns `None` for unknown
/// tools — callers treat that as a server bug (the roster is exhaustive).
pub fn tool_spec(name: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|t| t.name == name)
}

/// Apply the `[mcp].enabled_tools` whitelist to the roster. Semantics:
/// - `None` (key absent) or an **empty** slice → all tools (the friendly
///   default; an empty array reads as "no restriction", not "no tools").
/// - Otherwise → only tools whose name appears in `enabled`, in roster order.
///   Unknown entries are silently dropped (they're a config typo, not a crash).
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

/// The tool names this server will advertise, after applying `cfg.enabled_tools`.
/// Convenience for `mcp list-tools` and the ServerHandler info.
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
        // An empty array is "no restriction", not "disable everything".
        assert_eq!(enabled_tools(Some(&[])).len(), TOOLS.len());
    }

    #[test]
    fn whitelist_keeps_listed_tools_in_roster_order() {
        // Names given out of order; result follows TOOLS order, not input order.
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
