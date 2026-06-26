//! Incremental Server-Sent Events (SSE) decoder.
//!
//! HTTP-agnostic: feed it raw response-body byte chunks via [`SseDecoder::push`],
//! drain complete [`SseEvent`]s via [`SseDecoder::drain`]. Both Anthropic and
//! OpenAI stream over the same SSE wire format (only the JSON inside `data:`
//! differs), so one decoder serves both providers — each just owns its own
//! `SseMapping` in [`super::provider`].
//!
//! Wire format handled:
//! - Field lines `name: value` (one optional space after the colon; `data:foo` is
//!   valid and equals `data: foo`).
//! - `event:` sets the event type; `data:` appends (multiple `data:` lines in one
//!   block join with `\n`).
//! - Lines starting with `:` are comments (keepalives like `: ping`) — ignored.
//! - `id:` / `retry:` are ignored (we don't need them).
//! - Events are terminated by a blank line (`\n\n`); both `\n` and `\r\n`
//!   terminators are accepted.
//! - A block with neither an `event:` nor a `data:` line (pure heartbeat comment)
//!   yields no event — OpenRouter's `: OPENROUTER PROCESSING` must not produce a
//!   spurious empty `ChatEvent`.
//!
//! The `[DONE]` sentinel (OpenAI's stream terminator) is **not** special-cased
//! here: it arrives as `data: [DONE]` and is decoded verbatim into
//! `SseEvent { data: "[DONE]" }`; the OpenAI mapper recognizes it.

/// One decoded SSE event. `event` is the `event:` field (None when absent, which
/// OpenAI always is); `data` is the joined `data:` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Incremental SSE decoder. Buffers raw bytes; [`Self::drain`] returns the
/// complete events and retains any trailing partial block for the next push.
#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: String,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a raw response-body chunk (may split a field line or an event
    /// block across calls; partial data stays buffered until a blank line
    /// terminates it).
    pub fn push(&mut self, chunk: &[u8]) {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
    }

    /// Return all events terminated by a blank line so far; keep any trailing
    /// partial block buffered. Blocks with neither `event:` nor `data:` (pure
    /// comment heartbeats) produce no event.
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

/// Find the end index (exclusive) of the first complete SSE block in `buf`,
/// including its terminating blank line. Returns `None` if no blank line
/// terminates a block yet (the trailing partial line stays buffered). Both `\n`
/// and `\r\n` line terminators are recognized; a trailing lone `\r` is left
/// buffered (it may be the first byte of an incoming `\r\n`).
fn block_end(buf: &str) -> Option<usize> {
    let bytes = buf.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        let nl = bytes[i..].iter().position(|&b| b == b'\n')? + i;
        // The line spanning `[i, nl)` is complete (a `\n` follows at `nl`).
        let line = &buf[i..nl];
        let stripped = line.strip_suffix('\r').unwrap_or(line);
        let after = nl + 1; // consume the `\n`
        if stripped.is_empty() {
            // Blank line: the block is buf[..after].
            return Some(after);
        }
        i = after;
    }
    None
}

/// Parse one complete SSE block (its blank-line terminator included) into an
/// event. Returns `None` for blocks with no `event:` and no `data:` field (pure
/// comment heartbeats). Multiple `data:` lines join with `\n`; one optional
/// space after a colon is stripped from each value.
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
            // Comment / keepalive (`: ping`). Ignore the whole line.
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v),
            None => continue, // Field name with no colon + empty value — irrelevant to us.
        };
        // One optional leading space after the colon is part of the format, not data.
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
            _ => {} // `id` / `retry` / unknown — ignored.
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
        // Fully drained — nothing left.
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
        // OpenRouter sends `: OPENROUTER PROCESSING` as a keepalive — it must
        // not produce a spurious empty ChatEvent downstream.
        let mut dec = SseDecoder::new();
        dec.push(b": OPENROUTER PROCESSING\n\n");
        assert!(dec.drain().is_empty());
    }

    #[test]
    fn partial_block_buffers_across_pushes() {
        let mut dec = SseDecoder::new();
        dec.push(b"data: hel");
        assert!(dec.drain().is_empty()); // no terminator yet
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
        // `data:foo` (no space) is equivalent to `data: foo`.
        dec.push(b"data:foo\n\n");
        assert_eq!(dec.drain()[0].data, "foo");
    }

    #[test]
    fn done_sentinel_decoded_verbatim_not_special_cased() {
        // OpenAI terminates with `data: [DONE]`; the decoder passes it through
        // unchanged so the OpenAI mapper can recognize it.
        let mut dec = SseDecoder::new();
        dec.push(b"data: [DONE]\n\n");
        let events = dec.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "[DONE]");
        assert!(events[0].event.is_none());
    }

    #[test]
    fn json_payload_preserved_verbatim() {
        // A realistic Anthropic delta block: event + a JSON data payload.
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
        // One complete event, then an incomplete line with no trailing newline.
        dec.push(b"data: done\n\ndata: incomplete");
        let events = dec.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "done");
        // The incomplete line survives for the next push.
        dec.push(b" continued\n\n");
        let events = dec.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "incomplete continued");
    }
}
