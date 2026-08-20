//! PureTerm 插件示例:CPU 状态栏组件。
//!
//! 这是 `StatusBarComponent`
//! Provides 能力的参考实现。每个刷新周期读取 `/proc/stat`(Linux)或
//! `host_processor_info`(macOS,经宿主的 `sysinfo` 回调),
//! 然后输出一个 JSON 控件描述。
//!
//! 构建:
//!   构建命令:cargo build --release --target wasm32-wasip1
//!
//! 产出的 `cpu_status.wasm` 会在用户执行以下命令时
//! 被 PureTerm 的 `WasmPluginLoader` 加载:
//!   安装命令:pureterm plugin install ./cpu_status.wasm

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 宿主每个刷新周期调用此导出函数(间隔由
/// `plugin.toml` 中的 `refresh_interval_ms` 配置)。返回
/// 描述待渲染控件的 JSON 字符串。
///
/// 控件 schema(由宿主转成 iced):
///   {
///     "type": "text",
///     "content": "CPU 42%",
///     "color": "#58c4dc"
///   }
#[no_mangle]
pub extern "C" fn render() -> *const u8 {
    let pct = read_cpu_percent();
    let json = format!(
        r#"{{"type":"text","content":"CPU {}%","color":"#58c4dc"}}"#,
        pct
    );
    leak_string(json)
}

/// 读取当前 CPU 使用率(0–100)。wasm32 上无法
/// 直接访问 `/proc/stat`——宿主在调用 `render` 前
/// 通过 `HostFn::StatusBar` 回调注入该值。
/// 非 wasm 构建(如链接进二进制的静态插件)则
/// 回退为直接读取 `/proc/stat`。
fn read_cpu_percent() -> u32 {
    // wasm 运行时暴露宿主导入 `__host_cpu_percent()`。
    // 本示例直接返回占位值;真实实现
    // 会经宿主侧的 wasm 链接器调用该宿主函数。
    42
}

/// 把 `String` 泄漏为宿主可读的裸指针。宿主
/// 负责通过 `__free_string(ptr, len)` 释放该分配。
fn leak_string(s: String) -> *const u8 {
    let len = s.len();
    let mut leaked = Vec::with_capacity(len + 4);
    leaked.extend_from_slice(&(len as u32).to_le_bytes());
    leaked.extend_from_slice(s.as_bytes());
    let ptr = leaked.as_ptr();
    core::mem::forget(leaked);
    ptr
}