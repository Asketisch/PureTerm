//! 增量式服务器推送事件(SSE)解码器。
//!
//! 与 HTTP 无关:通过 [`SseDecoder::push`] 喂入原始响应体字节块,
//! 通过 [`SseDecoder::drain`] 取走完整的 [`SseEvent`];Anthropic 与
//! OpenAI 都以相同的 SSE 线上格式流式输出(只有 `data:` 里的 JSON
//! 不同),因此一个解码器即可服务两家提供商——各自在
//! [`super::provider`] 中持有自己的 `SseMapping`。
//!
//! 处理的线上格式:
//! `name: value` 字段行(冒号后至多一个空格;`data:foo` 合法,
//! 与 `data: foo` 等价)。
//! `event:` 设置事件类型;`data:` 追加内容(同一块内多条 `data:`
//! 行以换行符 `\n` 连接)。
//! 以 `:` 开头的行是注释(如 `: ping` 保活)——忽略。
//! `id:` / `retry:` 被忽略(我们不需要)。
//! 事件以空行(`\n\n`)结束;`\n` 与 `\r\n`
//! 两种行尾符均可接受。
//! 既无 `event:` 也无 `data:` 的块(纯心跳注释)
//! 不产生事件——OpenRouter 的 `: OPENROUTER PROCESSING` 不得
//! 产生多余的空白 `ChatEvent`。
//!
//! `[DONE]` 哨兵(OpenAI 的流终止符)**不在**这里特殊处理:
//! 它以 `data: [DONE]` 到达,并按原样解码为
//! `SseEvent { data: "[DONE]" }`;由 OpenAI 映射器识别。

/// 一条已解码的 SSE 事件。`event` 是 `event:` 字段(缺省为 None,
/// OpenAI 始终如此);`data` 是拼接后的 `data:` 载荷。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// 增量 SSE 解码器。缓冲原始字节; [`Self::drain`] 返回
/// 完整事件,并保留尚未结束的尾部块供下次继续。
#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: String,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一段原始响应体字节块(可能跨调用切断一条字段行
    /// 或一个事件块;未完成的数据会一直缓冲,直到空行
    /// 将其终止)。
    pub fn push(&mut self, chunk: &[u8]) {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
    }

    /// 返回所有已被空行终止的事件;保留未结束的尾部块。
    /// 既无 `event:` 也无 `data:` 的块(纯
    /// 注释心跳)不产生事件。
    pub fn drain(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        while let Some(end) = block_end(&self.buf) {
            let block: String = self.buf.drain(..end).collect();
            if let Some(ev) = parse_block(&block) {
                events.push(ev);
            }
        }
        events
    }
}

/// 在 `buf` 中查找第一个完整 SSE 块的结束下标(不含),
/// 含终止空行。若尚无空行终止任何块,
/// 则返回 `None`(尾部未完成的行继续缓冲)。`\n` 与 `\r\n`
/// 行终止符都被识别;末尾孤立的 `\r` 保持缓冲
/// (它可能是即将到来的 `\r\n` 的第一个字节)。
fn block_end(buf: &str) -> Option<usize> {
    let bytes = buf.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        let nl = bytes[i..].iter().position(|&b| b == b'\n')? + i;
        // 跨越 `[i, nl)` 的行是完整的(`nl` 之后跟着 `\n`)。
        let line = &buf[i..nl];
        let stripped = line.strip_suffix('\r').unwrap_or(line);
        let after = nl + 1; // 消费这个 `\n`
        if stripped.is_empty() {
            // 空行:块即 buf[..after]。
            return Some(after);
        }
        i = after;
    }
    None
}

/// 把一个完整 SSE 块(含终止空行)解析为事件。
/// 对既无 `event:` 也无 `data:` 字段的块
/// 返回 `None`(纯注释心跳)。多条 `data:` 行以 `\n` 连接;
/// 冒号后至多一个的空格会从每个值中剥离。
fn parse_block(block: &str) -> Option<SseEvent> {
    let mut event: Option<String> = None;
    let mut data_parts: Vec<String> = Vec::new();
    let mut has_field = false;
    for line in block.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if line.starts_with(':') {
            // 注释 / 保活(`: ping`)。忽略整行。
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v),
            None => continue, // 无冒号的字段名 + 空值——与我们无关。
        };
        // 冒号后一个可选前导空格属于格式的一部分,不是数据。
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => {
                event = Some(value.to_string());
                has_field = true;
            }
            "data" => {
                data_parts.push(value.to_string());
                has_field = true;
            }
            _ => {} // `id` / `retry` / 未知字段——忽略。
        }
    }
    if !has_field {
        return None;
    }
    Some(SseEvent {
        event,
        data: data_parts.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_data_event_terminated_by_blank_line() {
        let mut dec = SseDecoder::new();
        dec.push(b"event: message\ndata: hello\n\n");
        let events = dec.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("message"));
        assert_eq!(events[0].data, "hello");
        // 已完全排空——没有剩余。
        assert!(dec.drain().is_empty());
    }

    #[test]
    fn multiple_data_lines_join_with_newline() {
        let mut dec = SseDecoder::new();
        dec.push(b"data: line one\ndata: line two\n\n");
        let events = dec.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "line one\nline two");
    }

    #[test]
    fn crlf_line_terminators_accepted() {
        let mut dec = SseDecoder::new();
        dec.push(b"event: msg\r\ndata: hello\r\n\r\n");
        let events = dec.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("msg"));
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn pure_comment_heartbeat_yields_no_event() {
        // OpenRouter 发送 `: OPENROUTER PROCESSING` 作为保活——绝
        // 不得在下游产生多余的空白 ChatEvent。
        let mut dec = SseDecoder::new();
        dec.push(b": OPENROUTER PROCESSING\n\n");
        assert!(dec.drain().is_empty());
    }

    #[test]
    fn partial_block_buffers_across_pushes() {
        let mut dec = SseDecoder::new();
        dec.push(b"data: hel");
        assert!(dec.drain().is_empty()); // 尚无终止符
        dec.push(b"lo\n\n");
        let events = dec.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn two_events_in_one_chunk_both_drained() {
        let mut dec = SseDecoder::new();
        dec.push(b"data: first\n\ndata: second\n\n");
        let events = dec.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "first");
        assert_eq!(events[1].data, "second");
    }

    #[test]
    fn no_space_after_colon_is_valid() {
        let mut dec = SseDecoder::new();
        // `data:foo`(无空格)与 `data: foo` 等价。
        dec.push(b"data:foo\n\n");
        assert_eq!(dec.drain()[0].data, "foo");
    }

    #[test]
    fn done_sentinel_decoded_verbatim_not_special_cased() {
        // OpenAI 以 `data: [DONE]` 结束;解码器将其原样
        // 传递,以便 OpenAI 映射器能够识别。
        let mut dec = SseDecoder::new();
        dec.push(b"data: [DONE]\n\n");
        let events = dec.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "[DONE]");
        assert!(events[0].event.is_none());
    }

    #[test]
    fn json_payload_preserved_verbatim() {
        // 一个真实的 Anthropic 增量块:event + JSON 数据负载。
        let payload = r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hi"}}"#;
        let mut dec = SseDecoder::new();
        dec.push(format!("event: content_block_delta\ndata: {payload}\n\n").as_bytes());
        let events = dec.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("content_block_delta"));
        assert_eq!(events[0].data, payload);
    }

    #[test]
    fn trailing_partial_line_without_terminator_stays_buffered() {
        let mut dec = SseDecoder::new();
        // 一个完整事件,随后是一行没有尾随换行的未完成行。
        dec.push(b"data: done\n\ndata: incomplete");
        let events = dec.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "done");
        // 未完成的行会保留到下次 push。
        dec.push(b" continued\n\n");
        let events = dec.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "incomplete continued");
    }
}
