//! GAP-7.8：agent 浏览器命令执行层。
//!
//! 通过 JSON-RPC（`browser.*`）驱动内嵌的 WebView 面板。每个操作映射到
//! 一小段 JS 载荷；wry 的 `evaluate_script_with_callback` 执行它，并把
//! （WKWebView 上可感知 Promise 的）返回值以 JSON 字符串形式交还。
//! 我们**按操作**通过捕获在该回调中的 `tokio::sync::oneshot` 关联——
//! 没有全局待处理注册表，没有订阅管道。持有 oneshot 另一端的 iced
//! `Task::perform` 加了 `tokio::time` 兜底，使消失的回调（操作中途页面
//! 跳转）永远不会挂起 JSON-RPC 回复。
//!
//! MVP 操作：navigate / click / type / extract / scroll / wait / get_url /
//! get_title + `screenshot`（通过 `takeSnapshotWith…` 拍摄真实 WKWebView
//! 快照，仅 macOS；其他平台得到结构化的“仅 macOS”错误）。新增一个操作 =
//! （1）加 `BrowserOp` 分支（2）加 `build_js` 分支（3）加 `parse_op` 分支
//! （4）加 CLI 子命令；IPC + EventBus + 回复接线是通用的。
//!
//! 参见计划「GAP-3.5 + GAP-5.2 + GAP-7.8」Phase D。

use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::oneshot;

use super::BrowserContext;

/// 操作的线路“结果”：由 JS 回调（JS 操作）、Rust 直接生成
/// （`navigate`/`screenshot`）或超时兜底产生。
#[derive(Debug, Clone)]
pub enum BrowserReply {
    /// 操作成功；携带 JSON 结果对象（例如 `{"clicked": true}`）。
    Done(Value),
    /// 操作失败；人类可读的原因成为 JSON-RPC 错误消息。
    Error(String),
}

/// 一旦 JS 回调（或超时）完成，iced 主循环完成 `browser.*` 请求所需的
/// 全部信息。由 `Message::BrowserOpResolved` 携带。
///
/// `reply_tx` 是克隆的 IPC 回复句柄（与 `feed.request_approval` 相同的
/// 模式）：主循环通过它恰好发送一次响应，取走 `oneshot::Sender`，使迟到
/// 的超时无法二次回复。
#[derive(Clone)]
pub struct BrowserResolved {
    pub ipc_id: u64,
    pub reply_tx: crate::ipc::ReplyTx,
    pub reply: BrowserReply,
    /// EventBus 种类，例如 `"browser.click"`。
    pub ev_kind: String,
    /// EventBus 载荷（操作的细节，不含结果——由处理器标注 ok/error）。
    pub ev_payload: Value,
}

// 手动 `Debug`——`oneshot::Sender` 不是 `Debug`，但 `Message` 派生它。
impl std::fmt::Debug for BrowserResolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserResolved")
            .field("ipc_id", &self.ipc_id)
            .field("ev_kind", &self.ev_kind)
            .field("reply", &self.reply)
            .finish()
    }
}

/// agent 浏览器操作。由 `browser.<动词>` 方法 + 参数经 [`parse_op`] 构造；
/// 派生 serde 以便记录/序列化，但实时路径显式构建它（线路方法名是判别式，
/// 不是标签）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BrowserOp {
    Navigate { url: String },
    Click { selector: String },
    Type { selector: String, text: String },
    Extract { selector: String, attribute: Option<String> },
    Scroll { x: Option<i64>, y: Option<i64>, selector: Option<String> },
    Wait { selector: String, timeout_ms: Option<u64> },
    GetUrl,
    GetTitle,
    Screenshot,
}

impl BrowserOp {
    /// 方法名的动词部分（`browser.navigate` 的 `navigate`）。
    pub fn verb(&self) -> &'static str {
        match self {
            BrowserOp::Navigate { .. } => "navigate",
            BrowserOp::Click { .. } => "click",
            BrowserOp::Type { .. } => "type",
            BrowserOp::Extract { .. } => "extract",
            BrowserOp::Scroll { .. } => "scroll",
            BrowserOp::Wait { .. } => "wait",
            BrowserOp::GetUrl => "get_url",
            BrowserOp::GetTitle => "get_title",
            BrowserOp::Screenshot => "screenshot",
        }
    }

    /// 描述该操作的 EventBus 载荷（不含结果）。处理器在完成后并入
    /// `ok`/`error`。
    pub fn event_payload(&self) -> Value {
        match self {
            BrowserOp::Navigate { url } => json!({ "verb": "navigate", "url": url }),
            BrowserOp::Click { selector } => json!({ "verb": "click", "selector": selector }),
            BrowserOp::Type { selector, text } => {
                json!({ "verb": "type", "selector": selector, "length": text.len() })
            }
            BrowserOp::Extract { selector, attribute } => {
                json!({ "verb": "extract", "selector": selector, "attribute": attribute })
            }
            BrowserOp::Scroll { x, y, selector } => {
                json!({ "verb": "scroll", "x": x.unwrap_or(0), "y": y.unwrap_or(0), "selector": selector })
            }
            BrowserOp::Wait { selector, timeout_ms: _ } => {
                json!({ "verb": "wait", "selector": selector, "timeout_ms": self.wait_deadline_ms() })
            }
            BrowserOp::GetUrl => json!({ "verb": "get_url" }),
            BrowserOp::GetTitle => json!({ "verb": "get_title" }),
            BrowserOp::Screenshot => json!({ "verb": "screenshot" }),
        }
    }

    /// JS 操作在 Rust 兜底回复超时错误前允许运行多久。`wait` 有自己的
    /// （可能由调用方提供）截止时间 + 宽限；其余都是短超时。
    pub fn timeout(&self) -> Duration {
        match self {
            BrowserOp::Wait { .. } => Duration::from_millis(self.wait_deadline_ms() + 1_500),
            _ => Duration::from_millis(8_000),
        }
    }

    /// `wait` 的有效截止时间（毫秒）（调用方覆盖或默认 5 秒）。
    fn wait_deadline_ms(&self) -> u64 {
        wait_deadline_ms(self.wait_timeout_ms())
    }

    /// `wait` 的调用方指定超时(毫秒);非 `wait` 操作为 `None`。
    pub fn wait_timeout_ms(&self) -> Option<u64> {
        if let BrowserOp::Wait { timeout_ms, .. } = self {
            *timeout_ms
        } else {
            None
        }
    }

    /// 敏感度闸门：导航到与面板当前页面**不同源**的地址会被审计为
    /// `browser.request`（agent 正在把浏览器引向新位置）。MVP 会发出事件并
    /// 继续；完整的交互式 feed 面板拦截被推迟（两阶段回复接线需要一个
    /// 浏览器专用的审批浮层）。
    pub fn is_sensitive_navigate(&self, current_url: &str) -> Option<&str> {
        match self {
            BrowserOp::Navigate { url } if origin_of(url) != origin_of(current_url) => Some(url),
            _ => None,
        }
    }

    /// 为一个操作构建 JS。返回一个值（或解析为该值的 Promise），wry 把它
    /// 序列化为 JSON 传给回调。所有内容都包在 try/catch 中，使回调**总是**
    /// 以 `{ok,…}` 对象触发——在某些平台上，WKWebView 会在抛出异常时丢弃
    /// 完成处理器，而我们绝不想挂起回复。
    fn build_js(&self) -> String {
        match self {
            BrowserOp::Click { selector } => js_click(selector),
            BrowserOp::Type { selector, text } => js_type(selector, text),
            BrowserOp::Extract { selector, attribute } => js_extract(selector, attribute.as_deref()),
            BrowserOp::Scroll { x, y, selector } => js_scroll(x.unwrap_or(0), y.unwrap_or(0), selector.as_deref()),
            // `wait` 由 `browser::wait` 的轮询状态机驱动(每次一次非阻塞
            // 探测),不走这里 —— 这里只是兜底,保证 JS 里没有忙等。
            BrowserOp::Wait { selector, .. } => super::wait::wait_probe_js(selector),
            BrowserOp::GetUrl => "(function(){try{return {ok:true,url:location.href};}catch(e){return {ok:false,error:String(e&&e.message||e)};}})()".to_string(),
            BrowserOp::GetTitle => "(function(){try{return {ok:true,title:document.title};}catch(e){return {ok:false,error:String(e&&e.message||e)};}})()".to_string(),
            // navigate/screenshot 从不进入 build_js（在 Rust 中处理）；保留一个
            // 防御性分支使 match 保持穷尽。
            BrowserOp::Navigate { .. } | BrowserOp::Screenshot => {
                "(function(){return {ok:false,error:'op handled in Rust'};})()".to_string()
            }
        }
    }
}

/// 对 `ctx` 启动 `op`。恰好解析一次 `tx`：
/// - `navigate` → `ctx.load_url` + 立即确认（加载即发即忘）。
/// - `screenshot` → 真实 WKWebView 快照（`takeSnapshotWithConfiguration`，
///   仅 macOS）；非 macOS 以结构化的“仅 macOS”错误解析。
/// - 其他 → 通过 `evaluate_script_with_callback` 执行 JS；回调持有 `tx`。
///   若 webview 尚未就绪，立即以错误解析（agent 应先 `navigate` /
///   面板正在挂载中）。
pub fn execute(ctx: &BrowserContext, op: &BrowserOp, tx: oneshot::Sender<BrowserReply>) {
    match op {
        BrowserOp::Navigate { url } => {
            ctx.load_url(url);
            let _ = tx.send(BrowserReply::Done(json!({ "navigated": true, "url": url })));
        }
        BrowserOp::Screenshot => {
            if !ctx.is_realized() {
                let _ = tx.send(BrowserReply::Error(
                    "webview not realized yet — navigate to a page first".into(),
                ));
                return;
            }
            // Real WKWebView snapshot (replaces the prior "wry 未暴露" stub). The
            // `tx` 移入下面的 Obj-C 完成处理器块；若几乎不可能发生的
            // `is_realized`/`take()` 竞争丢弃了它，`rx` 侧会报“浏览器操作通道已
            // 关闭”——与 JS 路径性质相同。
            #[cfg(target_os = "macos")]
            {
                let _ = ctx.with_wry_webview(|wk| macos_take_snapshot(wk, tx));
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = tx.send(BrowserReply::Error(
                    "screenshot: macOS-only in GAP-7.8".into(),
                ));
            }
        }
        // `wait` 由 `browser::wait` 的**轮询状态机**驱动(iced 主循环
        // 按 tick 重新求值),不能走这里的一次性 JS 求值路径 ——
        // WKWebView 的 `evaluateJavaScript` 不 await Promise,而同步
        // 忙等会占住页面的 JS 线程,让动态出现的元素永远出不来。
        BrowserOp::Wait { .. } => {
            let _ = tx.send(BrowserReply::Error(
                "browser.wait must be driven by the app's polling loop".into(),
            ));
        },
        other => {
            if !ctx.is_realized() {
                let _ = tx.send(BrowserReply::Error(
                    "webview not realized yet — navigate to a page first".into(),
                ));
                return;
            }
            let js = other.build_js();
            // wry 的回调是 `Fn`（不是 `FnOnce`），但 `oneshot::Sender::send` 会消耗
            // sender。把它放在 `Mutex<Option<Sender>>` 后面保护，使闭包保持 `Fn`，
            // 且只有第一次调用真正发送（虚假的第二次触发——例如操作中途页面跳转——
            // 是空操作）。
            let tx = std::sync::Mutex::new(Some(tx));
            ctx.evaluate_with_callback(&js, move |json_str| {
                // `.ok()`（而非 `.unwrap()`）：此闭包在 wry/主线程触发；毒化的互斥锁
                // 不能让 GUI panic。与下面的截图完成处理器路径一致。
                if let Some(s) = tx.lock().ok().and_then(|mut g| g.take()) {
                    let _ = s.send(parse_js_reply(json_str));
                }
            });
        }
    }
}

/// `wait` 的默认等待上限（毫秒）——调用方未指定 `timeout_ms` 时使用。
pub const WAIT_DEFAULT_TIMEOUT_MS: u64 = 5_000;

/// `wait` 的有效截止时间（毫秒）：调用方覆盖,否则用默认值。
pub fn wait_deadline_ms(timeout_ms: Option<u64>) -> u64 {
    timeout_ms.unwrap_or(WAIT_DEFAULT_TIMEOUT_MS)
}

/// GAP-7.8（macOS）：启动 WKWebView `takeSnapshotWithConfiguration:
/// completionHandler:` 并把它的异步完成接入 `tx`。WebKit 在稍后的
/// 主循环迭代触发完成回调（保证在主线程）。
///
/// `tx` 用 `Mutex<Option<_>>` 包裹，因为 wry/block2 的回调是 `Fn`
/// （不是 `FnOnce`）——块通过 `take()` 恰好发送一次，所以虚假的第二次
/// 触发是空操作。与 [`crate::macos_eyedropper::start_sample`] 一致。
/// `wk` 是 `&WryWebView`（`WKWebView` 的子类）；`takeSnapshot…` 是继承自
/// `WKWebView` 的方法，可以像 wry 自身在同一类型上调用
/// `evaluateJavaScript…` / `loadRequest` 那样调用它。
#[cfg(target_os = "macos")]
fn macos_take_snapshot(wk: &wry::WryWebView, tx: oneshot::Sender<BrowserReply>) {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSImage;
    use objc2_foundation::NSError;
    use std::sync::Mutex;

    // `execute()` 在 iced 主线程运行，takeSnapshot 也必须在主线程。
    // 防御性：若处于非主线程，大声失败而不是触发 WebKit 未定义行为。
    if MainThreadMarker::new().is_none() {
        let _ = tx.send(BrowserReply::Error(
            "screenshot: must run on the main thread".into(),
        ));
        return;
    }

    let tx_cell = Mutex::new(Some(tx));
    let block = RcBlock::new(move |img: *mut NSImage, err: *mut NSError| {
        let reply = if !err.is_null() {
            // SAFETY：WebKit 保证回调期间 `err` 是有效的 NSError。
            let e: &NSError = unsafe { &*err };
            BrowserReply::Error(format!("screenshot failed: {}", e.localizedDescription()))
        } else if img.is_null() {
            BrowserReply::Error("screenshot returned a nil image".into())
        } else {
            // SAFETY：WebKit 保证回调期间 `img` 是有效的 NSImage。
            match unsafe { ns_image_to_png_data_url(&*img) } {
                Ok(url) => BrowserReply::Done(json!({ "ok": true, "screenshot": url })),
                Err(msg) => BrowserReply::Error(msg),
            }
        };
        if let Some(s) = tx_cell.lock().ok().and_then(|mut g| g.take()) {
            let _ = s.send(reply);
        }
    });

    // SAFETY：`wk` 是存活的 WKWebView 子类实例；`block` 是堆上 Fn 块，
    // 其生命周期由 RcBlock 保证。调用立即返回——`block` 在稍后的主循环
    // tick 触发，在 `tx` 上发送一次。
    // `None` 配置 = WebKit 默认值（完整边界）。
    unsafe {
        wk.takeSnapshotWithConfiguration_completionHandler(None, &block);
    }
}

/// GAP-7.8（macOS）：`NSImage` → `data:image/png;base64,…`。链路：
/// TIFF 数据 → `NSBitmapImageRep` → PNG 表示 → 字节 → base64。
///
/// # 安全性
/// 本次调用期间 `img` 必须指向有效的 `NSImage`。
#[cfg(target_os = "macos")]
unsafe fn ns_image_to_png_data_url(img: &objc2_app_kit::NSImage) -> Result<String, String> {
    use base64::Engine;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey};
    use objc2_foundation::NSDictionary;
    use std::ffi::c_void;
    use std::ptr::NonNull;

    // NSImage → TIFF 数据。
    let tiff = img
        .TIFFRepresentation()
        .ok_or_else(|| "screenshot: NSImage produced no TIFF data".to_string())?;
    // TIFF 数据 → 位图表示。
    let rep = NSBitmapImageRep::imageRepWithData(&tiff)
        .ok_or_else(|| "screenshot: could not build NSBitmapImageRep".to_string())?;
    // 位图表示 → PNG 字节。`properties` 是空字典（无 PNG 选项）；
    // `NSBitmapImageRepPropertyKey` 是 `NSString` 的类型别名。
    let props = NSDictionary::<NSBitmapImageRepPropertyKey, objc2::runtime::AnyObject>::new();
    let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props) }
        .ok_or_else(|| "screenshot: PNG encoding returned no data".to_string())?;

    let len = png.length();
    let mut buf = vec![0u8; len];
    if len > 0 {
        // SAFETY：`png` 是恰好 `len` 字节的有效 NSData；`buf` 是 `len` 字节
        // 可写存储。
        unsafe {
            png.getBytes_length(NonNull::new_unchecked(buf.as_mut_ptr() as *mut c_void), len);
        }
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
    Ok(format!("data:image/png;base64,{b64}"))
}

/// 解读 wry 回调载荷。空 → JS 返回了 null/undefined 或抛出了异常
/// （页面可能已跳转）。`{ok:false,error}` → 操作级错误。其他 → 操作的
/// 结果对象。
fn parse_js_reply(s: String) -> BrowserReply {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return BrowserReply::Error(
            "browser op returned no value (page may have navigated away)".into(),
        );
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(v) if v.get("ok").and_then(|o| o.as_bool()) == Some(false) => BrowserReply::Error(
            v.get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("browser op failed")
                .to_string(),
        ),
        Ok(v) => BrowserReply::Done(v),
        // 非对象片段（例如裸字符串）——作为结果包裹。
        Err(_) => BrowserReply::Done(Value::String(s)),
    }
}

// ── 方法 → 操作解析 ──────────────────────────────────────────────────

/// 把 `browser.<动词>` 方法 + 参数解析为 [`BrowserOp`]。`pane_id` 由
/// 调用方消费（它是面板选择器，不是操作的一部分）。
pub fn parse_op(method: &str, params: Option<&Value>) -> Result<BrowserOp, String> {
    let verb = method.strip_prefix("browser.").unwrap_or(method);
    let p = |k: &str| params.and_then(|v| v.get(k));
    let s = |k: &str| p(k).and_then(|v| v.as_str());
    let i = |k: &str| p(k).and_then(|v| v.as_i64());
    let u = |k: &str| p(k).and_then(|v| v.as_u64());
    match verb {
        "navigate" => {
            let raw = s("url").ok_or_else(|| "missing 'url'".to_string())?;
            // GAP-7.8：通过共享的 `loadable_url` 核心规范化，使裸的
            // `localhost:8080` 真正可加载（WKWebView 不能直接接受它）。
            // 严格——非 URL 输入直接报错，而不是静默回退到搜索
            // （agent 应当被告知垃圾输入，而不是落在搜索页上）。
            let url = super::url::loadable_url(raw)
                .ok_or_else(|| format!("not a loadable URL: {raw:?}"))?;
            Ok(BrowserOp::Navigate { url })
        }
        "click" => Ok(BrowserOp::Click {
            selector: s("selector").ok_or_else(|| "missing 'selector'".to_string())?.to_string(),
        }),
        "type" => Ok(BrowserOp::Type {
            selector: s("selector").ok_or_else(|| "missing 'selector'".to_string())?.to_string(),
            text: s("text").ok_or_else(|| "missing 'text'".to_string())?.to_string(),
        }),
        "extract" => Ok(BrowserOp::Extract {
            selector: s("selector").ok_or_else(|| "missing 'selector'".to_string())?.to_string(),
            attribute: s("attribute").map(str::to_string),
        }),
        "scroll" => Ok(BrowserOp::Scroll {
            x: i("x"),
            y: i("y"),
            selector: s("selector").map(str::to_string),
        }),
        "wait" => Ok(BrowserOp::Wait {
            selector: s("selector").ok_or_else(|| "missing 'selector'".to_string())?.to_string(),
            timeout_ms: u("timeout_ms"),
        }),
        "get_url" => Ok(BrowserOp::GetUrl),
        "get_title" => Ok(BrowserOp::GetTitle),
        "screenshot" => Ok(BrowserOp::Screenshot),
        other => Err(format!("unknown browser op '{other}'")),
    }
}

/// GAP-7.8 Phase C 判定：隧道报告的 `status` 是否安全到可以把 webview 的
/// SOCKS5 代理绑定到它？
///
/// `bind_browser_tunnel` 从守护进程的隧道列表读取 `local_addr`，但
/// `stopped` / `failed` / `need_auth` / `connecting` 隧道也会记录
/// `local_addr`（`pureterm-daemon::tunnel` 保留了一个没有存活监听器的
/// 预备地址）。绑定其中任何一个都会让 webview 指向一个死端口，
/// `navigate` 随后确认 `{navigated:true}` 而页面从不加载——一个静默的
/// 假成功。按 `status` 过滤（而不只是看 `local_addr` 是否存在）正是为了
/// 防止这一点。状态字符串是有约定的
/// （`tunnel.rs::status_strings_match_cli_contract`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BindTunnelVerdict {
    Bind,
    Reject(&'static str),
}

pub(crate) fn bind_tunnel_verdict(status: &str) -> BindTunnelVerdict {
    match status {
        "connected" => BindTunnelVerdict::Bind,
        "connecting" => BindTunnelVerdict::Reject("tunnel is still connecting"),
        "stopped" => BindTunnelVerdict::Reject("tunnel is stopped"),
        "failed" => BindTunnelVerdict::Reject("tunnel failed to connect"),
        "need_auth" => BindTunnelVerdict::Reject("tunnel is waiting for authentication"),
        // 未知的状态字符串（例如未来的守护进程变体）——拒绝，而不是冒险绑定
        // 到非存活监听器。
        _ => BindTunnelVerdict::Reject("tunnel is not connected"),
    }
}

// ── JS 构建器 ──────────────────────────────────────────────────────────

/// 把 `s` 嵌入为安全的 JS 字符串字面量（JSON 字符串 == JS 字符串字面量）。
fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "''".into())
}

fn js_click(selector: &str) -> String {
    let sel = js_str(selector);
    format!(
        "(function(sel){{try{{var el=document.querySelector(sel);if(!el)return{{ok:false,error:'element not found',selector:sel}};el.click();return{{ok:true,clicked:true,selector:sel}};}}catch(e){{return{{ok:false,error:String(e&&e.message||e),selector:sel}};}}}})({sel})"
    )
}

fn js_type(selector: &str, text: &str) -> String {
    let sel = js_str(selector);
    let txt = js_str(text);
    format!(
        "(function(sel,txt){{try{{var el=document.querySelector(sel);if(!el)return{{ok:false,error:'element not found',selector:sel}};el.focus();if(el.isContentEditable){{el.innerText=txt;}}else if(el.tagName==='INPUT'||el.tagName==='TEXTAREA'||el.tagName==='SELECT'){{el.value=txt;}}else{{for(var i=0;i<txt.length;i++){{var c=txt.charCodeAt(i);el.dispatchEvent(new KeyboardEvent('keydown',{{key:txt[i],keyCode:c,bubbles:true}}));el.dispatchEvent(new KeyboardEvent('keyup',{{key:txt[i],keyCode:c,bubbles:true}}));}}}}el.dispatchEvent(new Event('input',{{bubbles:true}}));el.dispatchEvent(new Event('change',{{bubbles:true}}));return{{ok:true,typed:true,length:txt.length,selector:sel}};}}catch(e){{return{{ok:false,error:String(e&&e.message||e),selector:sel}};}}}})({sel},{txt})"
    )
}

fn js_extract(selector: &str, attribute: Option<&str>) -> String {
    let sel = js_str(selector);
    let attr = match attribute {
        Some(a) => js_str(a),
        None => "null".to_string(),
    };
    format!(
        "(function(sel,attr){{try{{var el=document.querySelector(sel);if(!el)return{{ok:false,error:'element not found',selector:sel}};var val=attr?el.getAttribute(attr):el.innerText;return{{ok:true,value:val,selector:sel,attribute:attr}};}}catch(e){{return{{ok:false,error:String(e&&e.message||e),selector:sel}};}}}})({sel},{attr})"
    )
}

fn js_scroll(x: i64, y: i64, selector: Option<&str>) -> String {
    let sel = match selector {
        Some(s) => js_str(s),
        None => "null".to_string(),
    };
    format!(
        "(function(x,y,sel){{try{{if(sel){{var el=document.querySelector(sel);if(el){{el.scrollIntoView();return{{ok:true,scrolled:true,selector:sel}};}}}}window.scrollBy(x,y);return{{ok:true,scrolled:true,x:x,y:y}};}}catch(e){{return{{ok:false,error:String(e&&e.message||e)}};}}}})({x},{y},{sel})"
    )
}


/// 敏感度闸门用的尽力而为的 origin（`scheme://host[:port]`）。去掉
/// 路径/查询/用户信息。同源的两个 URL 不算“敏感”。
fn origin_of(url: &str) -> String {
    let no_frag = url.split('#').next().unwrap_or(url);
    let no_query = no_frag.split('?').next().unwrap_or(no_frag);
    let after_scheme = match no_query.split_once("://") {
        Some((scheme, rest)) => return format!("{scheme}://{}", origin_host(rest)),
        None => no_query,
    };
    origin_host(after_scheme)
}

fn origin_host(rest: &str) -> String {
    // 取到第一个 '/' 为止（去掉路径）；保留 host[:port]。
    rest.split('/').next().unwrap_or(rest).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_navigate_requires_url() {
        assert!(parse_op("browser.navigate", None).is_err());
        let op = parse_op("browser.navigate", Some(&json!({"url":"https://x.io"}))).unwrap();
        assert!(matches!(op, BrowserOp::Navigate { url } if url == "https://x.io"));
    }

    #[test]
    fn parse_navigate_normalizes_loadable_url() {
        // GAP-7.8：裸的 host:port 会被转成可加载（动机是开发服务器场景——
        // `browser navigate localhost:8080` 必须以带 scheme 的形式到达
        // WKWebView）。已有 scheme 的输入原样通过。
        let op = parse_op("browser.navigate", Some(&json!({"url":"localhost:8080"}))).unwrap();
        assert!(matches!(op, BrowserOp::Navigate { url } if url == "http://localhost:8080"));
        let op = parse_op("browser.navigate", Some(&json!({"url":"example.com"}))).unwrap();
        assert!(matches!(op, BrowserOp::Navigate { url } if url == "https://example.com"));
        // 非 URL 输入被拒绝——agent 路径上不做静默搜索回退。
        assert!(parse_op("browser.navigate", Some(&json!({"url":"hello world"}))).is_err());
    }

    #[test]
    fn parse_click_and_type() {
        let c = parse_op("browser.click", Some(&json!({"selector":"#go"}))).unwrap();
        assert!(matches!(c, BrowserOp::Click { selector } if selector == "#go"));
        let t = parse_op("browser.type", Some(&json!({"selector":"#q","text":"hi"}))).unwrap();
        assert!(matches!(t, BrowserOp::Type { selector, text } if selector == "#q" && text == "hi"));
    }

    #[test]
    fn parse_extract_optional_attribute() {
        let a = parse_op("browser.extract", Some(&json!({"selector":"#x"}))).unwrap();
        assert!(matches!(a, BrowserOp::Extract { attribute: None, .. }));
        let b = parse_op("browser.extract", Some(&json!({"selector":"#x","attribute":"href"}))).unwrap();
        assert!(matches!(b, BrowserOp::Extract { attribute, .. } if attribute.as_deref() == Some("href")));
    }

    #[test]
    fn parse_wait_and_others() {
        assert!(parse_op("browser.wait", Some(&json!({"selector":".y","timeout_ms":1500}))).is_ok());
        assert!(matches!(parse_op("browser.get_url", None).unwrap(), BrowserOp::GetUrl));
        assert!(matches!(parse_op("browser.screenshot", None).unwrap(), BrowserOp::Screenshot));
        assert!(parse_op("browser.bogus", None).is_err());
    }

    #[test]
    fn verb_round_trips() {
        assert_eq!(BrowserOp::GetTitle.verb(), "get_title");
        assert_eq!(BrowserOp::Click { selector: "x".into() }.verb(), "click");
    }

    #[test]
    fn wait_timeout_uses_override_plus_grace() {
        let op = BrowserOp::Wait { selector: "s".into(), timeout_ms: Some(2_000) };
        assert_eq!(op.timeout(), Duration::from_millis(3_500));
        let def = BrowserOp::Wait { selector: "s".into(), timeout_ms: None };
        assert_eq!(def.timeout(), Duration::from_millis(6_500));
    }

    #[test]
    fn non_wait_ops_have_short_backstop() {
        assert_eq!(BrowserOp::Click { selector: "s".into() }.timeout(), Duration::from_millis(8_000));
    }

    #[test]
    fn sensitivity_flags_cross_origin_only() {
        let nav = BrowserOp::Navigate { url: "https://other.example/page".into() };
        assert!(nav.is_sensitive_navigate("https://example.com/").is_some());
        // 同源不同路径 → 不敏感
        assert!(nav.is_sensitive_navigate("https://other.example/home").is_none());
        // 非 navigate 操作从不敏感
        assert!(BrowserOp::Click { selector: "x".into() }.is_sensitive_navigate("https://a.io/").is_none());
    }

    #[test]
    fn parse_js_reply_ok_object() {
        let r = parse_js_reply(r#"{"ok":true,"clicked":true}"#.into());
        assert!(matches!(r, BrowserReply::Done(_)));
    }

    #[test]
    fn parse_js_reply_error_object() {
        let r = parse_js_reply(r#"{"ok":false,"error":"element not found"}"#.into());
        assert!(matches!(r, BrowserReply::Error(_)));
    }

    #[test]
    fn parse_js_reply_empty_is_error() {
        assert!(matches!(parse_js_reply("".into()), BrowserReply::Error(_)));
    }

    #[test]
    fn origin_strips_path_and_query() {
        assert_eq!(origin_of("https://a.io/p?q=1#f"), "https://a.io");
        assert_eq!(origin_of("https://a.io:8443/x"), "https://a.io:8443");
        assert_eq!(origin_of("http://localhost:3000"), "http://localhost:3000");
    }

    #[test]
    fn js_builders_are_nonempty_and_escape_quotes() {
        let tricky = "a'\"</script>";
        let js = js_type(tricky, tricky);
        assert!(js.contains("function"));
        // 原始引号/反斜杠绝不能未转义地漏进 IIFE 参数列表
        // （它位于 JSON 引号包裹的 JS 字符串内）。合理性检查：括号平衡。
        assert_eq!(js.matches('(').count(), js.matches(')').count());
    }

    /// 回归：`wait` 的 JS 绝不能是**同步忙等**。
    ///
    /// 忙等会占住页面的 JS 线程 —— 页面自己用 `setTimeout` / `fetch`
    /// 回调插入的元素永远出不来，于是 `browser wait` 对动态元素必然
    /// 超时。现在 `wait` 走 `browser::wait` 的轮询状态机,JS 只做
    /// 一次 `querySelector`。
    #[test]
    fn wait_js_is_not_a_busy_loop() {
        let op = BrowserOp::Wait {
            selector: "#marker".into(),
            timeout_ms: Some(1_000),
        };
        let js = op.build_js();
        assert!(js.contains("querySelector"));
        assert!(!js.contains("while"), "wait 的 JS 不能忙等: {js}");
        assert!(!js.contains("for("), "wait 的 JS 不能忙等: {js}");
    }

    #[test]
    fn wait_deadline_uses_override_or_default() {
        assert_eq!(wait_deadline_ms(Some(1234)), 1234);
        assert_eq!(wait_deadline_ms(None), WAIT_DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn bind_verdict_only_allows_connected() {
        // GAP-7.8：只有 `connected` 隧道有存活的 SOCKS5 监听器；其他已记录
        // 的状态仍带有（死掉的）`local_addr`，必须拒绝，使 `navigate` 无法对
        // 死代理假成功。
        assert_eq!(bind_tunnel_verdict("connected"), BindTunnelVerdict::Bind);

        for (status, want_contains) in [
            ("connecting", "connecting"),
            ("stopped", "stopped"),
            ("failed", "failed"),
            ("need_auth", "authentication"),
            ("bogus", "not connected"),
        ] {
            match bind_tunnel_verdict(status) {
                BindTunnelVerdict::Reject(msg) => assert!(
                    msg.contains(want_contains),
                    "status {status:?}: expected message containing {want_contains:?}, got {msg:?}"
                ),
                BindTunnelVerdict::Bind => panic!("status {status:?} must not bind"),
            }
        }
    }
}
