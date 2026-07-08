//! Example PureTerm plugin: IPC method `plugin.hello(name)`.
//!
//! Registers an `IpcMethod` Provides capability. The host calls the
//! `handle` export whenever a JSON-RPC request for `plugin.hello`
//! arrives over the IPC socket.
//!
//! Request params:
//!   { "name": "<string>" }
//!
//! Response:
//!   { "greeting": "Hello, <name>!" }

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Host-invoked entry point. Receives the JSON-RPC `params` as a raw
/// byte pointer + length, returns a JSON response string the same way.
///
/// The host's `HostFn::Ipc` callback parses `params` from
/// `serde_json::Value` and passes the serialized bytes here.
#[no_mangle]
pub extern "C" fn handle(params_ptr: *const u8, params_len: usize) -> *const u8 {
    // Read the params bytes (the host guarantees the pointer is valid
    // for `params_len` bytes during the call).
    let params = if params_ptr.is_null() || params_len == 0 {
        b""
    } else {
        unsafe { core::slice::from_raw_parts(params_ptr, params_len) }
    };

    // Minimal JSON parsing: extract the `name` field. A real plugin
    // would pull in `serde_json` (compiled to wasm); for this example
    // we do a naive substring search to keep the dep surface empty.
    let name = extract_name_field(params).unwrap_or("stranger");

    let greeting = format!(r#"{{"greeting":"Hello, {name}!"}}"#);
    leak_string(greeting)
}

/// Naive `"name":"<value>"` extractor. Returns `None` if the field
/// isn't found.
fn extract_name_field(json: &[u8]) -> Option<String> {
    let needle = b"\"name\"";
    let pos = json
        .windows(needle.len())
        .position(|w| w == needle)?;
    let after = &json[pos + needle.len()..];
    // Skip `:`
    let colon = after.iter().position(|&b| b == b':')?;
    let after_colon = &after[colon + 1..];
    // Skip whitespace, find opening quote.
    let quote_open = after_colon.iter().position(|&b| b == b'"')?;
    let rest = &after_colon[quote_open + 1..];
    let quote_close = rest.iter().position(|&b| b == b'"')?;
    let bytes = &rest[..quote_close];
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// Leak a `String` into a raw pointer (length-prefixed, little-endian).
fn leak_string(s: String) -> *const u8 {
    let len = s.len();
    let mut leaked = Vec::with_capacity(len + 4);
    leaked.extend_from_slice(&(len as u32).to_le_bytes());
    leaked.extend_from_slice(s.as_bytes());
    let ptr = leaked.as_ptr();
    core::mem::forget(leaked);
    ptr
}