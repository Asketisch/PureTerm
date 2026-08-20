//! 核心配置结构体 —— vault/agent/resume/mcp/applescript/url/lsp 子配置。
//!
//! 这些是「非 UI」的集成型配置结构,从主 `app_config.rs` 抽出以控制行数。
//! UI 相关结构体见 `structs_ui.rs`。

use std::path::Path;

/// GAP-7.6/7.5：`[[vault.agents]]` 条目，声明如何恢复一个 agent。
/// `resume_command` 可含 `{session_id}` 占位符，在冷恢复时替换
///（如 `["claude", "--resume", "{session_id}"]`）。
///
/// GAP-7.6 字段:`name`/`detect`/`session_id_source`。`detect` 是进程名或 cmdline
/// 的**子串**(非 shell 命令 —— 避免注入与 shell 依赖,偏离 cmux 的 shell 风格),
/// 由 `agent_detect::scan` 在启动/周期扫描时识别机器上正在运行的 agent 进程。
/// `session_id_source` 本期仅文档化、不消费(预留 GAP-7.7)。
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct VaultAgentEntry {
    pub kind: String,
    pub resume_command: Vec<String>,
    /// 为 true 时，该 agent 类型免于休眠（GAP-7.3）。
    #[serde(default)]
    pub never_hibernate: bool,
    /// 可选:展示名(缺省回退 `kind`)。
    #[serde(default)]
    pub name: Option<String>,
    /// 可选:进程名/cmdline 子串,供 `agent_detect::scan` 识别运行中的 agent。
    /// 空/缺省 = 用 `kind` 小写形式。
    #[serde(default)]
    pub detect: Option<String>,
    /// 可选:预留(GAP-7.7),本期不消费。当前固定 transcript/env 启发式推断。
    #[serde(default)]
    pub session_id_source: Option<String>,
}

/// GAP-7.7：`[[resume.bindings]]` 条目——命名、带模板的 shell 命令，
/// 用于冷恢复时重新附加到外部持久会话（tmux/screen/zellij），
/// 替代或补充默认的 `claude --resume <id>`。以 `(kind, checkpoint)` 为键；
/// `shell` 是 argv 模板，支持 `{session_id}`、`{checkpoint}`、`{kind}`、
/// `{cwd}` 占位符，经轻量 `String::replace` 插值（不用 handlebars——与
/// `session_restore.rs:378` 的 `VaultAgentEntry.resume_command` 保持一致）。
/// `auto = true` 表示冷恢复会尝试此绑定（需通过 HMAC 令牌授权，见 M2）。
///
/// 注：`docs/TODOLIST.md` 中 GAP-7.7 的叙述提到 `handlebars`；我们刻意
/// 使用轻量占位符展开（YAGNI——DoD 不需要条件逻辑，且与现有
/// resume_command 插值风格一致）。
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
pub struct ResumeBinding {
    pub kind: String,
    pub checkpoint: String,
    pub shell: Vec<String>,
    #[serde(default)]
    pub auto: bool,
    /// T-core-tmux-cc v2：当 `kind = "tmux"` 时，设 `use_control_mode = true`
    /// 以通过 `tmux -CC`（控制模式）附加，而非旧的 `tmux attach -t`。
    /// 默认 `false` 让现有配置无需迁移即可工作；v3 会把默认翻转为 `true`。
    #[serde(default)]
    pub use_control_mode: bool,
    /// T-core-tmux-cc v2：当 `use_control_mode = true` 时，GUI 捕获
    /// 此会话名（未提供时从 `checkpoint` 自动生成），用于
    /// `tmux -CC new-session -A -s` 调用。
    #[serde(default)]
    pub session_name: Option<String>,
}

impl ResumeBinding {
    /// 该绑定处于 tmux 控制模式时，返回有效的 `tmux -CC` 参数。
    /// `use_control_mode = false` 时回退为冷 `attach -t`。
    ///
    /// 一旦有稳定的 GUI 集成，v3 将优先使用 `-CC` 形式；v2 仍以冷路径
    /// 为默认，以兼容现有用户配置。
    pub fn tmux_command_args(&self) -> Vec<String> {
        if self.use_control_mode && self.kind == "tmux" {
            let mut out = vec!["tmux".to_string(), "-CC".to_string(), "new-session".to_string(), "-A".to_string()];
            let name = self.session_name.clone().unwrap_or_else(|| self.checkpoint.clone());
            if !name.is_empty() {
                out.push("-s".to_string());
                out.push(format!("pureterm-{name}"));
            }
            out
        } else {
            self.shell.clone()
        }
    }
}

/// GAP-7.3：`[agent.hibernation]` 设置。默认值为禁用配置，因此缺失/畸形
/// 的表永远不会破坏启动。双重空闲条件为 `pty_idle_secs` 与
/// `cpu_idle_secs` 同时满足。
#[derive(Clone, Debug)]
pub struct AgentHibernationConfig {
    pub enabled: bool,
    pub max_live_terminals: u32,
    /// PTY 静默（无输出）达到该秒数。
    pub pty_idle_secs: u64,
    /// 子进程 CPU 接近零持续该秒数。
    pub cpu_idle_secs: u64,
    /// 确认面板自动处理超时（秒）。
    pub confirm_secs: u64,
    /// GUI 休眠扫描间隔（秒）。
    pub scan_interval_secs: u64,
    /// 守护进程 CPU 采样间隔（秒）。
    pub cpu_sample_interval_secs: u64,
}

impl Default for AgentHibernationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_live_terminals: 12,
            pty_idle_secs: 300,
            cpu_idle_secs: 300,
            confirm_secs: 60,
            scan_interval_secs: 3,
            cpu_sample_interval_secs: 1,
        }
    }
}

/// GAP-7.6：`[agent.detect]` 设置——驱动 detect-resume 提示面板。
/// 默认值为*启用*配置：检测是非侵入式提示，DoD 希望默认打开面板；
/// 缺失/畸形表永远不会破坏启动。未声明 `[[vault.agents]]` 时 `scan()`
/// 提前返回，因此直到用户注册 agent 前都是空操作。
#[derive(Clone, Debug)]
pub struct AgentDetectConfig {
    pub enabled: bool,
    /// 周期扫描间隔（秒）。与休眠的 tick 相互独立。
    pub scan_interval_secs: u64,
    /// 启动时扫描一次（除了周期 tick 之外）。
    pub scan_on_startup: bool,
}

impl Default for AgentDetectConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            scan_interval_secs: 30,
            scan_on_startup: true,
        }
    }
}

/// GAP-7.10：`[mcp]` 设置——调优 `pureterm mcp serve` 服务端。
/// 缺失/畸形表返回 `Default`（暴露全部工具、5000 行捕获上限），
/// 坏配置永远不会破坏启动。v1 只暴露这两个旋钮；`proxy_methods`
/// 白名单 + 流式捕获推迟到 v2。
#[derive(Clone, Debug)]
pub struct McpConfig {
    /// 工具省略 `lines` 时 `capture_pane` 的默认行数上限。限制 agent
    /// 未要求的失控全回滚捕获。
    pub default_capture_lines: usize,
    /// 暴露工具名的白名单。空/未设置 → 全部工具（见 `mcp::enabled_tools`）；
    /// 否则只广播列出的名称。
    pub enabled_tools: Vec<String>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            default_capture_lines: 5000,
            enabled_tools: Vec::new(),
        }
    }
}

/// GAP-47：`[applescript]` —— 门控 macOS 自动化桥接功能。
#[derive(Clone, Debug)]
pub struct AppleScriptConfig {
    pub enabled: bool,
}

impl Default for AppleScriptConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// GAP-48：`[url_scheme]` —— `pureterm://` 处理器注册 + 分发。
#[derive(Clone, Debug)]
pub struct UrlSchemeConfig {
    pub enabled: bool,
    /// 为 true 时，在 GUI 启动时调用一次 `LSSetDefaultHandlerForURLScheme`。
    pub register_on_launch: bool,
}

impl Default for UrlSchemeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            register_on_launch: false,
        }
    }
}

/// GAP-35：`[lsp]` 设置——内置 LSP 客户端（rust-analyzer MVP）。
/// 缺失/畸形表返回 `Default`（`enabled = false`，rust → `rust-analyzer`），
/// 二进制缺失时启动也不会失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspConfig {
    pub enabled: bool,
    /// 语言 id → 服务端命令（MVP：只消费 `rust`）。
    pub servers: std::collections::HashMap<String, String>,
}

impl Default for LspConfig {
    fn default() -> Self {
        let mut servers = std::collections::HashMap::new();
        servers.insert("rust".to_string(), "rust-analyzer".to_string());
        servers.insert("go".to_string(), "gopls".to_string());
        servers.insert("python".to_string(), "pyright-langserver".to_string());
        servers.insert("typescript".to_string(), "typescript-language-server".to_string());
        servers.insert("javascript".to_string(), "typescript-language-server".to_string());
        servers.insert("cpp".to_string(), "clangd".to_string());
        servers.insert("c".to_string(), "clangd".to_string());
        Self {
            enabled: false,
            servers,
        }
    }
}

impl LspConfig {
    pub fn rust_command(&self) -> String {
        self.servers
            .get("rust")
            .cloned()
            .unwrap_or_else(|| "rust-analyzer".to_string())
    }

    pub fn command_for_language(&self, lang: &str) -> Option<String> {
        self.servers.get(lang).cloned()
    }

    pub fn command_for_path(&self, path: &Path) -> Option<String> {
        pureterm_lsp::language_id(path).and_then(|lang| self.command_for_language(lang))
    }
}

