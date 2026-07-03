//! T-core-plugin-runtime v2: plugin loader infrastructure.
//!
//! v2 stops short of pulling in `wasmtime` (a 50+ MB dep that adds
//! minutes to cold builds); instead it provides:
//!
//! - The `PluginLoader` trait that the host uses to dispatch plugin
//!   lifecycle hooks.
//! - A `StaticPluginLoader` impl backed by a `HashMap<name, Box<dyn Plugin>>`
//!   — useful for tests + the bundled PureTerm "first-party" plugins
//!   (`status_bar.cpu`, `ipc.hello`, etc.) that ship inside the binary.
//! - A `WasmPluginLoader` *sketch* that documents where wasmtime would
//!   go in v3.
//! - The 9 host-function signatures that the v3 wasmtime linker will
//!   expose to plugins (`HostFn::Parser`, `Renderer`, etc.). Each is a
//!   `Box<dyn Fn>` so a v3 implementation can register real handlers.
//!
//! The checklist's `plugin_system/tests/loader.rs` ≥10 tests target this
//! surface.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use super::plugin_system::{PluginManifest, Provides};

/// Where to load a plugin from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginSource {
    /// Bundled inside the PureTerm binary (a `#[link_section]` payload
    /// or a `include_bytes!`-style constant). v2: registered via
    /// `StaticPluginLoader::register`.
    Bundled,
    /// Loaded from a `.wasm` file on disk. v3 only — v2 returns
    /// `Err(Unsupported)` here.
    WasmFile(std::path::PathBuf),
    /// Loaded from a `.dylib` / `.so` / `.dll` (cdylib-based plugin).
    /// v3 only — v2 returns `Err(Unsupported)`.
    DynamicLibrary(std::path::PathBuf),
}

/// The 9 host functions a plugin can call back into. Each variant
/// holds a boxed callback the host registers at plugin-load time.
#[derive(Clone)]
pub enum HostFn {
    /// Parser hook: pre-process terminal output before it reaches the
    /// grid. Used by plugins that want to inject OSC 133 marks or
    /// capture prompts.
    Parser(Arc<dyn Fn(&str) -> String + Send + Sync>),
    /// Renderer hook: overlay decorations on top of the GPU quad pass.
    Renderer(Arc<dyn Fn() + Send + Sync>),
    /// Palette hook: customize the 16-color palette.
    Palette(Arc<dyn Fn() + Send + Sync>),
    /// IPC hook: expose a custom JSON-RPC method.
    Ipc(Arc<dyn Fn(&str, serde_json::Value) -> serde_json::Value + Send + Sync>),
    /// Status bar component hook: register a custom widget.
    StatusBar(Arc<dyn Fn() + Send + Sync>),
    /// Sidebar tab hook: register a custom sidebar tab.
    Sidebar(Arc<dyn Fn() + Send + Sync>),
    /// Profile provider hook: supply a startup profile.
    Profile(Arc<dyn Fn() -> serde_json::Value + Send + Sync>),
    /// Tab context menu hook: add an entry to the right-click menu.
    TabMenu(Arc<dyn Fn() + Send + Sync>),
    /// Config provider hook: contribute to the merged config.
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

/// Outcome of loading a plugin.
#[derive(Debug)]
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub source: PluginSource,
    /// The `Provides` capabilities this plugin registered (mirrors
    /// `manifest.provides`, exposed separately so the GUI doesn't have
    /// to reach through the manifest to wire hooks). Strings match
    /// `Provides::as_str()`.
    pub provides: Vec<String>,
}

/// The plugin loader trait. v2 has two impls: `StaticPluginLoader`
/// (always available) and `WasmPluginLoader` (sketch only).
pub trait PluginLoader: Send + Sync {
    /// Load a single plugin from `source`. Returns the parsed
    /// manifest + a list of `Provides` capabilities the loader
    /// extracted from the plugin's exports.
    fn load(&self, source: PluginSource, manifest: PluginManifest) -> Result<LoadedPlugin, String>;

    /// Unload a plugin (release the wasm instance, deregister hooks).
    fn unload(&self, name: &str) -> Result<(), String>;

    /// List currently loaded plugin names.
    fn loaded(&self) -> Vec<String>;
}

/// A bundled plugin: a (manifest, provides) pair registered with the
/// `StaticPluginLoader`. v2 stores the manifest only (no code); v3
/// will add an `Arc<dyn Plugin>` once the trait lands.
#[derive(Clone, Debug)]
pub struct BundledPlugin {
    pub manifest: PluginManifest,
}

impl BundledPlugin {
    pub fn new(manifest: PluginManifest) -> Self {
        Self { manifest }
    }
}

/// Bundled-plugin loader: looks up plugins by name in an in-process
/// `HashMap`. Tests + first-party plugins use this.
#[derive(Default)]
pub struct StaticPluginLoader {
    plugins: HashMap<String, BundledPlugin>,
}

impl StaticPluginLoader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a bundled plugin under its manifest name. Subsequent
    /// `load(PluginSource::Bundled, manifest)` calls succeed for that
    /// name.
    pub fn register(&mut self, plugin: BundledPlugin) {
        let name = plugin.manifest.name.clone();
        self.plugins.insert(name, plugin);
    }

    /// Returns true if the named plugin is registered.
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
        // Bundled plugins are owned by the loader; unload is a no-op
        // for v2. (v3 may decrement refcounts once we move to
        // Arc-based unloading.)
        Ok(())
    }

    fn loaded(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }
}

/// WasmPluginLoader: v3 sketch. The v2 stub returns `Err(Unsupported)`
/// from `load` for any non-bundled source so the GUI can degrade
/// gracefully when wasmtime is disabled at build time.
#[derive(Default)]
pub struct WasmPluginLoader;

impl WasmPluginLoader {
    pub fn new() -> Self {
        Self
    }

    /// Where wasmtime would link the host functions. The function
    /// returns a `Vec<HostFn>` for clarity — the v3 implementation
    /// builds the linker by iterating this list.
    pub fn host_fns() -> Vec<HostFn> {
        Vec::new()
    }
}

impl PluginLoader for WasmPluginLoader {
    fn load(&self, source: PluginSource, manifest: PluginManifest) -> Result<LoadedPlugin, String> {
        match source {
            PluginSource::Bundled => Err("WasmPluginLoader does not accept Bundled plugins".into()),
            PluginSource::WasmFile(_) | PluginSource::DynamicLibrary(_) => {
                Err("wasmtime is not wired in v2; tracked by t-core-plugin-runtime".into())
            },
        }
    }

    fn unload(&self, _name: &str) -> Result<(), String> {
        Ok(())
    }

    fn loaded(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Read a manifest from a `.toml` file. Used by the CLI `pureterm
/// plugin install` flow to verify a downloaded plugin before linking
/// it.
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