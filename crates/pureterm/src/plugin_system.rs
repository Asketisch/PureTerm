//! GAP-49：插件系统——目录扫描 + 清单解析 + 安装。
//!
//! ## 模型
//!
//! 一个“插件”是 [`PluginSystemConfig::plugin_dir`] 下的一个文件夹
//!（默认 `~/.config/pureterm/plugins/<名称>/`），其中包含一个 `plugin.toml`
//! 清单。清单是一个微小的 TOML 文件：
//!
//! ```toml
//! name = "my-plugin"      # 必填，须与文件夹名一致
//! version = "0.1.0"       # 必填，类 semver
//! description = ""        # 可选
//! author = ""             # 可选
//! # 未来的扩展点（尚未接入运行时）：
//! # provides = ["profile_provider", "status_bar_component"]
//! ```
//!
//! 运行时（WASM / dylib）有意推迟。脚手架只做校验清单、列出插件、
//! 安装/更新桩三件事。把插件挂进终端表面是 D 阶段的事。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `[plugins]` 配置。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSystemConfig {
    pub enabled: bool,
    pub directory: Option<PathBuf>,
}

impl Default for PluginSystemConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            directory: None,
        }
    }
}

impl PluginSystemConfig {
    pub fn plugin_dir(&self) -> PathBuf {
        self.directory.clone().unwrap_or_else(default_plugin_dir)
    }
}

fn default_plugin_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pureterm")
        .join("plugins")
}

/// 磁盘上的清单（`<插件>/plugin.toml`）。
///
/// t-core-plugin-runtime 在 v0 清单（GAP-49）基础上扩展了
/// `abi_version` / `entry_points` / `provides` / `config_schema` 字段。
/// 不含这些字段的 v0 清单仍然可用：每个新字段都是 `#[serde(default)]`，
/// 由加载器填上合理的默认值。v1 的 ABI 版本号为 `1`；未来升级需要
/// 加载器显式选择 `[pureterm abi = "N"]`。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    /// 该插件编译时针对的 ABI 版本。加载器拒绝加载 `abi_version` 大于
    /// 当前二进制支持的上限（目前为 1）的插件。
    #[serde(default = "default_abi_version")]
    pub abi_version: u32,
    /// 入口点：WASM 模块内 名称 → 函数索引 的映射。v1 的键是
    /// [`PLUGIN_ENTRY_POINTS`] 中的字符串。v0 清单的该映射为空。
    #[serde(default)]
    pub entry_points: std::collections::HashMap<String, String>,
    /// 能力声明。v1 以字符串存储；加载器通过 [`Provides::from_str`] 逐个解析。
    #[serde(default)]
    pub provides: Vec<String>,
    /// 可选的类 JSON-schema 配置块。v1：只是一张自由格式的 TOML 表，
    /// GUI 可将其传给插件的 `on_config` 宿主调用。
    #[serde(default)]
    pub config_schema: Option<toml::Value>,
    /// T-core-plugin-runtime v2：按插件启用 / 禁用。由 `pureterm plugin
    /// enable|disable <名称>` CLI 切换；缺省为 `false`（插件加载器把缺省
    /// 视为“用户尚未启用”）。
    #[serde(default)]
    pub enabled: bool,
}

fn default_abi_version() -> u32 {
    1
}

/// 可识别的 `provides = [...]` 条目。字符串经 `as_str` 序列化；
/// 加载器的解析器是其逆操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provides {
    /// 配置档（SSH / shell 配置档）提供者。
    ProfileProvider,
    /// 宿主应用贡献（菜单项、深链接等）。
    HostApp,
    /// GUI 框架中的工具栏按钮。
    ToolbarButtonProvider,
    /// 标签栏中的右键菜单项。
    TabContextMenuProvider,
    /// 配置文件提供者（例如远程配置后端）。
    ConfigProvider,
    /// 状态栏组件（渲染在状态栏中的控件）。
    StatusBarComponent,
    /// 侧边栏标签页（右侧边栏中的自定义页面）。
    SidebarTab,
    /// 命令面板条目（可模糊匹配的动作）。
    CommandPaletteItem,
    /// IPC 方法（自定义 JSON-RPC 端点）。
    IpcMethod,
    /// 终端解析器钩子（自定义 OSC/CSI/DCS 序列）。
    TerminalParserHook,
    /// 渲染器浮层（在终端网格之上的自定义绘制调用）。
    RendererOverlay,
}

impl Provides {
    /// 规范字符串形式（用于 `plugin.toml` 与 JSON 序列化）。
    pub fn as_str(&self) -> &'static str {
        match self {
            Provides::ProfileProvider => "profile_provider",
            Provides::HostApp => "host_app",
            Provides::ToolbarButtonProvider => "toolbar_button_provider",
            Provides::TabContextMenuProvider => "tab_context_menu_provider",
            Provides::ConfigProvider => "config_provider",
            Provides::StatusBarComponent => "status_bar_component",
            Provides::SidebarTab => "sidebar_tab",
            Provides::CommandPaletteItem => "command_palette_item",
            Provides::IpcMethod => "ipc_method",
            Provides::TerminalParserHook => "terminal_parser_hook",
            Provides::RendererOverlay => "renderer_overlay",
        }
    }

    /// 从规范字符串解析。未知值返回 `None`（向前兼容：v0 加载器跳过
    /// 未知的 provides 并给出警告，而不是中止）。
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "profile_provider" => Some(Provides::ProfileProvider),
            "host_app" => Some(Provides::HostApp),
            "toolbar_button_provider" => Some(Provides::ToolbarButtonProvider),
            "tab_context_menu_provider" => Some(Provides::TabContextMenuProvider),
            "config_provider" => Some(Provides::ConfigProvider),
            "status_bar_component" => Some(Provides::StatusBarComponent),
            "sidebar_tab" => Some(Provides::SidebarTab),
            "command_palette_item" => Some(Provides::CommandPaletteItem),
            "ipc_method" => Some(Provides::IpcMethod),
            "terminal_parser_hook" => Some(Provides::TerminalParserHook),
            "renderer_overlay" => Some(Provides::RendererOverlay),
            _ => None,
        }
    }
}

/// 插件可声明的全部入口点名称。v0 插件（无 `entry_points` 映射）得到
/// 空映射，并以惰性桩加载（v0 表面——`install_stub` / `list_plugins`
/// 只校验清单，从不调用插件）。
pub const PLUGIN_ENTRY_POINTS: &[&str] = &[
    "on_load",
    "on_unload",
    "on_config",
    "on_palette_command",
    "on_ipc_method",
    "on_status_bar_tick",
    "on_parser_osc",
    "on_render_overlay",
];

#[derive(Clone, Debug, PartialEq)]
pub struct PluginEntry {
    pub name: String,
    pub path: PathBuf,
    pub manifest: Option<PluginManifest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginError {
    pub plugin: String,
    pub reason: String,
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.plugin, self.reason)
    }
}

/// 对照文件夹名校验解析后的清单。集中实现，使 `load_manifest` 与
/// `install_stub` 共用同一套规则。
pub(crate) fn validate_manifest(folder_name: &str, m: &PluginManifest) -> Result<(), String> {
    if m.name != folder_name {
        return Err(format!(
            "manifest name \"{}\" does not match folder \"{}\"",
            m.name, folder_name
        ));
    }
    if m.name.is_empty() || m.name.contains('/') {
        return Err("plugin name must be non-empty and not contain '/'".into());
    }
    if m.version.is_empty() {
        return Err("version is required".into());
    }
    Ok(())
}

/// 从插件目录加载并校验清单。成功返回解析后的清单，失败返回
/// 描述问题的错误。
pub fn load_manifest(plugin_dir: &Path) -> Result<PluginManifest, String> {
    let folder_name = plugin_dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "plugin path has no folder name".to_string())?;
    let manifest_path = plugin_dir.join("plugin.toml");
    if !manifest_path.exists() {
        return Err(format!(
            "missing plugin.toml in {}",
            plugin_dir.display()
        ));
    }
    let raw = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let m: PluginManifest = toml::from_str(&raw).map_err(|e| e.to_string())?;
    validate_manifest(folder_name, &m)?;
    Ok(m)
}

/// 列出插件文件夹（每个子目录一个插件）并解析其清单。
/// 清单错误不会中止列出——而是把该插件以 `manifest = None` 返回，
/// 并收集一个 [`PluginError`]。
pub fn list_plugins(cfg: &PluginSystemConfig) -> Vec<PluginEntry> {
    list_plugins_with_errors(cfg).0
}

/// 与 [`list_plugins`] 相同，但还返回每个插件的清单错误。
/// 对 CLI / 设置界面有用——除有效条目外，还能暴露“坏清单”。
pub fn list_plugins_with_errors(
    cfg: &PluginSystemConfig,
) -> (Vec<PluginEntry>, Vec<PluginError>) {
    if !cfg.enabled {
        return (Vec::new(), Vec::new());
    }
    let root = cfg.plugin_dir();
    let Ok(read) = std::fs::read_dir(&root) else {
        return (Vec::new(), Vec::new());
    };
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for entry in read.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        match load_manifest(&path) {
            Ok(manifest) => entries.push(PluginEntry {
                name,
                path,
                manifest: Some(manifest),
            }),
            Err(reason) => errors.push(PluginError { plugin: name, reason }),
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    (entries, errors)
}

/// 安装桩插件：创建目录并写入最小化的 `plugin.toml` 清单。
/// 若清单已存在且解析成功，调用为空操作（幂等）。
pub fn install_stub(cfg: &PluginSystemConfig, name: &str) -> Result<PathBuf, String> {
    if !cfg.enabled {
        return Err("plugins disabled".into());
    }
    if name.is_empty() || name.contains('/') {
        return Err("invalid plugin name".into());
    }
    let dir = cfg.plugin_dir().join(name);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let manifest = dir.join("plugin.toml");
    if !manifest.exists() {
        let body = format!(
            "# GAP-49 plugin stub\nname = \"{name}\"\nversion = \"0.1.0\"\ndescription = \"\"\nauthor = \"\"\n"
        );
        std::fs::write(&manifest, body).map_err(|e| e.to_string())?;
    } else {
        // 校验已有清单仍然有效（防止“有人改了 plugin.toml 导致名称不匹配”）。
        load_manifest(&dir)?;
    }
    Ok(dir)
}

/// 通过删除目录来卸载插件。找不到清单时拒绝执行（防御性设计——
/// 我们不希望 `rm -rf` 任意文件夹）。
pub fn uninstall(cfg: &PluginSystemConfig, name: &str) -> Result<(), String> {
    if !cfg.enabled {
        return Err("plugins disabled".into());
    }
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err("invalid plugin name".into());
    }
    let dir = cfg.plugin_dir().join(name);
    // 目录必须看起来像插件目录：包含可解析且名称匹配的 plugin.toml。
    // 这可以防止拼写错误与 shell 通配符意外。
    load_manifest(&dir)?;
    std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())
}

/// T-core-plugin-runtime v2：就地翻转插件清单中的 `enabled` 标志。
/// 插件未安装或插件被全局禁用时返回 Err。
///
/// GUI 在下次启动时读取该标志；v3 将支持不重启即热重载清单。
pub fn set_enabled(cfg: &PluginSystemConfig, name: &str, enabled: bool) -> Result<(), String> {
    if !cfg.enabled {
        return Err("plugins disabled".into());
    }
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err("invalid plugin name".into());
    }
    let manifest_path = cfg.plugin_dir().join(name).join("plugin.toml");
    if !manifest_path.exists() {
        return Err(format!("plugin not installed: {name}"));
    }
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("parse {}: {e}", manifest_path.display()))?;
    // 在文档根处设置 `enabled = true|false`。还没有 `[enabled]` 标志的
    // 插件会被创建该标志。
    doc["enabled"] = toml_edit::value(enabled);
    std::fs::write(&manifest_path, doc.to_string())
        .map_err(|e| format!("write {}: {e}", manifest_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled_cfg(dir: &Path) -> PluginSystemConfig {
        PluginSystemConfig {
            enabled: true,
            directory: Some(dir.to_path_buf()),
        }
    }

    #[test]
    fn list_empty_when_disabled() {
        assert!(list_plugins(&PluginSystemConfig::default()).is_empty());
    }

    #[test]
    fn list_empty_when_dir_missing() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_does_not_exist_xyz");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        assert!(list_plugins(&cfg).is_empty());
    }

    #[test]
    fn install_creates_manifest_that_loads() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_install_test");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        let dir = install_stub(&cfg, "hello").unwrap();
        let m = load_manifest(&dir).unwrap();
        assert_eq!(m.name, "hello");
        assert_eq!(m.version, "0.1.0");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_rejects_empty_name() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_install_invalid");
        let cfg = enabled_cfg(&tmp);
        assert!(install_stub(&cfg, "").is_err());
        assert!(install_stub(&cfg, "a/b").is_err());
    }

    /// T-core-plugin-runtime v2：`enable` / `disable` 翻转插件 `plugin.toml`
    /// 中的 `enabled` 标志。翻转后重新读取清单即可看到新状态。
    #[test]
    fn set_enabled_flips_manifest_flag() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_set_enabled_test");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        let dir = install_stub(&cfg, "toggle").unwrap();
        // 默认状态：无 enabled 标志（读取时默认为 false）。
        let m_before = load_manifest(&dir).unwrap();
        assert_ne!(m_before.enabled, true, "stub installs with no enabled flag");

        // 翻转为 true。
        set_enabled(&cfg, "toggle", true).expect("enable");
        let m_on = load_manifest(&dir).unwrap();
        assert!(m_on.enabled, "after enable, flag must be true");

        // 翻回 false。
        set_enabled(&cfg, "toggle", false).expect("disable");
        let m_off = load_manifest(&dir).unwrap();
        assert!(!m_off.enabled, "after disable, flag must be false");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 对缺失的插件设置 enabled 会得到 Err。
    #[test]
    fn set_enabled_rejects_unknown_plugin() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_set_enabled_unknown");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        let err = set_enabled(&cfg, "ghost", true).unwrap_err();
        assert!(err.contains("not installed"), "must report not installed: {err}");
    }

    /// `set_enabled` 与插件 API 的其他部分一样，拒绝空名称 / 路径穿越名称。
    #[test]
    fn set_enabled_rejects_invalid_names() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_set_enabled_invalid");
        let cfg = enabled_cfg(&tmp);
        assert!(set_enabled(&cfg, "", true).is_err());
        assert!(set_enabled(&cfg, "a/b", true).is_err());
        assert!(set_enabled(&cfg, "..", true).is_err());
    }

    /// 全局禁用插件时，每插件的开关同样被阻止。
    #[test]
    fn set_enabled_rejects_when_plugins_globally_disabled() {
        let cfg = PluginSystemConfig::default();
        assert!(set_enabled(&cfg, "any", true).is_err());
    }

    #[test]
    fn install_rejects_when_disabled() {
        let cfg = PluginSystemConfig::default();
        assert!(install_stub(&cfg, "hello").is_err());
    }

    #[test]
    fn list_reports_manifest_errors_separately() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_mixed");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        // 好插件
        install_stub(&cfg, "good").unwrap();
        // 坏插件：文件夹存在，但没有 plugin.toml
        std::fs::create_dir_all(tmp.join("broken")).unwrap();
        let (entries, errors) = list_plugins_with_errors(&cfg);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "good");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].plugin, "broken");
        assert!(errors[0].reason.contains("missing plugin.toml"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn manifest_name_mismatch_is_rejected() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_mismatch");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        let dir = install_stub(&cfg, "alpha").unwrap();
        // 用与文件夹名不匹配的清单覆盖。
        std::fs::write(
            dir.join("plugin.toml"),
            "name = \"beta\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let res = load_manifest(&dir);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("does not match folder"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn uninstall_removes_directory() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_uninstall");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        install_stub(&cfg, "to_remove").unwrap();
        assert!(tmp.join("to_remove").exists());
        uninstall(&cfg, "to_remove").unwrap();
        assert!(!tmp.join("to_remove").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn uninstall_refuses_non_plugin_dir() {
        let tmp = std::env::temp_dir().join("pureterm_plugins_uninstall_safe");
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = enabled_cfg(&tmp);
        // 文件夹存在但没有 plugin.toml
        std::fs::create_dir_all(tmp.join("not_a_plugin")).unwrap();
        let res = uninstall(&cfg, "not_a_plugin");
        assert!(res.is_err());
        // 文件夹应仍然存在
        assert!(tmp.join("not_a_plugin").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ---- t-core-plugin-runtime：新清单字段 ----

    #[test]
    fn v0_manifest_parses_with_default_abi() {
        let raw = "name = \"old\"\nversion = \"0.1.0\"\n";
        let m: PluginManifest = toml::from_str(raw).expect("parse v0");
        assert_eq!(m.abi_version, 1);
        assert!(m.entry_points.is_empty());
        assert!(m.provides.is_empty());
        assert!(m.config_schema.is_none());
    }

    #[test]
    fn v1_manifest_parses_with_provides_and_entries() {
        let raw = r#"
name = "status-cpu"
version = "0.2.0"
description = "Status-bar CPU monitor"
abi_version = 1
entry_points = { on_load = "load", on_status_bar_tick = "tick" }
provides = ["status_bar_component", "command_palette_item"]
config_schema = { interval_ms = 1000 }
"#;
        let m: PluginManifest = toml::from_str(raw).expect("parse v1");
        assert_eq!(m.abi_version, 1);
        assert_eq!(m.entry_points.get("on_load"), Some(&"load".to_string()));
        assert_eq!(m.entry_points.get("on_status_bar_tick"), Some(&"tick".to_string()));
        assert_eq!(m.provides, vec!["status_bar_component", "command_palette_item"]);
        assert!(m.config_schema.is_some());
    }

    #[test]
    fn provides_str_round_trip() {
        for p in [
            Provides::ProfileProvider,
            Provides::HostApp,
            Provides::ToolbarButtonProvider,
            Provides::TabContextMenuProvider,
            Provides::ConfigProvider,
            Provides::StatusBarComponent,
            Provides::SidebarTab,
            Provides::CommandPaletteItem,
            Provides::IpcMethod,
            Provides::TerminalParserHook,
            Provides::RendererOverlay,
        ] {
            let s = p.as_str();
            let back = Provides::from_str(s).expect("parse known");
            assert_eq!(back, p);
        }
    }

    #[test]
    fn provides_from_str_unknown_is_none() {
        // 向前兼容：未知的 provides 值是空操作（GUI 记录警告并继续加载插件）。
        assert!(Provides::from_str("future_thing").is_none());
        assert!(Provides::from_str("").is_none());
    }

    #[test]
    fn plugin_entry_points_contains_v1_set() {
        // 对文档化的入口点做合理性检查；渲染器与 IPC 代码路径按这些名称分发。
        let v: Vec<&&str> = PLUGIN_ENTRY_POINTS.iter().collect();
        assert!(v.contains(&&"on_load"));
        assert!(v.contains(&&"on_unload"));
        assert!(v.contains(&&"on_palette_command"));
        assert!(v.contains(&&"on_ipc_method"));
    }
}
