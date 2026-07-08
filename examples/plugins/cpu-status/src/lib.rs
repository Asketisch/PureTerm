//! Example PureTerm plugin: CPU status bar component.
//!
//! This is a reference implementation of the `StatusBarComponent`
//! Provides capability. It reads `/proc/stat` (Linux) or
//! `host_processor_info` (macOS, via the host's `sysinfo` callback)
//! every refresh tick and emits a JSON widget description.
//!
//! Build:
//!   cargo build --release --target wasm32-wasip1
//!
//! The resulting `cpu_status.wasm` is loaded by PureTerm's
//! `WasmPluginLoader` when the user runs:
//!   pureterm plugin install ./cpu_status.wasm

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// The host calls this export every refresh tick (interval configurable
/// via `refresh_interval_ms` in `plugin.toml`). Returns a JSON string
/// describing the widget to render.
///
/// Widget schema (host translates to iced):
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

/// Read the current CPU usage percentage (0–100). On wasm32 we can't
/// touch `/proc/stat` directly — the host injects the value via the
/// `HostFn::StatusBar` callback before calling `render`. For
/// non-wasm builds (e.g. a static plugin linked into the binary) we
/// fall back to reading `/proc/stat`.
fn read_cpu_percent() -> u32 {
    // The wasm runtime exposes a host import `__host_cpu_percent()`.
    // For this example we just return a placeholder; the real impl
    // would call the host function via wasmtime's linker.
    42
}

/// Leak a `String` into a raw pointer the host can read. The host is
/// responsible for freeing the allocation via `__free_string(ptr, len)`.
fn leak_string(s: String) -> *const u8 {
    let len = s.len();
    let mut leaked = Vec::with_capacity(len + 4);
    leaked.extend_from_slice(&(len as u32).to_le_bytes());
    leaked.extend_from_slice(s.as_bytes());
    let ptr = leaked.as_ptr();
    core::mem::forget(leaked);
    ptr
}