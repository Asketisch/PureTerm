//! PureTerm 的 JSON-RPC 2.0 协议类型。
//!
//! 与传输层无关：纯 serde 类型，不含任何 socket 代码。GUI 的 Socket API
//!（`crates/pureterm/src/ipc/`）与 `puretermd` 守护进程控制面共用同一套类型，
//! 保证客户端与服务端永不漂移。
//!
//! 线上格式：每行一个 JSON 对象（换行分隔的 JSON-RPC）。

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 协议版本号常量。
pub const JSONRPC_VERSION: &str = "2.0";

// ── 标准 JSON-RPC 2.0 错误码 ────────────────────────────────────────────────

/// 服务端收到无法解析的 JSON。
pub const ERROR_PARSE: i64 = -32700;
/// 发送的 JSON 不是合法的 Request 对象。
pub const ERROR_INVALID_REQUEST: i64 = -32600;
/// 方法不存在或不可用。
pub const ERROR_METHOD_NOT_FOUND: i64 = -32601;
/// 方法参数非法。
pub const ERROR_INVALID_PARAMS: i64 = -32602;
/// JSON-RPC 内部错误。
pub const ERROR_INTERNAL: i64 = -32603;

// ── 请求 / 响应 ────────────────────────────────────────────────────────────

/// 客户端发来的 JSON-RPC 2.0 请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

/// 返回给客户端的 JSON-RPC 2.0 响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 错误对象。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// ── 构造函数 ───────────────────────────────────────────────────────────────

impl JsonRpcRequest {
    /// 用给定的 id 和方法名创建一个新请求。
    pub fn new(id: u64, method: impl Into<String>) -> Self {
        Self { jsonrpc: JSONRPC_VERSION.into(), id, method: method.into(), params: None }
    }

    /// 创建一个带参数的新请求。
    pub fn with_params(id: u64, method: impl Into<String>, params: serde_json::Value) -> Self {
        Self { jsonrpc: JSONRPC_VERSION.into(), id, method: method.into(), params: Some(params) }
    }

    /// 从一行 JSON 字符串解析请求。
    pub fn from_line(line: &str) -> Result<Self, JsonRpcError> {
        serde_json::from_str(line).map_err(|e| JsonRpcError {
            code: ERROR_PARSE,
            message: format!("Parse error: {e}"),
            data: None,
        })
    }
}

impl JsonRpcResponse {
    /// 成功响应。
    pub fn ok(id: u64, result: serde_json::Value) -> Self {
        Self { jsonrpc: JSONRPC_VERSION.into(), id, result: Some(result), error: None }
    }

    /// 错误响应。
    pub fn err(id: u64, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message: message.into(), data: None }),
        }
    }

    /// 方法不存在的响应（便捷构造函数）。
    pub fn method_not_found(id: u64, method: &str) -> Self {
        Self::err(id, ERROR_METHOD_NOT_FOUND, format!("Method not found: {method}"))
    }

    /// 内部错误响应（便捷构造函数）。
    pub fn internal(id: u64, msg: impl Into<String>) -> Self {
        Self::err(id, ERROR_INTERNAL, msg)
    }

    /// 序列化为单行 JSON（末尾带换行符）。
    pub fn to_line(&self) -> String {
        let mut s = serde_json::to_string(self).unwrap_or_default();
        s.push('\n');
        s
    }
}

// ── 测试 ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_request() {
        let req = JsonRpcRequest::new(1, "system.ping");
        let json = serde_json::to_string(&req).unwrap();
        let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 1);
        assert_eq!(parsed.method, "system.ping");
        assert_eq!(parsed.jsonrpc, "2.0");
    }

    #[test]
    fn roundtrip_response_ok() {
        let resp = JsonRpcResponse::ok(42, serde_json::json!({"version": "0.1.0"}));
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 42);
        assert!(parsed.result.is_some());
        assert!(parsed.error.is_none());
    }

    #[test]
    fn roundtrip_response_err() {
        let resp = JsonRpcResponse::method_not_found(7, "foo.bar");
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 7);
        assert!(parsed.result.is_none());
        let err = parsed.error.unwrap();
        assert_eq!(err.code, ERROR_METHOD_NOT_FOUND);
    }

    #[test]
    fn from_line_valid() {
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"system.version"}"#;
        let req = JsonRpcRequest::from_line(line).unwrap();
        assert_eq!(req.method, "system.version");
    }

    #[test]
    fn from_line_invalid() {
        let line = "not json";
        let err = JsonRpcRequest::from_line(line).unwrap_err();
        assert_eq!(err.code, ERROR_PARSE);
    }

    #[test]
    fn to_line_ends_with_newline() {
        let resp = JsonRpcResponse::ok(1, serde_json::json!(null));
        assert!(resp.to_line().ends_with('\n'));
    }
}
