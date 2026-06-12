//! JSON-RPC 2.0 protocol types for PureTerm Socket API v2.
//!
//! Wire format: one JSON object per line (newline-delimited JSON-RPC).

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 constant version string.
pub const JSONRPC_VERSION: &str = "2.0";

// ── Standard JSON-RPC 2.0 error codes ──────────────────────────────────

/// Invalid JSON was received by the server.
pub const ERROR_PARSE: i64 = -32700;
/// The JSON sent is not a valid Request object.
pub const ERROR_INVALID_REQUEST: i64 = -32600;
/// The method does not exist / is not available.
pub const ERROR_METHOD_NOT_FOUND: i64 = -32601;
/// Invalid method parameter(s).
pub const ERROR_INVALID_PARAMS: i64 = -32602;
/// Internal JSON-RPC error.
pub const ERROR_INTERNAL: i64 = -32603;

// ── Request / Response ─────────────────────────────────────────────────

/// Incoming JSON-RPC 2.0 request from a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

/// Outgoing JSON-RPC 2.0 response to a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// ── Constructors ───────────────────────────────────────────────────────

impl JsonRpcRequest {
    /// Create a new request with the given id and method.
    pub fn new(id: u64, method: impl Into<String>) -> Self {
        Self { jsonrpc: JSONRPC_VERSION.into(), id, method: method.into(), params: None }
    }

    /// Create a new request with params.
    pub fn with_params(id: u64, method: impl Into<String>, params: serde_json::Value) -> Self {
        Self { jsonrpc: JSONRPC_VERSION.into(), id, method: method.into(), params: Some(params) }
    }

    /// Parse from a JSON string line.
    pub fn from_line(line: &str) -> Result<Self, JsonRpcError> {
        serde_json::from_str(line).map_err(|e| JsonRpcError {
            code: ERROR_PARSE,
            message: format!("Parse error: {e}"),
            data: None,
        })
    }
}

impl JsonRpcResponse {
    /// Successful response.
    pub fn ok(id: u64, result: serde_json::Value) -> Self {
        Self { jsonrpc: JSONRPC_VERSION.into(), id, result: Some(result), error: None }
    }

    /// Error response.
    pub fn err(id: u64, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message: message.into(), data: None }),
        }
    }

    /// Method-not-found response (convenience).
    pub fn method_not_found(id: u64, method: &str) -> Self {
        Self::err(id, ERROR_METHOD_NOT_FOUND, format!("Method not found: {method}"))
    }

    /// Internal error response (convenience).
    pub fn internal(id: u64, msg: impl Into<String>) -> Self {
        Self::err(id, ERROR_INTERNAL, msg)
    }

    /// Serialize to a single JSON line (with trailing newline).
    pub fn to_line(&self) -> String {
        let mut s = serde_json::to_string(self).unwrap_or_default();
        s.push('\n');
        s
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

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
