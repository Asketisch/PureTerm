//! LSP 消息分帧:`Content-Length` 头 + JSON 体。

use std::io::{self, BufRead, Write};

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("missing Content-Length header")]
    MissingLength,
    #[error("Content-Length {0} exceeds limit")]
    OversizedMessage(usize),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// 单条 LSP 消息体上限。失控/恶意服务器发送超大 Content-Length
/// 时直接报错,防止 `vec![0u8; len]` 触发超大分配。
pub const MAX_MESSAGE_SIZE: usize = 32 * 1024 * 1024;

/// 把 JSON 值编码为 LSP 分帧消息(头 + 体)。
pub fn encode_message(value: &Value) -> String {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "{}".into());
    format!("Content-Length: {}\r\n\r\n{body}", body.len())
}

/// 从带缓冲的读取器读一条 LSP 分帧消息。
pub fn read_message<R: BufRead>(reader: &mut R) -> Result<Value, TransportError> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(TransportError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "lsp stream closed",
            )));
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = Some(rest.trim().parse::<usize>().map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, e)
            })?);
        }
    }
    let len = content_length.ok_or(TransportError::MissingLength)?;
    if len > MAX_MESSAGE_SIZE {
        return Err(TransportError::OversizedMessage(len));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

/// 向任意写入器写一条分帧消息。
pub fn write_message<W: Write>(writer: &mut W, value: &Value) -> io::Result<()> {
    writer.write_all(encode_message(value).as_bytes())?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrip_framing() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": { "uri": "file:///tmp/a.rs", "diagnostics": [] }
        });
        let framed = encode_message(&msg);
        let mut cursor = Cursor::new(framed.as_bytes());
        let parsed = read_message(&mut cursor).unwrap();
        assert_eq!(parsed["method"], "textDocument/publishDiagnostics");
    }
}
