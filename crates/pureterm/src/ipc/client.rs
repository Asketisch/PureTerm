//! PureTerm Socket API 的 CLI 客户端。
//!
//! 连接到运行中的 PureTerm 实例的 Unix socket,发送一条 JSON-RPC
//! 请求,并返回解析后的响应。

use std::io::{BufRead, BufReader, Write};

use super::listener::find_socket;
use super::protocol::{JsonRpcRequest, JsonRpcResponse};

/// 向运行中的 PureTerm 实例发送一条 JSON-RPC 请求。
///
/// 成功时返回解析出的 `result` 值(若响应里无该字段则返回 `None`),
/// 失败时返回 `Err` 消息。展示(经 `--json` 紧凑或美观)与
/// 退出处理由调用方负责——GAP-6.2 把两者移出 socket 桥,
/// 以便各 CLI 命令组统一格式化
/// 输出。
pub fn send_rpc(
    method: &str,
    params: Option<serde_json::Value>,
    socket_override: Option<std::path::PathBuf>,
) -> Result<Option<serde_json::Value>, String> {
    let stream = find_socket(socket_override).map_err(|e| format!("Cannot connect to PureTerm: {e}"))?;

    // 用简单的递增 ID 构建请求。
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let request = if let Some(p) = params {
        JsonRpcRequest::with_params(id, method, p)
    } else {
        JsonRpcRequest::new(id, method)
    };

    // 序列化并发送。
    let mut json = serde_json::to_string(&request).map_err(|e| format!("Serialize error: {e}"))?;
    json.push('\n');

    let mut stream = stream;
    stream.write_all(json.as_bytes()).map_err(|e| format!("Write error: {e}"))?;
    stream.shutdown(std::net::Shutdown::Write).map_err(|e| format!("Shutdown error: {e}"))?;

    // 读取响应。
    let mut buffer = String::new();
    let mut reader = BufReader::new(&stream);
    reader.read_line(&mut buffer).map_err(|e| format!("Read error: {e}"))?;

    if buffer.trim().is_empty() {
        return Err("No response from PureTerm".into());
    }

    let resp: JsonRpcResponse =
        serde_json::from_str(&buffer).map_err(|e| format!("Invalid response: {e}\nRaw: {buffer}"))?;

    if let Some(err) = &resp.error {
        return Err(format!("Error ({}): {}", err.code, err.message));
    }

    Ok(resp.result)
}
