//! GAP-7.8: agent-browser command execution layer.
//!
//! Drives an embedded WebView pane over JSON-RPC (`browser.*`). Each op maps
//! to a tiny JS payload; wry's `evaluate_script_with_callback` runs it and
//! hands the (Promise-aware, on WKWebView) return value back as a JSON string.
//! We correlate **per-op** via a `tokio::sync::oneshot` captured in that
//! callback — no global pending registry, no subscription plumbing. The iced
//! `Task::perform` that owns the other end of the oneshot adds a `tokio::time`
//! backstop so a vanished callback (page navigated mid-op) can never hang the
//! JSON-RPC reply.
//!
//! MVP ops: navigate / click / type / extract / scroll / wait / get_url /
//! get_title + a `screenshot` stub (native WKWebView snapshot isn't exposed by
//! wry — surfaced as a structured error, not a silent no-op). Adding an op =
//! (1) a `BrowserOp` arm (2) a `build_js` arm (3) a `parse_op` arm (4) a CLI
//! subcommand; the IPC + EventBus + reply wiring is generic.
//!
//! See plan「GAP-3.5 + GAP-5.2 + GAP-7.8」Phase D.

use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::oneshot;

use super::BrowserContext;

/// The wire "result" of an op, produced either by the JS callback (JS ops), by
/// Rust directly (`navigate`/`screenshot`), or by the timeout backstop.
#[derive(Debug, Clone)]
pub enum BrowserReply {
    /// Op succeeded; carry the JSON result object (e.g. `{"clicked": true}`).
    Done(Value),
    /// Op failed; the human-readable reason becomes the JSON-RPC error message.
    Error(String),
}

/// Everything the iced main loop needs to finish a `browser.*` request once the
/// JS callback (or timeout) resolves. Carried by `Message::BrowserOpResolved`.
///
/// `reply_tx` is the cloned IPC reply handle (same pattern as
/// `feed.request_approval`): the main loop sends exactly one response through
/// it, taking the `oneshot::Sender` so a late timeout can't double-reply.
#[derive(Clone)]
pub struct BrowserResolved {
    pub ipc_id: u64,
    pub reply_tx: std::sync::Arc<std::sync::Mutex<Option<oneshot::Sender<crate::ipc::JsonRpcResponse>>>>,
    pub reply: BrowserReply,
    /// EventBus kind, e.g. `"browser.click"`.
    pub ev_kind: String,
    /// EventBus payload (op specifics, no outcome — the handler tags ok/error).
    pub ev_payload: Value,
}

// Manual `Debug` — `oneshot::Sender` isn't `Debug`, but `Message` derives it.
impl std::fmt::Debug for BrowserResolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserResolved")
            .field("ipc_id", &self.ipc_id)
            .field("ev_kind", &self.ev_kind)
            .field("reply", &self.reply)
            .finish()
    }
}

/// The agent-browser op. Constructed from a `browser.<verb>` method + params by
/// [`parse_op`]; serde-derived so it can be logged/serialized, but the live path
/// builds it explicitly (the wire method name is the discriminant, not a tag).
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
    /// The verb half of the method name (`navigate` for `browser.navigate`).
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

    /// EventBus payload describing the op (no outcome). The handler merges in
    /// `ok`/`error` after resolution.
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
            BrowserOp::Wait { selector, timeout_ms } => {
                json!({ "verb": "wait", "selector": selector, "timeout_ms": self.wait_deadline_ms() })
            }
            BrowserOp::GetUrl => json!({ "verb": "get_url" }),
            BrowserOp::GetTitle => json!({ "verb": "get_title" }),
            BrowserOp::Screenshot => json!({ "verb": "screenshot" }),
        }
    }

    /// How long the JS op may run before the Rust backstop replies with a
    /// timeout error. `wait` is given its own (possibly caller-supplied)
    /// deadline + grace; the rest are short.
    pub fn timeout(&self) -> Duration {
        match self {
            BrowserOp::Wait { .. } => Duration::from_millis(self.wait_deadline_ms() + 1_500),
            _ => Duration::from_millis(8_000),
        }
    }

    /// Effective `wait` deadline in ms (caller override or 5 s default).
    fn wait_deadline_ms(&self) -> u64 {
        if let BrowserOp::Wait { timeout_ms, .. } = self {
            timeout_ms.unwrap_or(5_000)
        } else {
            5_000
        }
    }

    /// Sensitivity gate: a navigation to a **different origin** than the pane's
    /// current page is audited as `browser.request` (the agent is steering the
    /// browser somewhere new). MVP emits + proceeds; a full interactive
    /// feed-panel block is deferred (the two-phase-reply wiring would need a
    /// browser-specific approval overlay).
    pub fn is_sensitive_navigate(&self, current_url: &str) -> Option<&str> {
        match self {
            BrowserOp::Navigate { url } if origin_of(url) != origin_of(current_url) => Some(url),
            _ => None,
        }
    }

    /// Build the JS for an op. Returns a value (or a Promise that resolves to
    /// one) that wry serializes to JSON for the callback. Everything is wrapped
    /// in try/catch so the callback **always** fires with a `{ok,…}` object —
    /// WKWebView drops the completion handler on a thrown exception on some
    /// platforms, and we never want to hang the reply.
    fn build_js(&self) -> String {
        match self {
            BrowserOp::Click { selector } => js_click(selector),
            BrowserOp::Type { selector, text } => js_type(selector, text),
            BrowserOp::Extract { selector, attribute } => js_extract(selector, attribute.as_deref()),
            BrowserOp::Scroll { x, y, selector } => js_scroll(x.unwrap_or(0), y.unwrap_or(0), selector.as_deref()),
            BrowserOp::Wait { selector, .. } => js_wait(selector, self.wait_deadline_ms()),
            BrowserOp::GetUrl => "(function(){try{return {ok:true,url:location.href};}catch(e){return {ok:false,error:String(e&&e.message||e)};}})()".to_string(),
            BrowserOp::GetTitle => "(function(){try{return {ok:true,title:document.title};}catch(e){return {ok:false,error:String(e&&e.message||e)};}})()".to_string(),
            // navigate/screenshot never reach build_js (handled in Rust); keep a
            // defensive arm so the match stays exhaustive.
            BrowserOp::Navigate { .. } | BrowserOp::Screenshot => {
                "(function(){return {ok:false,error:'op handled in Rust'};})()".to_string()
            }
        }
    }
}

/// Kick off `op` against `ctx`. Resolves `tx` exactly once:
/// - `navigate` → `ctx.load_url` + immediate ack (load is fire-and-forget).
/// - `screenshot` → structured "not wired" error.
/// - everything else → JS via `evaluate_script_with_callback`; the callback
///   owns `tx`. If the webview isn't realized yet, resolve immediately with an
///   error (the agent should `navigate` first / the pane is mid-attach).
pub fn execute(ctx: &BrowserContext, op: &BrowserOp, tx: oneshot::Sender<BrowserReply>) {
    match op {
        BrowserOp::Navigate { url } => {
            ctx.load_url(url);
            let _ = tx.send(BrowserReply::Done(json!({ "navigated": true, "url": url })));
        }
        BrowserOp::Screenshot => {
            let _ = tx.send(BrowserReply::Error(
                "screenshot: native WKWebView snapshot is not exposed by wry (GAP-7.8 TODO)".into(),
            ));
        }
        other => {
            if !ctx.is_realized() {
                let _ = tx.send(BrowserReply::Error(
                    "webview not realized yet — navigate to a page first".into(),
                ));
                return;
            }
            let js = other.build_js();
            // wry's callback is `Fn` (not `FnOnce`), but `oneshot::Sender::send`
            // consumes the sender. Guard it behind a `Mutex<Option<Sender>>` so
            // the closure stays `Fn` and only the first invocation actually sends
            // (a spurious second fire — e.g. page navigating mid-op — is a no-op).
            let tx = std::sync::Mutex::new(Some(tx));
            ctx.evaluate_with_callback(&js, move |json_str| {
                if let Some(s) = tx.lock().unwrap().take() {
                    let _ = s.send(parse_js_reply(json_str));
                }
            });
        }
    }
}

/// Interpret the wry callback payload. Empty → the JS returned null/undefined
/// or threw (page may have navigated). `{ok:false,error}` → op-level error.
/// Anything else → the op's result object.
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
        // Non-object fragment (e.g. a bare string) — wrap as the result.
        Err(_) => BrowserReply::Done(Value::String(s)),
    }
}

// ── Method → op parsing ──────────────────────────────────────────────────

/// Parse a `browser.<verb>` method + params into a [`BrowserOp`]. `pane_id` is
/// consumed by the caller (it's a pane selector, not part of the op).
pub fn parse_op(method: &str, params: Option<&Value>) -> Result<BrowserOp, String> {
    let verb = method.strip_prefix("browser.").unwrap_or(method);
    let p = |k: &str| params.and_then(|v| v.get(k));
    let s = |k: &str| p(k).and_then(|v| v.as_str());
    let i = |k: &str| p(k).and_then(|v| v.as_i64());
    let u = |k: &str| p(k).and_then(|v| v.as_u64());
    match verb {
        "navigate" => {
            let url = s("url").ok_or_else(|| "missing 'url'".to_string())?.to_string();
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

// ── JS builders ──────────────────────────────────────────────────────────

/// Embed `s` as a safe JS string literal (JSON string == JS string literal).
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

/// `wait` returns a **Promise**; WKWebView's `evaluateJavaScript` awaits it and
/// fires the wry callback on settlement — so the Rust side gets exactly one
/// resolution (found / timeout) without polling.
fn js_wait(selector: &str, timeout_ms: u64) -> String {
    let sel = js_str(selector);
    format!(
        "(function(sel,timeoutMs){{return new Promise(function(resolve){{var start=Date.now();function check(){{if(document.querySelector(sel)){{resolve({{ok:true,found:true,selector:sel}});return;}}if(Date.now()-start>=timeoutMs){{resolve({{ok:false,error:'timeout',selector:sel,timeout_ms:timeoutMs}});return;}}setTimeout(check,80);}}check();}});}})({sel},{timeout_ms})"
    )
}

/// Best-effort origin (`scheme://host[:port]`) for the sensitivity gate. Drops
/// path/query/userinfo. Two URLs with the same origin aren't "sensitive".
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
    // Take up to the first '/' (strip path); keep host[:port].
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
        // same origin, different path → not sensitive
        assert!(nav.is_sensitive_navigate("https://other.example/home").is_none());
        // non-navigate ops are never sensitive
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
        // The raw quote/backslash must NOT leak unescaped into the IIFE arg list
        // (it's inside a JSON-quoted JS string). Sanity: balanced parens.
        assert_eq!(js.matches('(').count(), js.matches(')').count());
    }
}
