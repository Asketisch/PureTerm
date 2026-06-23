//! GAP-3.5: 浏览器面板上下文(Web pane 的内容载体)。
//!
//! 持有一个 wry `WebView`,作为 iced content NSView 的子视图嵌入(macOS)。
//! iced 0.14 daemon 不向应用代码暴露 `&Window`,故无法走文档化的
//! `HasWindowHandle` —— 改由 [`crate::browser::platform`] 在主线程 objc2
//! walk 拿到 content NSView,这里把 wry `WKWebView` 作为它的子视图挂上去。
//!
//! 几何真值是 `split_tree` 的 layout(终端 grid resize 也用同一 layout),
//! 故 reposition pass 直接拿 pane 的 `PaneRect` 喂 `set_frame`,无需在 iced
//! Widget 树里报告 bounds(那在 daemon 模式下很脆)。
//!
//! 所有方法取 `&self`(内部可变性),便于在不可变遍历 tabs/panes 时调用。

use std::sync::Arc;

use parking_lot::Mutex;
use parking_lot::RwLock;

/// 浏览器上下文的唯一标识。
///
/// 与 `PaneId` 独立命名空间:一个 web pane 同时拥有 `PaneId`(在分割树中
/// 的位置)和 `BrowserContextId`(其内容载体)。当前仅为内部追踪用,
/// 不参与持久化(快照只存 `url`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BrowserContextId(pub u64);

impl BrowserContextId {
    /// 全局自增计数器生成新的 id。
    pub fn next() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

/// Web pane 的内容载体。
///
/// `webview` 用 `Mutex<Option<_>>` 而非 `OnceCell`:关 pane 时要能 `take`
/// 掉 wry `WebView`(防 WKWebView 进程泄漏),`OnceCell` 一旦写入无法清空。
pub struct BrowserContext {
    pub id: BrowserContextId,
    url: RwLock<String>,
    title: RwLock<String>,
    webview: Mutex<Option<wry::WebView>>,
    /// Phase C: 绑定的 SSH 隧道 id(GAP-5.2);`None` = 直连。仅标识用 ——
    /// realize 时真正消费的是 `proxy`(解析后的 SOCKS5 端点)。daemon 侧隧道 id
    /// 是字符串(`new_tunnel_id()` 生成),故这里是 `Option<String>`。`Mutex` 包
    /// 一层是为了让 `set_tunnel`/`clear_tunnel` 能在 `&self` 下变更(模块约定所有
    /// 方法取 `&self`,便于在不可变遍历 tabs/panes 时经 `Arc` 调用)。
    pub tunnel_id: Mutex<Option<String>>,
    /// Phase C: 该 webview 应走的 SOCKS5 代理端点(`(host, port)`,通常
    /// `127.0.0.1:<daemon 隧道 local_port>`)。`None` = 直连。仅 macOS
    /// realize 时通过 wry `with_proxy_config` 消费(mac-proxy feature);其它
    /// 平台忽略(Phase A 嵌入本就 macOS-only)。代理是 build-time 配置,故
    /// 绑定/解绑隧道后要重建 webview(见 [`BrowserContext::set_tunnel`])。
    proxy: Mutex<Option<(String, u16)>>,
}

impl BrowserContext {
    /// 创建一个新的浏览器上下文(尚未 realize wry WebView)。
    pub fn new(url: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            id: BrowserContextId::next(),
            url: RwLock::new(url.into()),
            title: RwLock::new(String::new()),
            webview: Mutex::new(None),
            tunnel_id: Mutex::new(None),
            proxy: Mutex::new(None),
        })
    }

    /// 当前加载(或正在加载)的 URL。
    pub fn url(&self) -> String {
        self.url.read().clone()
    }

    /// 当前页面标题(加载完成前为空串)。
    pub fn title(&self) -> String {
        self.title.read().clone()
    }

    /// 由 wry 页面标题回调更新。
    pub fn set_title(&self, title: impl Into<String>) {
        *self.title.write() = title.into();
    }

    /// wry WebView 是否已构造(realize 过)。
    pub fn is_realized(&self) -> bool {
        self.webview.lock().is_some()
    }

    /// 在主线程惰性构造 wry WebView。已构造则 no-op。失败仅 log —— 下一帧
    /// reposition pass 会重试。
    ///
    /// 平台:Phase A 仅 macOS 真嵌入(取 content NSView);其它平台走
    /// open-in-browser fallback(待后续)。
    pub fn try_realize(&self) {
        #[cfg(target_os = "macos")]
        {
            if let Some(handle) = crate::browser::platform::active_ns_view() {
                self.realize_macos(&handle);
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Phase A: macOS-only embed。Linux/Windows 走 open-in-browser 兜底。
            log::debug!("browser: webview embed is macOS-only in Phase A (url={})", self.url());
        }
    }

    /// macOS: 用 wry `build_as_child` 把 WKWebView 挂为 content NSView 的子视图。
    #[cfg(target_os = "macos")]
    fn realize_macos(&self, parent: &crate::browser::platform::NsViewHandle) {
        // 先读 url(锁序:url 先、webview 后,与 load_url 一致,避免死锁)。
        let url = self.url.read().clone();
        let mut guard = self.webview.lock();
        if guard.is_some() {
            return;
        }
        // 初始 bounds 0×0;reposition pass 会在构造后立即 set_frame 到真实 rect。
        let bounds = wry::Rect {
            position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
            size: wry::dpi::LogicalSize::new(0.0, 0.0).into(),
        };
        // Phase C (R8): 若绑定了 SSH 隧道,把 webview 经由该 SOCKS5 端点出网,
        // 仅网关可达的内网站点才会加载。wry 用 Network framework 的
        // `nw_proxy_config_create_socksv5` 实现真正的 per-view 代理(mac-proxy
        // feature,需 macOS 14+)。代理是 build-time 配置 —— 绑定/解绑隧道会
        // 重建整个 webview(见 [`BrowserContext::set_tunnel`])。
        let proxy = self.proxy.lock().clone();
        let mut builder = wry::WebViewBuilder::new().with_url(&url).with_bounds(bounds);
        #[cfg(target_os = "macos")]
        if let Some((host, port)) = proxy {
            builder = builder.with_proxy_config(wry::ProxyConfig::Socks5(wry::ProxyEndpoint {
                host,
                port: port.to_string(),
            }));
        }
        let result = builder.build_as_child(parent);
        match result {
            Ok(wv) => *guard = Some(wv),
            Err(e) => log::warn!("browser: realize wry webview failed (url={url}): {e}"),
        }
    }

    /// 把 webview 对齐到 pane rect(逻辑像素,content view 坐标,**top-left 原点**)。
    ///
    /// wry macOS 的 `set_bounds` 内部会把 top-left 翻成 NSView 的 bottom-left
    /// 原点,故调用方按 top-left 传即可。未 realize 时为 no-op。
    pub fn set_frame(&self, x: f32, y: f32, w: f32, h: f32) {
        let guard = self.webview.lock();
        let Some(wv) = guard.as_ref() else { return };
        let bounds = wry::Rect {
            position: wry::dpi::LogicalPosition::new(x as f64, y as f64).into(),
            size: wry::dpi::LogicalSize::new(w as f64, h as f64).into(),
        };
        if let Err(e) = wv.set_bounds(bounds) {
            log::warn!("browser: set_bounds failed: {e}");
        }
    }

    /// 显示/隐藏 webview(切 tab 时隐藏非活跃 tab 的 web pane)。
    pub fn set_visible(&self, visible: bool) {
        let guard = self.webview.lock();
        let Some(wv) = guard.as_ref() else { return };
        if let Err(e) = wv.set_visible(visible) {
            log::warn!("browser: set_visible failed: {e}");
        }
    }

    /// 导航到新 URL:更新记录 + 让已 realize 的 webview 加载。
    pub fn load_url(&self, url: &str) {
        *self.url.write() = url.to_string();
        let guard = self.webview.lock();
        let Some(wv) = guard.as_ref() else { return };
        if let Err(e) = wv.load_url(url) {
            log::warn!("browser: load_url failed (url={url}): {e}");
        }
    }

    /// GAP-7.8: 在 webview 上执行 JS,回调拿到「序列化为 JSON 的返回值」
    /// (WKWebView 还会 await 返回的 Promise,再回调)。未 realize 时返回
    /// `false`,由调用方自行报错(不在此处猜一个空结果)。持锁仅跨入队,JS
    /// 异步执行期间不持锁。
    pub fn evaluate_with_callback(
        &self,
        js: &str,
        callback: impl Fn(String) + Send + 'static,
    ) -> bool {
        let guard = self.webview.lock();
        let Some(wv) = guard.as_ref() else { return false };
        if let Err(e) = wv.evaluate_script_with_callback(js, callback) {
            log::warn!("browser: evaluate_script failed: {e}");
        }
        true
    }

    /// GAP-7.8: 把底层 WKWebView(wry 的 `WryWebView` 子类)交给闭包,用于原生
    /// `takeSnapshotWithConfiguration:completionHandler:` 快照。未 realize 时返回
    /// `None`(调用方据此报错)。
    ///
    /// `WebViewExtMacOS::webview()` clone 出新的 `Retained<WryWebView>`(引用计数
    /// +1);**不缓存**该 `Retained` —— 缓存会在 webview 被 `take()` 掉后悬空,导致
    /// 快照打到已释放的 WKWebView。闭包拿到的是 `&WryWebView`(`Retained` 解引用);
    /// WKWebView 的方法(含 `takeSnapshot`)经继承可直接在其上调用(与 wry 内部调
    /// `evaluateJavaScript` 一致)。
    #[cfg(target_os = "macos")]
    pub fn with_wry_webview<R>(&self, f: impl FnOnce(&wry::WryWebView) -> R) -> Option<R> {
        use wry::WebViewExtMacOS;
        let guard = self.webview.lock();
        let Some(wv) = guard.as_ref() else { return None };
        let wk = wv.webview(); // Retained<WryWebView>
        Some(f(&wk))
    }

    /// Phase C: 把该 webview 绑定到一条 SSH 隧道 —— 后续所有流量经 `socks`
    /// 端点出网,可达仅网关能解析的内网站点。`tunnel_id` 仅作标识/快照;
    /// `socks` 是 realize 时真正消费的端点(通常 `127.0.0.1:<local_port>`)。
    ///
    /// 代理是 wry 的 build-time 配置,故若 webview 已 realize,这里 `take`
    /// 掉它,下一帧 `reposition_web_panes` → `try_realize` 会带新代理重建。
    pub fn set_tunnel(
        &self,
        tunnel_id: impl Into<String>,
        socks_host: impl Into<String>,
        socks_port: u16,
    ) {
        *self.tunnel_id.lock() = Some(tunnel_id.into());
        *self.proxy.lock() = Some((socks_host.into(), socks_port));
        let _ = self.webview.lock().take(); // 强制重建以套用新代理
    }

    /// Phase C: 解绑隧道,恢复直连(同样靠下一帧重建 webview 生效)。
    pub fn clear_tunnel(&self) {
        *self.tunnel_id.lock() = None;
        *self.proxy.lock() = None;
        let _ = self.webview.lock().take();
    }

    /// Phase C: 当前绑定的隧道 id(标识位,与 `proxy` 端点保持一致)。
    pub fn tunnel_id(&self) -> Option<String> {
        self.tunnel_id.lock().clone()
    }

    /// Phase C: 当前是否绑定了隧道。
    pub fn has_tunnel(&self) -> bool {
        self.tunnel_id.lock().is_some()
    }

    /// 关 pane / 关窗时调用:`take` 掉 wry WebView,释放底层 WKWebView。
    pub fn close(&self) {
        let _ = self.webview.lock().take();
    }
}

impl Drop for BrowserContext {
    fn drop(&mut self) {
        // 兜底:即使未显式 close(),也确保 WKWebView 随 pane 一起释放。
        let _ = self.webview.get_mut().take();
    }
}
