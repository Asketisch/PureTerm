//! 提供商抽象 + 共享的 SSE 管道。
//!
//! 两个关注点：
//! - [`SseMapping`] —— 把一条已解码的
//!   [`SseEvent`](super::sse::SseEvent) 翻译为零或一条
//!   [`ChatEvent`](super::types::ChatEvent) 的提供商特定转换。**有状态**：
//!   令牌用量分散在多个事件中（Anthropic 的 `message_start` 携带
//!   `input_tokens`，`message_delta` 携带 `output_tokens`），因此映射器记住
//!   它们，并在终态 `Done` 时发出。这是逐提供商逻辑，**无需 HTTP** 即可
//!   完全单元测试——喂给它构造好的 `SseEvent` 即可。
//! - `ChatProvider` + `run_sse` —— 具体提供商实现的 trait，以及驱动
//!   [`super::sse::SseDecoder`] 遍历响应体字节流的共享读循环；它把每个事件
//!   经提供商的 `SseMapping` 映射，产出 `ChatEvent`，并尊重取消。这些需要
//!   流胶水；它们与（延后的）reqwest HTTP shim 一起在下一个 TDD 周期落地。
//!
//! 阶段 1 无头交付 `SseMapping` + 两个映射器；`ChatProvider`/`run_sse`
//! 在流式读循环接线后跟进。

use crate::ai_chat::sse::SseEvent;
use crate::ai_chat::types::ChatEvent;

/// 把一条已解码的 SSE 事件翻译为零或一条，针对特定提供商的线路格式
/// 的聊天事件。
///
/// `&mut self` 是因为令牌用量跨事件拆分到达（Anthropic 的
/// `message_start` → `input_tokens`，`message_delta` → `output_tokens`；
/// OpenAI 的可选 `usage` 块）；映射器累积它们，并在终态
/// `Done`/`message_stop`/`[DONE]` 时呈现。
///
/// `Send` 是因为映射器位于运行在 tokio 任务上的异步读循环内部。
pub trait SseMapping: Send {
    fn map(&mut self, ev: &SseEvent) -> Option<ChatEvent>;
}

/// 驱动一轮流式对话：把字节块喂给 [`SseDecoder`]，经 `mapper` 映射每条
/// 已解码的 [`SseEvent`]，按顺序返回得到的 [`ChatEvent`]。在第一个终态事件
/// （`Done`/`Error`）之后或字节流结束时停止。
///
/// **与 HTTP 无关**：`bytes` 是任意 `Result<字节块, 传输错误>` 流。生产
/// 中提供商注入其 reqwest 响应体；测试中注入覆盖手写 SSE 字节夹具的
/// `stream::iter`——同样的覆盖，无网络。`Err` 块模拟流中途的传输失败，
/// 变为终态 [`ChatEvent::Error`]。
///
/// 取消（面板关闭 / 新的一轮 / Stop）由上一层处理：提供商丢弃其消费者 /
/// 发出 `tokio::sync::Notify` 信号；此核心循环只负责 字节→事件 的翻译。
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
            // 流中途的传输失败 → 终态 ChatEvent::Error。
            Err(e) => {
                out.push(ChatEvent::Error(e.to_string()));
                return out;
            }
        }
    }
    // 流结束。若没有终态事件触发（服务器提前切断，没有
    // `message_stop`/`[DONE]`），我们返回已有的内容——不合成 Done；
    // 面板把这一轮标记为未完成。
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_chat::anthropic::AnthropicMap;
    use crate::ai_chat::openai::OpenAiMap;
    use iced::futures::stream;

    /// 在轻量级当前线程 tokio 运行时上把 `run_sse` future 跑到完成
    /// （无工作线程；循环不需要 tokio I/O）。
    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    /// 把一个字节切片包装为单块 `Result<Ok, String>` 流输入
    /// （固定错误类型，使 `vec![Ok(...)]` 无需注解）。
    fn one_chunk(b: &[u8]) -> Vec<std::result::Result<Vec<u8>, String>> {
        vec![Ok(b.to_vec())]
    }

    /// 一个逼真的 Anthropic 流式回合：start、两个文本增量、一个携带
    /// output_tokens 的 message_delta，然后是 message_stop。
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
        // 在任意偏移处切分字节流（`data:` 行中间、事件边界中间）——解码器必须
        // 跨块缓冲不完整数据，就像 socket 拆分交付它们一样。
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
        // message_stop 之后的字节（结尾的幽灵增量）必须被忽略——
        // 循环在第一个终态事件处就返回。
        let mut payload = ANTHROPIC_TURN.to_vec();
        payload.extend(
            b"event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ghost\"}}\n\n",
        );
        let events = block_on(run_sse(stream::iter(one_chunk(&payload)), AnthropicMap::new()));
        assert_eq!(events.len(), 3); // 没有第 4 个“幽灵” TextDelta
        assert_eq!(
            events[2],
            ChatEvent::Done {
                input_tokens: Some(12),
                output_tokens: Some(7),
            },
        );
    }

    /// 一个逼真的 OpenAI 兼容回合：仅 role 的第一块、一个内容增量、
    /// 一个 finish_reason 块，然后 [DONE]。
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
        // 第一块交付一个增量；第二块是传输错误，必须作为携带该消息的终态
        // ChatEvent::Error 呈现。
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
        // 若服务器提前切断（没有 message_stop/[DONE]），我们不编造 Done——
        // 面板把这一轮标记为未完成。只有孤立的增量，没有终结符。
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
// API 密钥处理（环境优先；密钥绝不进 TOML/日志）。
// ============================================================================

/// 一个 LLM API 密钥。包裹秘密，使其 [`std::fmt::Debug`] 实现绝不泄露
/// 值——可以安全地记录包含它的结构体。安全契约：密钥在提供商构造时从
/// 环境解析，绝不经 TOML/`toml_edit`/Settings 往返。
#[derive(Clone)]
pub struct SecretKey(String);

impl SecretKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    /// 借用原始密钥（例如设置 Authorization 头）。内部使用。
    pub fn get(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey(<redacted>)")
    }
}

/// 解析 API 密钥，环境优先。若 `override_env` 指定的变量存在于 `lookup`
/// 中，它获胜；否则回退到 `default_env`。两者都未设置时返回 `None`。
/// `lookup` 抽象了环境访问，因此无需接触（或竞争）真实进程环境即可
/// 单元测试。
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

/// 把 reqwest 传输错误归类为稳定的、不含 URL 的类别标签。
/// reqwest 的 `Display` 内嵌请求 URL——对于 OpenAI 兼容的提供商，那是
/// 用户配置的（可能是内部的）基础 URL——所以我们绝不在 `ChatEvent::Error`
/// 中呈现原始错误；而是呈现这个标签。
pub fn classify_reqwest_error(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "timeout"
    } else if e.is_connect() {
        "network"
    } else if e.is_decode() {
        "decode"
    } else if e.is_body() {
        "body"
    } else {
        "request"
    }
}

// ============================================================================
// ChatProvider trait + 流式读循环。
// ============================================================================

/// 一个流式 LLM 提供商（Anthropic Claude、任何 OpenAI 兼容端点）。
///
/// `stream()` 发起一个补全请求，并返回面板通过 `iced::Task::stream`
/// 驱动的 `ChatEvent` 流。`cancel` 被发信号以迅速中止进行中的 HTTP 请求
/// （面板关闭 / 新的一轮 / Stop 按钮）。
pub trait ChatProvider: Send + Sync + 'static {
    /// 供日志/配置使用的稳定标识符（“anthropic”、“openai”）。
    fn id(&self) -> &'static str;
    fn stream(
        &self,
        req: crate::ai_chat::types::ChatRequest,
        cancel: std::sync::Arc<tokio::sync::Notify>,
    ) -> std::pin::Pin<Box<dyn iced::futures::Stream<Item = ChatEvent> + Send>>;
}

/// 作为 `iced::futures::Stream` 暴露的 `tokio::sync::mpsc::Receiver`。
/// 流式读循环（[`run_sse_stream`] / [`drive_decode`]）把已解码的
/// `ChatEvent` 推过通道；面板消费此流。`Receiver<T>` 是 `Unpin` 的，
/// 因此包装器也是。
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

/// 共享的解码循环：把 `bytes` 喂给 SSE 解码器，经 `mapper` 映射每个事件，
/// 把 `ChatEvent` 推给 `tx`。在以下情况结束：取消、第一个终态事件
/// （`Done`/`Error`）、字节源结束、或消费者丢弃流（`send` 失败）。
/// `bytes` 被装箱+固定，因此这接受任意流——测试中的合成 `stream::iter`、
/// 提供商中的 reqwest `bytes_stream()`。
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
            // 快速中止（面板关闭 / 新的一轮 / Stop）——即使字节源阻塞在网络上也
            // 会跳出。
            _ = cancel.notified() => break,
            chunk = bytes.next() => match chunk {
                None => break,
                Some(Ok(b)) => {
                    decoder.push(b.as_ref());
                    for ev in decoder.drain() {
                        let Some(ce) = mapper.map(&ev) else { continue };
                        let terminal = ce.is_terminal();
                        // 消费者丢弃了流 → 停止。
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
    // tx 在此处析构 → 接收端（流）关闭 → 消费者看到 `None`。
}

/// [`run_sse`] 的流式对应物：生成一个解码任务，把已解码的 `ChatEvent`
/// 推过通道，增量产出（使面板在增量到达时即时渲染，而不是只在回合
/// 结束时）。`cancel` 收到通知时迅速中止——即使字节源阻塞在网络上也
/// 如此。
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
        // 一个永不产出、永不结束的字节源 → 解码任务阻塞在 `bytes.next()` 上。
        // 发出 cancel 信号必须打破循环并迅速关闭通道（远低于 2s 超时）。
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
            assert!(done.unwrap().is_empty()); // 无事件,流仅已关闭
        });
    }
}

#[cfg(test)]
mod classify_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn classify_reqwest_error_returns_stable_label() {
        // 格式错误的 URL 会在本地请求构建时失败（无网络）。契约：classify 返回
        // 固定标签集合中的一个，绝不返回原始错误（它内嵌配置的基础 URL）。
        let rt = tokio::runtime::Runtime::new().unwrap();
        let label: &str = rt.block_on(async {
            let tried = tokio::time::timeout(
                Duration::from_secs(3),
                reqwest::get("ht!tp://%%invalid url"),
            )
            .await;
            match tried {
                Ok(Ok(_)) => "success",
                Ok(Err(e)) => classify_reqwest_error(&e),
                Err(_) => "timed-out",
            }
        });
        const LABELS: &[&str] = &["timeout", "network", "decode", "body", "request"];
        assert!(LABELS.contains(&label), "unexpected label: {label}");
    }
}
