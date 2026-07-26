//! 本地（无 IPC）子命令组 — agents/feed/replay/events/agent(daemon)。
//!
//! 这些命令读/写本地 JSONL 或配置，或直连 daemon 控制面，不走 GUI socket。

use clap::Subcommand;

/// GAP-7.2: install PureTerm hook shims into external agent configs.
#[derive(Subcommand)]
pub enum AgentsSub {
    /// Install PureTerm hooks for an agent (e.g. `claude-code`)
    Install {
        /// Agent id, e.g. `claude-code`
        adapter: String,
        /// Register all 9 Claude Code events (default: only events with
        /// configured `[[agent.hook]]` rules)
        #[arg(long)]
        all_events: bool,
        /// Preview the merged config without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// List known agent adapters + install status
    List,
    /// Remove PureTerm hooks from an agent config
    Uninstall {
        /// Agent id, e.g. `claude-code`
        adapter: String,
    },
    /// GAP-7.5: list persisted agent session ids (for cold resume). Reads the
    /// local `agents/<kind>-sessions.json` store; no IPC.
    Sessions {
        /// Filter by agent kind (e.g. `claude-code`); omit to list all kinds
        #[arg(long)]
        kind: Option<String>,
    },
    /// GAP-7.6: scan host processes for agents declared in `[[vault.agents]]`
    /// (via their `detect` substring). Surfaces agents running outside
    /// PureTerm so you can resume them. No IPC.
    Detect,
    /// GAP-7.6: print the parsed `[[vault.agents]]` registry (kind / detect /
    /// resume_command / never_hibernate) to confirm config took effect. No IPC.
    Registry,
}

/// GAP-7.6: read the Feed approval audit log (`feed.jsonl`). Local only.
#[derive(Subcommand)]
pub enum FeedSub {
    /// List recent feed approval decisions (newest first)
    List {
        /// Max number of entries to show (default 50)
        #[arg(long)]
        limit: Option<usize>,
    },
}

/// GAP-11: instant replay — list/export session timeline from events.jsonl.
#[derive(Subcommand)]
pub enum ReplaySub {
    /// List replay frames (newest first)
    List {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        session: Option<String>,
    },
    /// Export replay frames to standalone JSONL
    Export {
        #[arg(long)]
        output: std::path::PathBuf,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        session: Option<String>,
    },
}

/// GAP-6.7: read/follow the unified event log (`events.jsonl`). Local only.
#[derive(Subcommand)]
pub enum EventsSub {
    /// List recent events (newest first)
    List {
        /// Max number of events to show (default 50)
        #[arg(long)]
        limit: Option<usize>,
        /// Filter: category (`feed`) or `category.kind` (`feed.resolved`)
        #[arg(long = "type")]
        r#type: Option<String>,
    },
    /// Follow the event log in real time (tail -f)
    Tail {
        /// Filter: category (`feed`) or `category.kind` (`feed.resolved`)
        #[arg(long = "type")]
        r#type: Option<String>,
    },
}

/// GAP-7.3: hibernate/resume agent sessions via the daemon control plane
/// (`agent.hibernate` / `agent.resume`). Hits `puretermd` directly (not the
/// GUI IPC socket).
#[cfg(unix)]
#[derive(Subcommand)]
pub enum AgentSub {
    /// SIGSTOP a live session (hibernate it to free resources)
    Hibernate {
        /// Daemon session id (see `agent list`)
        session_id: String,
    },
    /// SIGCONT a hibernated session (resume it)
    Resume {
        /// Daemon session id
        session_id: String,
    },
    /// List all daemon sessions with idle/hibernated status
    List,
}

/// GAP-7.7: `pureterm surface resume ...` — manage custom resume bindings
/// (named, templated shell commands that reattach to an external persistent
/// session such as tmux/screen on cold-resume). Local config CRUD; no IPC.
/// Unix-only (resume execution goes through `puretermd`).
#[cfg(unix)]
#[derive(Subcommand, Clone)]
pub enum ResumeSub {
    /// Add or replace a resume binding keyed by (kind, checkpoint).
    Set {
        /// Agent kind this binding applies to (matches vault.agents.kind).
        #[arg(long)]
        kind: String,
        /// Named checkpoint; also the value substituted for {checkpoint}.
        #[arg(long)]
        checkpoint: String,
        /// argv template for the resume command. Supports the placeholders
        /// {session_id}, {checkpoint}, {kind}, {cwd}. Must be the LAST
        /// argument, e.g. `--shell tmux attach -t {checkpoint}`.
        #[arg(long, num_args = 1.., allow_hyphen_values = true)]
        shell: Vec<String>,
        /// If set, cold-resume will attempt this binding (subject to HMAC-token
        /// approval — see M2).
        #[arg(long)]
        auto: bool,
    },
    /// List all resume bindings.
    List,
    /// Remove the resume binding keyed by (kind, checkpoint).
    Remove {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        checkpoint: String,
    },
    /// Execute a resume binding now (manual trigger; M2 — approval + spawn).
    Run {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        checkpoint: String,
    },
}

/// Manage the persistent puretermd daemon + login autostart (GAP-5.1 M4).
#[derive(Subcommand)]
pub enum DaemonSub {
    /// Show daemon status (installed/loaded/running) + ping it
    Status,
    /// Install the login autostart (macOS LaunchAgent / systemd user service)
    Install,
    /// Remove the login autostart
    Uninstall,
}

/// GAP-7.10: `pureterm mcp …` subcommands.
///
/// - `serve` runs the stdio MCP server (feature-gated `mcp-server`); each tool
///   forwards to the GUI Socket API. Install adapters (Phase C) land here too.
/// - `list-tools` prints the roster (filtered by `[mcp].enabled_tools`) — local,
///   no IPC, available in every build.
#[derive(Subcommand)]
pub enum McpSub {
    /// Run the MCP stdio server. Reads JSON-RPC on stdin, writes on stdout —
    /// meant to be spawned by an MCP client (Claude Code / Codex). Requires the
    /// `mcp-server` build feature (on by default).
    Serve,
    /// Print the MCP tool roster this build exposes, filtered by
    /// `[mcp].enabled_tools`. Local only — no GUI/IPC needed.
    ListTools,
    /// GAP-7.10 (Phase C): write PureTerm's MCP server into an agent client's
    /// config so it auto-launches. `client`: claude-code | codex | claude-desktop.
    /// Idempotent; preserves other servers the client already has.
    Install {
        /// Which client to install into: claude-code | codex | claude-desktop.
        client: String,
    },
    /// Show which clients have PureTerm's MCP server installed.
    List,
    /// Remove PureTerm's MCP server from a client's config.
    Uninstall {
        /// Which client: claude-code | codex | claude-desktop.
        client: String,
    },
}
