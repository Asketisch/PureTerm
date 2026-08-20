//! PureTerm 插件示例:IPC 方法 `plugin.hello(name)`。
//!
//! 注册 `IpcMethod` Provides 能力。当 JSON-RPC 请求
//! 经 IPC socket 到达 `plugin.hello` 时,宿主调用
//! `handle` 导出函数。
//!
//! 请求参数:
//!   { "name": "<string>" }
//!
//! 响应:
//!   { "greeting": "Hello, <name>!" }

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 宿主调用的入口函数。以裸字节指针 + 长度接收
/// JSON-RPC `params`,并以同样的方式返回 JSON 响应字符串。
///
/// 宿主的 `HostFn::Ipc` 回调把 `params` 从
/// `serde_json::Value` 取出,并将序列化后的字节传到这里。
#[no_mangle]
pub extern "C" fn handle(params_ptr: *const u8, params_len: usize) -> *const u8 {
    // 读取 params 字节(宿主保证指针在调用期间
    // 对 `params_len` 个字节有效)。
    let params = if params_ptr.is_null() || params_len == 0 {
        b""
    } else {
        unsafe { core::slice::from_raw_parts(params_ptr, params_len) }
    };

    // 极简 JSON 解析:提取 `name` 字段。真实插件
    // 会引入 `serde_json`(编译为 wasm);本示例
    // 只做朴素的子串搜索,以保持依赖面为空。
    let name = extract_name_field(params).unwrap_or("stranger");

    let greeting = format!(r#"{{"greeting":"Hello, {name}!"}}"#);
    leak_string(greeting)
}

/// 朴素的 `"name":"<value>"` 提取器。找不到字段
/// 时返回 `None`。
fn extract_name_field(json: &[u8]) -> Option<String> {
    let needle = b"\"name\"";
    let pos = json
        .windows(needle.len())
        .position(|w| w == needle)?;
    let after = &json[pos + needle.len()..];
    // 跳过 `:`。
    let colon = after.iter().position(|&b| b == b':')?;
    let after_colon = &after[colon + 1..];
    // 跳过空白,找到起始引号。
    let quote_open = after_colon.iter().position(|&b| b == b'"')?;
    let rest = &after_colon[quote_open + 1..];
    let quote_close = rest.iter().position(|&b| b == b'"')?;
    let bytes = &rest[..quote_close];
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// 把 `String` 泄漏为裸指针(长度前缀,小端)。
fn leak_string(s: String) -> *const u8 {
    let len = s.len();
    let mut leaked = Vec::with_capacity(len + 4);
    leaked.extend_from_slice(&(len as u32).to_le_bytes());
    leaked.extend_from_slice(s.as_bytes());
    let ptr = leaked.as_ptr();
    core::mem::forget(leaked);
    ptr
}