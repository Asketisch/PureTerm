//! Provider abstraction + shared SSE plumbing.
//!
//! Two concerns:
//! - [`SseMapping`] — provider-specific translation of one decoded
//!   [`SseEvent`](super::sse::SseEvent) into zero or one
//!   [`ChatEvent`](super::types::ChatEvent). **Stateful**: token usage is split
//!   across events (Anthropic's `message_start` carries `input_tokens`,
//!   `message_delta` carries `output_tokens`), so the mapper remembers them and
//!   emits them on the terminal `Done`. This is the per-provider logic that's
//!   fully unit-testable with **no HTTP** — feed it constructed `SseEvent`s.
//! - `ChatProvider` + `run_sse` — the trait a concrete provider implements and
//!   the shared read loop that drives [`super::sse::SseDecoder`] over a response
//!   body byte-stream, mapping each event through the provider's `SseMapping`,
//!   yielding `ChatEvent`s, and honouring cancellation. These need stream glue;
//!   they land in the next TDD cycle alongside the (deferred) reqwest HTTP shim.
//!
//! Phase 1 ships `SseMapping` + the two mappers headless; `ChatProvider`/`run_sse`
//! follow once the streaming read loop is wired.

use crate::ai_chat::sse::SseEvent;
use crate::ai_chat::types::ChatEvent;

/// Translate one decoded SSE event into zero or one chat events for a specific
/// provider's wire format.
///
/// `&mut self` because token usage arrives split across events (Anthropic's
/// `message_start` → `input_tokens`, `message_delta` → `output_tokens`; OpenAI's
/// optional `usage` chunk); the mapper accumulates and surfaces them on the
/// terminal `Done`/`message_stop`/`[DONE]`.
///
/// `Send` because the mapper lives inside the async read loop running on a tokio
/// task.
pub trait SseMapping: Send {
    fn map(&mut self, ev: &SseEvent) -> Option<ChatEvent>;
}

/// Drive one streaming turn: feed byte chunks through [`SseDecoder`], map each
/// decoded [`SseEvent`] via `mapper`, and return the resulting [`ChatEvent`]s in
/// order. Stops after the first terminal event (`Done`/`Error`) or when the byte
/// stream ends.
///
/// **HTTP-agnostic**: `bytes` is any stream of `Result<byte-chunk, transport
/// error>`. In production a provider injects its reqwest response body; in tests
/// we inject `stream::iter` over hand-written SSE byte fixtures — same coverage,
/// no network. An `Err` chunk models a mid-stream transport failure and becomes a
/// terminal [`ChatEvent::Error`].
///
/// Cancellation (panel close / new turn / Stop) is handled one layer up by the
/// provider dropping its consumer / signalling a `tokio::sync::Notify`; this core
/// loop owns the byte→event translation only.
pub async fn run_sse<S, M, B, E>(mut bytes: S, mut mapper: M) -> Vec<ChatEvent>
where
    S: iced::futures::Stream<Item = std::result::Result<B, E>> + Unpin,
    M: SseMapping,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    use iced::futures::StreamExt;

    let mut decoder = crate::ai_chat::sse::SseDecoder::new();
    let mut out = Vec::new();
    while let Some(chunk) = bytes.next().await {
        match chunk {
            Ok(b) => {
                decoder.push(b.as_ref());
                for ev in decoder.drain() {
                    let Some(ce) = mapper.map(&ev) else {
                        continue;
                    };
                    let terminal = ce.is_terminal();
                    out.push(ce);
                    if terminal {
                        return out;
                    }
                }
            }
            // Mid-stream transport failure → terminal ChatEvent::Error.
            Err(e) => {
                out.push(ChatEvent::Error(e.to_string()));
                return out;
            }
        }
    }
    // Stream ended. If no terminal event fired (server cut early without
    // `message_stop`/`[DONE]`), we return what we have — no synthetic Done; the
    // panel marks the turn incomplete.
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_chat::anthropic::AnthropicMap;
    use crate::ai_chat::openai::OpenAiMap;
    use iced::futures::stream;

    /// Run a `run_sse` future to completion on a lightweight current-thread
    /// tokio runtime (no worker threads; the loop needs no tokio I/O).
    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    /// Wrap one byte slice as a single-chunk `Result<Ok, String>` stream input
    /// (fixes the error type so `vec![Ok(...)]` needs no annotation).
    fn one_chunk(b: &[u8]) -> Vec<std::result::Result<Vec<u8>, String>> {
        vec![Ok(b.to_vec())]
    }

    /// A realistic Anthropic streaming turn: start, two text deltas, a
    /// message_delta carrying output_tokens, then message_stop.
    const ANTHROPIC_TURN: &[u8] = b"\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":1}}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":7}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";

    #[test]
    fn anthropic_turn_full_pipeline_one_chunk() {
        let bytes = stream::iter(one_chunk(ANTHROPIC_TURN));
        let events = block_on(run_sse(bytes, AnthropicMap::new()));
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], ChatEvent::TextDelta("Hello".into()));
        assert_eq!(events[1], ChatEvent::TextDelta(" world".into()));
        assert_eq!(
            events[2],
            ChatEvent::Done {
                input_tokens: Some(12),
                output_tokens: Some(7),
            },
        );
    }

    #[test]
    fn anthropic_turn_split_across_chunks_mid_line() {
        // Split the byte stream at arbitrary offsets (mid-`data:` line, mid-event
        // boundary) — the decoder must buffer partials across chunks exactly as a
        // socket would deliver them split.
        let mid = ANTHROPIC_TURN.len() / 3;
        let mid2 = 2 * ANTHROPIC_TURN.len() / 3;
        let chunks: Vec<std::result::Result<Vec<u8>, String>> = vec![
            Ok(ANTHROPIC_TURN[..mid].to_vec()),
            Ok(ANTHROPIC_TURN[mid..mid2].to_vec()),
            Ok(ANTHROPIC_TURN[mid2..].to_vec()),
        ];
        let events = block_on(run_sse(stream::iter(chunks), AnthropicMap::new()));
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], ChatEvent::TextDelta("Hello".into()));
        assert_eq!(events[1], ChatEvent::TextDelta(" world".into()));
        assert_eq!(
            events[2],
            ChatEvent::Done {
                input_tokens: Some(12),
                output_tokens: Some(7),
            },
        );
    }

    #[test]
    fn terminal_done_stops_before_trailing_events() {
        // Bytes AFTER message_stop (a trailing ghost delta) must be ignored —
        // the loop returns on the first terminal event.
        let mut payload = ANTHROPIC_TURN.to_vec();
        payload.extend(
            b"event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ghost\"}}\n\n",
        );
        let events = block_on(run_sse(stream::iter(one_chunk(&payload)), AnthropicMap::new()));
        assert_eq!(events.len(), 3); // no 4th "ghost" TextDelta
        assert_eq!(
            events[2],
            ChatEvent::Done {
                input_tokens: Some(12),
                output_tokens: Some(7),
            },
        );
    }

    /// A realistic OpenAI-compatible turn: role-only first chunk, one content
    /// delta, a finish_reason chunk, then [DONE].
    const OPENAI_TURN: &[u8] = b"\
data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
data: [DONE]\n\n";

    #[test]
    fn openai_turn_full_pipeline() {
        let events = block_on(run_sse(
            stream::iter(one_chunk(OPENAI_TURN)),
            OpenAiMap::new(),
        ));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], ChatEvent::TextDelta("Hi".into()));
        assert_eq!(
            events[1],
            ChatEvent::Done {
                input_tokens: None,
                output_tokens: None,
            },
        );
    }

    #[test]
    fn transport_error_mid_stream_becomes_chat_error() {
        // First chunk delivers a delta; the second chunk is a transport error,
        // which must surface as a terminal ChatEvent::Error carrying the message.
        let first = b"event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n\n";
        let chunks: Vec<std::result::Result<Vec<u8>, String>> = vec![
            Ok(first.to_vec()),
            Err("connection reset by peer".to_string()),
        ];
        let events = block_on(run_sse(stream::iter(chunks), AnthropicMap::new()));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], ChatEvent::TextDelta("partial".into()));
        assert!(matches!(events[1], ChatEvent::Error(ref m) if m.contains("connection reset")));
    }

    #[test]
    fn stream_ending_without_terminal_yields_no_synthetic_done() {
        // If the server cuts early (no message_stop/[DONE]), we do NOT fabricate a
        // Done — the panel marks the turn incomplete. A lone delta, no terminator.
        let bytes = b"event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"orphan\"}}\n\n";
        let events = block_on(run_sse(
            stream::iter(one_chunk(bytes)),
            AnthropicMap::new(),
        ));
        assert_eq!(events, vec![ChatEvent::TextDelta("orphan".into())]);
    }
}

// ============================================================================
// API-key handling (env-first; key NEVER in TOML/logs).
// ============================================================================

/// An LLM API key. Wraps the secret so its [`std::fmt::Debug`] impl never reveals
/// the value — safe to log a struct that contains one. Security contract: keys
/// resolve from the environment at provider construction and never round-trip
/// through TOML/`toml_edit`/Settings.
#[derive(Clone)]
pub struct SecretKey(String);

impl SecretKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    /// Borrow the raw key (e.g. to set an Authorization header). Internal use.
    pub fn get(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey(<redacted>)")
    }
}

/// Resolve an API key, env-first. If `override_env` names a variable present in
/// `lookup`, it wins; otherwise fall back to `default_env`. Returns `None` if
/// neither is set. `lookup` abstracts environment access so this is unit-testable
/// without touching (or racing on) the real process environment.
pub fn resolve_key(
    lookup: impl Fn(&str) -> Option<String>,
    override_env: Option<&str>,
    default_env: &str,
) -> Option<SecretKey> {
    if let Some(name) = override_env {
        if let Some(v) = lookup(name) {
            return Some(SecretKey::new(v));
        }
    }
    lookup(default_env).map(SecretKey::new)
}

// ============================================================================
// ChatProvider trait + streaming read loop.
// ============================================================================

/// A streaming LLM provider (Anthropic Claude, any OpenAI-compatible endpoint).
///
/// `stream()` issues one completion request and returns a `ChatEvent` stream the
/// panel drives via `iced::Task::stream`. `cancel` is signalled to abort the
/// in-flight HTTP request promptly (panel close / new turn / Stop button).
pub trait ChatProvider: Send + Sync + 'static {
    /// Stable identifier for logging/config ("anthropic", "openai").
    fn id(&self) -> &'static str;
    fn stream(
        &self,
        req: crate::ai_chat::types::ChatRequest,
        cancel: std::sync::Arc<tokio::sync::Notify>,
    ) -> std::pin::Pin<Box<dyn iced::futures::Stream<Item = ChatEvent> + Send>>;
}

/// `tokio::sync::mpsc::Receiver` exposed as an `iced::futures::Stream`. The
/// streaming read loop ([`run_sse_stream`] / [`drive_decode`]) pushes decoded
/// `ChatEvent`s through the channel; the panel consumes this stream.
/// `Receiver<T>` is `Unpin`, so the wrapper is too.
pub(super) struct MpscReceiverStream(tokio::sync::mpsc::Receiver<ChatEvent>);

impl MpscReceiverStream {
    pub(super) fn new(rx: tokio::sync::mpsc::Receiver<ChatEvent>) -> Self {
        Self(rx)
    }
}

impl iced::futures::Stream for MpscReceiverStream {
    type Item = ChatEvent;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<ChatEvent>> {
        self.0.poll_recv(cx)
    }
}

/// The shared decode loop: feed `bytes` through the SSE decoder, map each event
/// via `mapper`, push `ChatEvent`s to `tx`. Ends on: cancel, the first terminal
/// event (`Done`/`Error`), the byte source ending, or the consumer dropping the
/// stream (`send` fails). `bytes` is boxed+pinned so this accepts any stream —
/// synthetic `stream::iter` in tests, reqwest's `bytes_stream()` in providers.
pub(super) async fn drive_decode<S, M, B, E>(
    bytes: S,
    mut mapper: M,
    cancel: std::sync::Arc<tokio::sync::Notify>,
    tx: tokio::sync::mpsc::Sender<ChatEvent>,
) where
    S: iced::futures::Stream<Item = std::result::Result<B, E>> + Send,
    M: SseMapping + Send,
    B: AsRef<[u8]> + Send,
    E: std::fmt::Display + Send,
{
    use iced::futures::StreamExt;
    let mut bytes = Box::pin(bytes);
    let mut decoder = crate::ai_chat::sse::SseDecoder::new();
    loop {
        tokio::select! {
            // Prompt abort (panel close / new turn / Stop) — breaks even if the
            // byte source is blocked on the network.
            _ = cancel.notified() => break,
            chunk = bytes.next() => match chunk {
                None => break,
                Some(Ok(b)) => {
                    decoder.push(b.as_ref());
                    for ev in decoder.drain() {
                        let Some(ce) = mapper.map(&ev) else { continue };
                        let terminal = ce.is_terminal();
                        // Consumer dropped the stream → stop.
                        if tx.send(ce).await.is_err() {
                            return;
                        }
                        if terminal {
                            return;
                        }
                    }
                }
                Some(Err(e)) => {
                    let _ = tx.send(ChatEvent::Error(e.to_string())).await;
                    return;
                }
            }
        }
    }
    // tx drops here → the receiver (stream) closes → consumer sees `None`.
}

/// Streaming counterpart of [`run_sse`]: spawn a decode task that pushes decoded
/// `ChatEvent`s through a channel, yielding them incrementally (so the panel
/// renders deltas as they arrive, not only at turn end). Aborts promptly when
/// `cancel` is notified — even if the byte source is blocked on the network.
pub fn run_sse_stream<S, M, B, E>(
    bytes: S,
    mapper: M,
    cancel: std::sync::Arc<tokio::sync::Notify>,
) -> MpscReceiverStream
where
    S: iced::futures::Stream<Item = std::result::Result<B, E>> + Send + Unpin + 'static,
    M: SseMapping + Send + 'static,
    B: AsRef<[u8]> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel::<ChatEvent>(64);
    tokio::spawn(drive_decode(bytes, mapper, cancel, tx));
    MpscReceiverStream::new(rx)
}

#[cfg(test)]
mod keys_tests {
    use super::*;
    use std::collections::HashMap;

    fn env_lookup(map: HashMap<&'static str, String>) -> impl Fn(&str) -> Option<String> {
        move |name| map.get(name).cloned()
    }

    #[test]
    fn secret_key_debug_never_exposes_the_value() {
        let k = SecretKey::new("sk-super-secret-1234567");
        let dbg = format!("{:?}", k);
        assert_eq!(dbg, "SecretKey(<redacted>)");
        assert!(!dbg.contains("sk-super-secret"));
    }

    #[test]
    fn resolve_key_uses_default_env() {
        let mut e = HashMap::new();
        e.insert("ANTHROPIC_API_KEY", "sk-anth".to_string());
        let key = resolve_key(env_lookup(e), None, "ANTHROPIC_API_KEY");
        assert_eq!(key.unwrap().get(), "sk-anth");
    }

    #[test]
    fn resolve_key_override_env_takes_precedence() {
        let mut e = HashMap::new();
        e.insert("ANTHROPIC_API_KEY", "default".to_string());
        e.insert("MY_CUSTOM_KEY", "override".to_string());
        let key = resolve_key(env_lookup(e), Some("MY_CUSTOM_KEY"), "ANTHROPIC_API_KEY");
        assert_eq!(key.unwrap().get(), "override");
    }

    #[test]
    fn resolve_key_falls_back_to_default_when_override_unset() {
        let mut e = HashMap::new();
        e.insert("ANTHROPIC_API_KEY", "default".to_string());
        let key = resolve_key(env_lookup(e), Some("MY_CUSTOM_KEY"), "ANTHROPIC_API_KEY");
        assert_eq!(key.unwrap().get(), "default");
    }

    #[test]
    fn resolve_key_returns_none_when_neither_set() {
        let key = resolve_key(env_lookup(HashMap::new()), Some("MY_CUSTOM_KEY"), "ANTHROPIC_API_KEY");
        assert!(key.is_none());
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use crate::ai_chat::anthropic::AnthropicMap;
    use iced::futures::StreamExt;
    use iced::futures::stream;
    use std::sync::Arc;
    use std::time::Duration;

    const ANTHROPIC_TURN: &[u8] = b"\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":1}}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";

    async fn collect<S: iced::futures::Stream<Item = ChatEvent> + Unpin>(mut s: S) -> Vec<ChatEvent> {
        let mut out = Vec::new();
        while let Some(ce) = s.next().await {
            out.push(ce);
        }
        out
    }

    #[test]
    fn streams_events_incrementally_then_closes() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let events = rt.block_on(async {
            let s = run_sse_stream(
                stream::iter(vec![std::result::Result::<Vec<u8>, String>::Ok(
                    ANTHROPIC_TURN.to_vec(),
                )]),
                AnthropicMap::new(),
                Arc::new(tokio::sync::Notify::new()),
            );
            collect(s).await
        });
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], ChatEvent::TextDelta("Hello".into()));
        assert!(matches!(events[1], ChatEvent::Done { .. }));
    }

    #[test]
    fn cancel_unblocks_a_pending_byte_source() {
        // A byte source that never yields and never ends → the decode task blocks
        // on `bytes.next()`. Signalling cancel must break the loop and close the
        // channel promptly (well under the 2s timeout).
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cancel = Arc::new(tokio::sync::Notify::new());
            let s = run_sse_stream(
                stream::pending::<std::result::Result<Vec<u8>, String>>(),
                AnthropicMap::new(),
                cancel.clone(),
            );
            cancel.notify_one();
            let done = tokio::time::timeout(Duration::from_secs(2), collect(s)).await;
            assert!(done.is_ok(), "stream did not close after cancel (timed out)");
            assert!(done.unwrap().is_empty()); // no events, just closed
        });
    }
}
