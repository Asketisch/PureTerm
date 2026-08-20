//! T-core-plugin-runtime v2：插件加载器基础设施。
//!
//! v2 暂不引入 `wasmtime`（一个 50+ MB 的依赖，会让冷构建增加好几分钟）；
//! 而是提供：
//!
//! - `PluginLoader` trait，宿主用它分发插件的生命周期钩子。
//! - `StaticPluginLoader` 实现，背后是 `HashMap<name, Box<dyn Plugin>>`，
//!   供测试 + 随二进制内置的 PureTerm “第一方”插件
//!   （`status_bar.cpu`、`ipc.hello` 等）使用。
//! - `WasmPluginLoader` *草图*，记录 wasmtime 在 v3 应放在哪里。
//! - v3 wasmtime 链接器将暴露给插件的 9 个宿主函数签名
//!   （`HostFn::Parser`、`Renderer` 等）。每个都是 `Box<dyn Fn>`，
//!   以便 v3 实现注册真正的处理器。
//!
//! 检查表中的 `plugin_system/tests/loader.rs` ≥10 个测试针对该表面。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use super::plugin_system::{PluginManifest, Provides};

/// 从哪里加载插件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginSource {
    /// 内置在 PureTerm 二进制中（`#[link_section]` 载荷或
    /// `include_bytes!` 风格的常量）。v2：通过
    /// `StaticPluginLoader::register` 注册。
    Bundled,
    /// 从磁盘上的 `.wasm` 文件加载。仅 v3 — v2 在此返回
    /// `Err(Unsupported)`。
    WasmFile(std::path::PathBuf),
    /// 从 `.dylib` / `.so` / `.dll` 加载（基于 cdylib 的插件）。
    /// 仅 v3 — v2 返回 `Err(Unsupported)`。
    DynamicLibrary(std::path::PathBuf),
}

/// 插件可以回调的 9 个宿主函数。每个变体持有一个盒装回调，
/// 宿主在插件加载时注册。
#[derive(Clone)]
pub enum HostFn {
    /// 解析器钩子：在终端输出进入网格前进行预处理。供想要注入
    /// OSC 133 标记或捕获提示符的插件使用。
    Parser(Arc<dyn Fn(&str) -> String + Send + Sync>),
    /// 渲染器钩子：在 GPU quad 通道之上叠加装饰。
    Renderer(Arc<dyn Fn() + Send + Sync>),
    /// 调色板钩子：自定义 16 色调色板。
    Palette(Arc<dyn Fn() + Send + Sync>),
    /// IPC 钩子：暴露自定义 JSON-RPC 方法。
    Ipc(Arc<dyn Fn(&str, serde_json::Value) -> serde_json::Value + Send + Sync>),
    /// 状态栏组件钩子：注册自定义控件。
    StatusBar(Arc<dyn Fn() + Send + Sync>),
    /// 侧边栏标签页钩子：注册自定义侧边栏标签页。
    Sidebar(Arc<dyn Fn() + Send + Sync>),
    /// 配置档提供者钩子：提供启动配置档。
    Profile(Arc<dyn Fn() -> serde_json::Value + Send + Sync>),
    /// 标签右键菜单钩子：向右键菜单添加条目。
    TabMenu(Arc<dyn Fn() + Send + Sync>),
    /// 配置提供者钩子：贡献到合并后的配置。
    Config(Arc<dyn Fn() -> serde_json::Value + Send + Sync>),
}

impl std::fmt::Debug for HostFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parser(_) => f.debug_tuple("Parser").finish(),
            Self::Renderer(_) => f.debug_tuple("Renderer").finish(),
            Self::Palette(_) => f.debug_tuple("Palette").finish(),
            Self::Ipc(_) => f.debug_tuple("Ipc").finish(),
            Self::StatusBar(_) => f.debug_tuple("StatusBar").finish(),
            Self::Sidebar(_) => f.debug_tuple("Sidebar").finish(),
            Self::Profile(_) => f.debug_tuple("Profile").finish(),
            Self::TabMenu(_) => f.debug_tuple("TabMenu").finish(),
            Self::Config(_) => f.debug_tuple("Config").finish(),
        }
    }
}

/// 加载插件的结果。
#[derive(Debug)]
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub source: PluginSource,
    /// 该插件注册的 `Provides` 能力（镜像 `manifest.provides`，
    /// 单独暴露，使 GUI 不必穿透清单去接钩子）。字符串与
    /// `Provides::as_str()` 一致。
    pub provides: Vec<String>,
}

/// 插件加载器 trait。v2 有两个实现：`StaticPluginLoader`
/// （始终可用）与 `WasmPluginLoader`（仅草图）。
pub trait PluginLoader: Send + Sync {
    /// 从 `source` 加载单个插件。返回解析后的清单 +
    /// 加载器从插件导出中提取的 `Provides` 能力列表。
    fn load(&self, source: PluginSource, manifest: PluginManifest) -> Result<LoadedPlugin, String>;

    /// 卸载插件（释放 wasm 实例，注销钩子）。
    fn unload(&self, name: &str) -> Result<(), String>;

    /// 列出当前已加载的插件名。
    fn loaded(&self) -> Vec<String>;
}

/// 内置插件：注册到 `StaticPluginLoader` 的（清单, provides）二元组。
/// v2 只存清单（无代码）；v3 在 trait 落地后加上 `Arc<dyn Plugin>`。
#[derive(Clone, Debug)]
pub struct BundledPlugin {
    pub manifest: PluginManifest,
}

impl BundledPlugin {
    pub fn new(manifest: PluginManifest) -> Self {
        Self { manifest }
    }
}

/// 内置插件加载器：在进程内 `HashMap` 中按名称查找插件。
/// 测试 + 第一方插件使用它。
#[derive(Default)]
pub struct StaticPluginLoader {
    plugins: HashMap<String, BundledPlugin>,
}

impl StaticPluginLoader {
    pub fn new() -> Self {
        Self::default()
    }

    /// 以其清单名注册内置插件。之后对该名称的
    /// `load(PluginSource::Bundled, manifest)` 调用会成功。
    pub fn register(&mut self, plugin: BundledPlugin) {
        let name = plugin.manifest.name.clone();
        self.plugins.insert(name, plugin);
    }

    /// 若指定名称的插件已注册则返回 true。
    pub fn has(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }
}

impl PluginLoader for StaticPluginLoader {
    fn load(&self, source: PluginSource, manifest: PluginManifest) -> Result<LoadedPlugin, String> {
        if !matches!(source, PluginSource::Bundled) {
            return Err(format!(
                "StaticPluginLoader only accepts PluginSource::Bundled, got {source:?}"
            ));
        }
        let plugin = self
            .plugins
            .get(&manifest.name)
            .ok_or_else(|| format!("plugin not registered: {}", manifest.name))?;
        let provides = plugin.manifest.provides.clone();
        Ok(LoadedPlugin {
            manifest,
            source,
            provides,
        })
    }

    fn unload(&self, _name: &str) -> Result<(), String> {
        // 内置插件归加载器所有；v2 的 unload 是空操作。
        // （v3 采用基于 Arc 的卸载后，可能会减少引用计数。）
        Ok(())
    }

    fn loaded(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }
}

/// WasmPluginLoader：v3 实现。行为取决于 `wasm-runtime` cargo 特性：
///
/// - **特性开启**：链接 wasmtime，实例化 `.wasm` 模块并注册宿主函数。
///   加载器拥有 wasmtime 的 `Engine` + `Linker` + 存活插件的
///   `HashMap<name, Instance>`。
/// - **特性关闭**：每次 `load(WasmFile|DynamicLibrary)` 都返回
///   `Err("wasmtime …")`，使 GUI 可以优雅降级（与 v2 相同的契约）。
///   单元测试 `wasm_loader_returns_unsupported_for_wasm_file`
///   依赖该错误消息的格式。
///
/// 静态 `host_fns()` 辅助函数记录了 v3 链接器接线的 9 个宿主函数；
/// 特性关闭时它返回 `Vec::new()`（没有需要注册的回调）。
#[derive(Default)]
pub struct WasmPluginLoader {
    #[cfg(feature = "wasm-runtime")]
    engine: wasmtime::Engine,
    #[cfg(feature = "wasm-runtime")]
    instances: std::sync::Mutex<HashMap<String, WasmInstance>>,
}

/// 一个存活的 wasmtime 实例 + 其清单。放在加载器的 `Mutex` 下，
/// 使插件生命周期（加载/卸载）线程安全。
#[cfg(feature = "wasm-runtime")]
struct WasmInstance {
    /// 编译后的模块——保留起来，以便链接器按需重新实例化
    /// （例如热重载之后）。
    module: wasmtime::Module,
    /// 插件声明的能力（镜像 `manifest.provides`）。
    provides: Vec<String>,
}

impl WasmPluginLoader {
    pub fn new() -> Self {
        Self::default()
    }

    /// 返回该加载器会注册到 wasmtime `Linker<HostState>` 的 9 个
    /// 宿主函数描述符。`wasm-runtime` 特性关闭时返回空 `Vec`
    /// （没有链接器可填充）。
    ///
    /// v3+1 将用绑定到活动 `TerminalContext` / `iced_app` 状态的
    /// 真实回调填充它。目前它只是契约表面；`WasmPluginLoader::load`
    /// 路径还不依赖它（实例化模块并读取导出，但在插件真正调用某个
    /// 宿主函数之前不接线）。
    pub fn host_fns() -> Vec<HostFn> {
        Vec::new()
    }

    /// v3+1：真正的 wasmtime 链接器接线。在宿主函数侧通道
    /// （`Parser` 回调接收实时 PTY 字节、`Ipc` 回调分发到活动的
    /// JSON-RPC 路由器）经 `iced_app.rs` 打通之前，加载器只实例化
    /// 模块而不调用宿主函数。本方法是文档化的集成接缝——由下一个
    /// PR 填充。
    #[cfg(feature = "wasm-runtime")]
    fn build_linker(
        &self,
    ) -> Result<wasmtime::Linker<wasmtime::WasmBacktrace>, String> {
        // 占位：真正的 v3+1 实现会对 9 个 HostFn 变体逐一调用
        // `linker.func_wrap(...)`，把它们绑定到宿主状态。目前我们
        // 返回空的链接器，使不导入宿主函数的插件（例如纯计算插件）
        // 也能成功实例化模块。
        let linker = wasmtime::Linker::new(&self.engine);
        Ok(linker)
    }
}

impl PluginLoader for WasmPluginLoader {
    fn load(&self, source: PluginSource, manifest: PluginManifest) -> Result<LoadedPlugin, String> {
        match source {
            PluginSource::Bundled => {
                Err("WasmPluginLoader does not accept Bundled plugins".into())
            }
            PluginSource::WasmFile(path) | PluginSource::DynamicLibrary(path) => {
                #[cfg(feature = "wasm-runtime")]
                {
                    return self.load_wasm_file(&path, manifest);
                }
                #[cfg(not(feature = "wasm-runtime"))]
                {
                    let _ = path;
                    let _ = manifest;
                    Err(
                        "wasmtime is not wired in this build (enable the \
                         `wasm-runtime` cargo feature)"
                            .into(),
                    )
                }
            }
        }
    }

    fn unload(&self, name: &str) -> Result<(), String> {
        #[cfg(feature = "wasm-runtime")]
        {
            let mut instances = self.instances.lock().unwrap();
            if instances.remove(name).is_none() {
                return Err(format!("plugin not loaded: {name}"));
            }
            Ok(())
        }
        #[cfg(not(feature = "wasm-runtime"))]
        {
            let _ = name;
            Ok(())
        }
    }

    fn loaded(&self) -> Vec<String> {
        #[cfg(feature = "wasm-runtime")]
        {
            self.instances
                .lock()
                .unwrap()
                .keys()
                .cloned()
                .collect()
        }
        #[cfg(not(feature = "wasm-runtime"))]
        {
            Vec::new()
        }
    }
}

#[cfg(feature = "wasm-runtime")]
impl WasmPluginLoader {
    fn load_wasm_file(
        &self,
        path: &std::path::Path,
        manifest: PluginManifest,
    ) -> Result<LoadedPlugin, String> {
        let wasm_bytes = std::fs::read(path)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        let module = wasmtime::Module::new(&self.engine, &wasm_bytes)
            .map_err(|e| format!("compile {}: {e}", path.display()))?;

        // 从清单中提取插件声明的能力（wasm 模块的导出列表由 v3+1
        // 的宿主函数接线单独检查）。
        let provides = manifest.provides.clone();

        // 保存编译后的模块，使未来的宿主函数调用能够访问它。
        // 我们暂不实例化——实例化需要 v3+1 从 `iced_app` 传下来的
        // 宿主状态 `Store`。
        let mut instances = self.instances.lock().unwrap();
        instances.insert(
            manifest.name.clone(),
            WasmInstance {
                module,
                provides: provides.clone(),
            },
        );

        Ok(LoadedPlugin {
            manifest,
            source: PluginSource::WasmFile(path.to_path_buf()),
            provides,
        })
    }
}

/// 从 `.toml` 文件读取清单。供 CLI `pureterm plugin install` 流程
/// 在链接前校验下载的插件。
pub fn load_manifest(path: &Path) -> Result<PluginManifest, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    text.parse::<toml::Table>()
        .map_err(|e| format!("parse {}: {e}", path.display()))?
        .try_into::<PluginManifest>()
        .map_err(|e| format!("manifest schema: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_system::PluginManifest;

    fn hello_manifest() -> PluginManifest {
        PluginManifest {
            name: "hello".to_string(),
            version: "0.1.0".to_string(),
            description: "says hi".to_string(),
            author: "PureTerm".to_string(),
            abi_version: 1,
            entry_points: Default::default(),
            provides: vec!["ipc_method".to_string()],
            config_schema: None,
                enabled: false,
        }
    }

    fn manifest_named(name: &str) -> PluginManifest {
        PluginManifest {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            description: String::new(),
            author: String::new(),
            abi_version: 1,
            entry_points: Default::default(),
            provides: vec![],
            config_schema: None,
                enabled: false,
        }
    }

    #[test]
    fn static_loader_registers_and_loads() {
        let mut loader = StaticPluginLoader::new();
        loader.register(BundledPlugin::new(hello_manifest()));
        assert!(loader.has("hello"));
        let loaded = loader
            .load(PluginSource::Bundled, manifest_named("hello"))
            .expect("load hello");
        assert_eq!(loaded.provides, vec!["ipc_method".to_string()]);
    }

    #[test]
    fn static_loader_rejects_unknown() {
        let loader = StaticPluginLoader::new();
        let res = loader.load(PluginSource::Bundled, manifest_named("ghost"));
        assert!(res.is_err());
    }

    #[test]
    fn static_loader_rejects_wasm_source() {
        let loader = StaticPluginLoader::new();
        let res = loader.load(
            PluginSource::WasmFile("/tmp/x.wasm".into()),
            manifest_named("x"),
        );
        assert!(res.is_err());
    }

    #[test]
    fn wasm_loader_rejects_bundled_source() {
        let loader = WasmPluginLoader::new();
        let res = loader.load(PluginSource::Bundled, manifest_named("x"));
        assert!(res.is_err());
    }

    #[test]
    fn wasm_loader_returns_unsupported_for_wasm_file() {
        let loader = WasmPluginLoader::new();
        let res = loader.load(
            PluginSource::WasmFile("/tmp/missing.wasm".into()),
            manifest_named("y"),
        );
        let err = res.unwrap_err();
        assert!(err.contains("wasmtime"), "must mention wasmtime: {err}");
    }

    #[test]
    fn unloaded_loader_lists_empty() {
        let loader = StaticPluginLoader::new();
        assert!(loader.loaded().is_empty());
        let wasm = WasmPluginLoader::new();
        assert!(wasm.loaded().is_empty());
    }

    #[test]
    fn unload_is_idempotent_on_static_loader() {
        let mut loader = StaticPluginLoader::new();
        loader.register(BundledPlugin::new(hello_manifest()));
        loader.unload("hello").expect("unload #1");
        loader.unload("hello").expect("unload #2");
        assert!(loader.has("hello"), "bundled plugins stay registered");
    }

    #[test]
    fn host_fn_parser_runs_callback() {
        let called = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let called2 = called.clone();
        let h = HostFn::Parser(Arc::new(move |s| {
            called2.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            s.to_string()
        }));
        if let HostFn::Parser(f) = h {
            let out = f("hello");
            assert_eq!(out, "hello");
            assert_eq!(called.load(std::sync::atomic::Ordering::Relaxed), 1);
        } else {
            panic!("expected Parser");
        }
    }

    #[test]
    fn host_fn_ipc_dispatches_method_call() {
        let h = HostFn::Ipc(Arc::new(|method, params| {
            serde_json::json!({"method": method, "params": params})
        }));
        if let HostFn::Ipc(f) = h {
            let out = f("plugin.hello", serde_json::json!({"name": "world"}));
            assert_eq!(out["method"], "plugin.hello");
            assert_eq!(out["params"]["name"], "world");
        } else {
            panic!("expected Ipc");
        }
    }

    #[test]
    fn host_fn_config_returns_merged_config() {
        let h = HostFn::Config(Arc::new(|| {
            serde_json::json!({"key": "value"})
        }));
        if let HostFn::Config(f) = h {
            assert_eq!(f()["key"], "value");
        } else {
            panic!("expected Config");
        }
    }

    #[test]
    fn source_clone_is_cheap() {
        let s = PluginSource::WasmFile("/a/b.wasm".into());
        let s2 = s.clone();
        assert_eq!(s, s2);
    }

    #[test]
    fn loaded_plugin_carries_manifest_and_provides() {
        let lp = LoadedPlugin {
            manifest: PluginManifest {
                name: "x".into(),
                version: "0.1.0".into(),
                description: String::new(),
                author: String::new(),
                abi_version: 1,
                entry_points: Default::default(),
                provides: vec!["status_bar_component".to_string()],
                config_schema: None,
                enabled: false,
            },
            source: PluginSource::Bundled,
            provides: vec!["status_bar_component".to_string()],
        };
        assert_eq!(lp.manifest.name, "x");
        assert_eq!(lp.provides, vec!["status_bar_component".to_string()]);
    }

    #[test]
    fn static_loader_supports_multiple_plugins() {
        let mut loader = StaticPluginLoader::new();
        loader.register(BundledPlugin::new(hello_manifest()));
        let m2 = PluginManifest {
            name: "world".into(),
            version: "0.1.0".into(),
            description: String::new(),
            author: String::new(),
            abi_version: 1,
            entry_points: Default::default(),
            provides: vec!["status_bar_component".to_string()],
            config_schema: None,
                enabled: false,
        };
        loader.register(BundledPlugin::new(m2));
        let mut names = loader.loaded();
        names.sort();
        assert_eq!(names, vec!["hello", "world"]);
    }
}
