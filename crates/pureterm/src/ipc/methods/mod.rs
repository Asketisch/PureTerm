//! Socket 方法注册表——将方法名映射到处理函数。
//!
//! `SocketRouter` 持有 `HashMap<String, Handler>`,每个处理器
//! 接收可选参数与一个 `AppSnapshot`,返回 JSON 结果
//! (或错误)以及一个供主循环执行的可选副作用。
//!
//! GAP-6.1:覆盖 8 个命名空间的 30+ 个 JSON-RPC 方法。
//!
//! 模块拆分(满足 CLAUDE.md 文件行数规范,各文件 < 500 行):
//! - [`types`]:副作用枚举 `IpcSideEffect` 与只读快照 `AppSnapshot` 等数据类型。
//! - [`register_system`]:system / window / tab / workspace 命名空间注册。
//! - [`register_pane`]:pane / surface / shell.context / trigger.list 命名空间注册。
//! - [`register_config`]:config / ssh / debug 命名空间注册。
//! - [`register_agent`]:feed / resume / agent / events / notification 命名空间注册。
//! - [`tests`]:端到端 `dispatch` 单元测试。

mod register_agent;
mod register_config;
mod register_pane;
mod register_system;
mod types;
#[cfg(test)]
mod tests;

// Re-export 公共数据类型,使 `crate::ipc::methods::{AppSnapshot, ...}`
// 等外部引用路径保持不变(mod.rs 拆分前的对外契约)。
pub use types::{AppSnapshot, IpcSideEffect, SshPaneState, TriggerListEntry};

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::protocol::{JsonRpcError, JsonRpcResponse, ERROR_INVALID_PARAMS};

// ── 处理器类型 ────────────────────────────────────────────────────────────────

/// 方法处理器:接收可选参数 + 快照,返回
/// (JSON 结果或错误,以及可选副作用)。
pub(super) type Handler = Box<
    dyn Fn(Option<Value>, &AppSnapshot) -> (Result<Value, JsonRpcError>, Option<IpcSideEffect>)
        + Send
        + Sync,
>;

// ── SocketRouter(方法注册表) ──────────────────────────────────────────────────

/// 方法注册表。构造一次,包上 `Arc`,在多个任务间共享。
pub struct SocketRouter {
    handlers: HashMap<String, Handler>,
}

impl SocketRouter {
    /// 创建一个注册了全部内置方法的新路由器(30+ 个方法)。
    pub fn new() -> Arc<Self> {
        let mut router = Self { handlers: HashMap::new() };
        router.register_system();
        router.register_pane();
        router.register_config();
        router.register_agent();
        Arc::new(router)
    }

    /// 注册一个方法处理器。
    fn register(
        &mut self,
        method: impl Into<String>,
        handler: impl Fn(Option<Value>, &AppSnapshot) -> (Result<Value, JsonRpcError>, Option<IpcSideEffect>)
            + Send
            + Sync
            + 'static,
    ) {
        self.handlers.insert(method.into(), Box::new(handler));
    }

    /// 注册一个返回提示信息的空操作方法(用于平台桩)。
    fn register_noop(&mut self, method: &str, note: &str) {
        let note = note.to_string();
        self.handlers.insert(
            method.to_string(),
            Box::new(move |_params, _snap| (Ok(serde_json::json!({"note": note})), None)),
        );
    }

    /// 列出所有已注册的方法名(用于帮助/发现)。
    pub fn list_methods(&self) -> Vec<&str> {
        let mut methods: Vec<&str> = self.handlers.keys().map(|s| s.as_str()).collect();
        methods.sort();
        methods
    }

    /// 分发请求:查找对应的处理器并调用。
    ///
    /// 返回 `(响应,可选副作用)`。
    pub fn dispatch(
        &self,
        id: u64,
        method: &str,
        params: Option<Value>,
        snap: &AppSnapshot,
    ) -> (JsonRpcResponse, Option<IpcSideEffect>) {
        match self.handlers.get(method) {
            Some(handler) => {
                let (result, effect) = handler(params, snap);
                // 特殊分支:system.methods 需要从路由器返回方法列表。
                let response = match result {
                    Ok(v) if v.get("__list_methods__").is_some() => {
                        let methods: Vec<&str> = self.list_methods();
                        JsonRpcResponse::ok(id, serde_json::json!({"methods": methods, "count": methods.len()}))
                    }
                    Ok(v) => JsonRpcResponse::ok(id, v),
                    Err(err) => JsonRpcResponse {
                        jsonrpc: super::protocol::JSONRPC_VERSION.into(),
                        id,
                        result: None,
                        error: Some(err),
                    },
                };
                (response, effect)
            }
            None => (JsonRpcResponse::method_not_found(id, method), None),
        }
    }
}

// ── 工具函数 ─────────────────────────────────────────────────────────────────

/// 从参数中提取标签页/窗格索引。
///
/// 接受 `{"index": N}` 或裸数字作为参数。
/// 未给出索引时,回退到当前活动标签页。
/// 索引越界时返回错误。
pub(super) fn extract_index(params: Option<Value>, max: usize) -> Result<usize, JsonRpcError> {
    if max == 0 {
        return Err(JsonRpcError {
            code: ERROR_INVALID_PARAMS,
            message: "No items available".into(),
            data: None,
        });
    }

    let idx = params
        .as_ref()
        .and_then(|p| {
            p.get("index")
                .and_then(|v| v.as_u64())
                .or_else(|| p.as_u64())
        })
        .unwrap_or(0) as usize;

    if idx >= max {
        return Err(JsonRpcError {
            code: ERROR_INVALID_PARAMS,
            message: format!("Index {idx} out of range (0..{max})"),
            data: None,
        });
    }
    Ok(idx)
}

/// 不依赖外部 chrono 的简易 ISO 时间戳。
pub(super) fn chrono_now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("T{secs}")
}
