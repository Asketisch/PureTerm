//! GAP-7.8：`browser.wait` 的**轮询式**等待。
//!
//! 背景：WKWebView 的 `evaluateJavaScript:completionHandler:`（wry 0.55
//! 用的就是它）**不 await Promise** —— 返回 Promise 时直接给回
//! `undefined`。`objc2-web-kit` 0.3 也没有绑定 `awaitJavaScript:`，
//! 所以「一次求值、等它 settle」这条路走不通。
//!
//! 早前的实现退而求其次：在 JS 里 `while(true)` 同步忙等。这个改法有
//! 一个致命缺陷：**忙等期间页面的 JS 线程被占住**，页面自己靠
//! `setTimeout` / `fetch` 回调插入的元素永远出不来 —— 于是
//! `browser wait` 对「动态出现的元素」必然超时。
//!
//! 本模块改用**轮询**：Rust 侧每隔 [`POLL_INTERVAL`] 发起一次**同步、
//! 非阻塞**的「元素在不在」探测。每次探测之间页面的 JS 线程是空闲的，
//! 动态元素因此能够出现。轮询由 iced 的 `Task` 定时器驱动（必须回到
//! 主线程才能调用 `evaluateJavaScript`）。
//!
//! 状态机放在这里而不是 `iced_app.rs`：那里已经很大，且本状态机是
//! 纯逻辑 + 少量线程同步原语，可以独立单测。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use super::{BrowserContext, BrowserReply};

/// 两次探测之间的间隔。100ms 足以让页面推进一帧,又不会把主线程
/// 的消息循环饿死。
pub const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 一次 `browser.wait` 的进行中状态。
///
/// 由 iced 主循环持有（`HashMap<WaitId, WaitHandle>`）；每次
/// [`WaitHandle::probe_once`] 之后由定时器再驱动下一轮。
pub struct WaitHandle {
    /// 目标 web 窗格。窗格被关掉时轮询终止(报「窗格已关闭」)。
    pub pane_id: u64,
    /// 等待的选择器(CSS 表达式)。
    pub selector: String,
    /// 截止时刻。超过它仍未找到 → 超时。
    pub deadline: Instant,
    /// 等待上限(毫秒),仅用于错误信息。
    pub timeout_ms: u64,
    /// 「已经找到」标志。**只置位不清零** —— 多个探测的回调可能乱序
    /// 到达,后来的「没找到」不能抹掉先前的「找到了」。
    found: Arc<AtomicBool>,
    /// 解决 oneshot 的发送端(取走后即失效,保证只回一次)。
    tx: Arc<Mutex<Option<oneshot::Sender<BrowserReply>>>>,
}

impl WaitHandle {
    /// 新建一次等待。`timeout_ms` 是等待上限(毫秒)。
    pub fn new(
        pane_id: u64,
        selector: impl Into<String>,
        timeout_ms: u64,
        tx: oneshot::Sender<BrowserReply>,
    ) -> Self {
        Self {
            pane_id,
            selector: selector.into(),
            deadline: Instant::now() + Duration::from_millis(timeout_ms),
            timeout_ms,
            found: Arc::new(AtomicBool::new(false)),
            tx: Arc::new(Mutex::new(Some(tx))),
        }
    }

    /// 是否已经找到目标元素。
    pub fn is_found(&self) -> bool {
        self.found.load(Ordering::Relaxed)
    }

    /// 是否已过截止时刻。
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.deadline
    }

    /// 发起一次探测。返回 `false` 表示 webview 不可用(未 realize 或
    /// 已被销毁)—— 调用方应据此终止轮询。
    ///
    /// 探测本身是**同步但非阻塞**的:JS 只做一次 `querySelector`,
    /// 立刻返回。回调在稍后的主线程 tick 到达,命中时置位 `found`。
    pub fn probe_once(&self, ctx: &BrowserContext) -> bool {
        let js = wait_probe_js(&self.selector);
        let found = self.found.clone();
        ctx.evaluate_with_callback(&js, move |json_str| {
            if probe_hit(&json_str) {
                found.store(true, Ordering::Relaxed);
            }
        })
    }

    /// 解决等待(找到 / 超时 / 出错)。返回 `true` 表示本次调用真的
    /// 解决了 —— 之后的调用是空操作。
    pub fn resolve(&self, reply: BrowserReply) -> bool {
        match self.tx.lock().ok().and_then(|mut g| g.take()) {
            Some(tx) => {
                let _ = tx.send(reply);
                true
            },
            None => false,
        }
    }

    /// 「已经找到」的成功载荷。
    pub fn found_reply(&self) -> BrowserReply {
        BrowserReply::Done(serde_json::json!({
            "ok": true,
            "found": true,
            "selector": self.selector,
        }))
    }

    /// 超时载荷。
    pub fn timeout_reply(&self) -> BrowserReply {
        BrowserReply::Error(format!(
            "timeout waiting for selector {:?} after {} ms",
            self.selector, self.timeout_ms
        ))
    }
}

/// 探测用的 JS:**一次** `querySelector`,立刻返回。
///
/// 绝不能写成 `while` 忙等 —— 那会占住页面的 JS 线程,让页面自己
/// 插入的元素永远出不来(这正是旧实现的核心缺陷)。
pub fn wait_probe_js(selector: &str) -> String {
    let sel = js_str(selector);
    format!(
        "(function(sel){{try{{return{{ok:true,found:!!document.querySelector(sel)}};}}catch(e){{return{{ok:false,error:String(e&&e.message||e)}};}}}})({sel})"
    )
}

/// 解析探测回调的 JSON,判断是否命中。
fn probe_hit(json_str: &str) -> bool {
    let trimmed = json_str.trim();
    if trimmed.is_empty() {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .and_then(|v| v.get("found").and_then(|b| b.as_bool()))
        .unwrap_or(false)
}

/// 把字符串安全地嵌进 JS 字面量(与 `commands.rs` 的 `js_str` 同法)。
fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handle(timeout_ms: u64) -> WaitHandle {
        let (tx, _rx) = oneshot::channel();
        WaitHandle::new(1, "#marker", timeout_ms, tx)
    }

    #[test]
    fn probe_js_is_not_a_busy_loop() {
        let js = wait_probe_js("#marker");
        // 忙等会包含 while/for 循环 —— 探测必须是单次查询。
        assert!(!js.contains("while"), "探测 JS 不能是忙等: {js}");
        assert!(!js.contains("for("), "探测 JS 不能是忙等: {js}");
        assert!(js.contains("querySelector"));
    }

    #[test]
    fn probe_hit_reads_found_flag() {
        assert!(probe_hit(r#"{"ok":true,"found":true}"#));
        assert!(!probe_hit(r#"{"ok":true,"found":false}"#));
        assert!(!probe_hit(""));
        assert!(!probe_hit("not json"));
        assert!(!probe_hit(r#"{"ok":false,"error":"x"}"#));
    }

    #[test]
    fn found_flag_is_sticky() {
        let h = handle(1_000);
        assert!(!h.is_found());
        h.found.store(true, Ordering::Relaxed);
        assert!(h.is_found());
    }

    #[test]
    fn expired_after_deadline() {
        let h = handle(0);
        assert!(h.is_expired());
        let h2 = handle(60_000);
        assert!(!h2.is_expired());
    }

    #[test]
    fn resolve_only_once() {
        let (tx, mut rx) = oneshot::channel();
        let h = WaitHandle::new(1, "#x", 1_000, tx);
        assert!(h.resolve(BrowserReply::Done(serde_json::json!({"ok": true}))));
        // 第二次是空操作(不应 panic,也不应重复回复)。
        assert!(!h.resolve(BrowserReply::Done(serde_json::json!({"ok": true}))));
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn timeout_reply_mentions_selector() {
        let h = handle(1);
        let BrowserReply::Error(msg) = h.timeout_reply() else {
            panic!("timeout_reply 必须是错误");
        };
        assert!(msg.contains("#marker"), "{msg}");
    }
}
