//! LSP message framing: `Content-Length` header + JSON body.

use std::io::{self, BufRead, Write};

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("missing Content-Length header")]
    MissingLength,
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Encode a JSON value as an LSP-framed message (header + body).
pub fn encode_message(value: &Value) -> String {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "{}".into());
    format!("Content-Length: {}\r\n\r\n{body}", body.len())
}

/// Read one LSP-framed message from a buffered reader.
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
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

/// Write a framed message to any writer.
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
