//! `puretermd` — 常驻 PTY 守护进程(GAP-5.1)。
//!
//! 持有本地与(M3)SSH PTY 会话,使关闭 PureTerm GUI 不会
//! 杀死正在运行的 shell/SSH 子进程。GUI 是瘦客户端,经 Unix 域
//! 数据面镜像会话的 PTY 字节。
//!
//! 架构(见 GAP-5.1 方案):
//! - `discovery` — 稳定 socket 路径 + 数据目录 + 单实例锁
//! - `frame`     — 数据面长度前缀带标签帧编解码器
//! - `control`   — JSON-RPC 2.0 控制面(`session.*`、`daemon.*`)
//! - `session`   — 持有各 PTY 与每会话泵的 `SessionManager`
//! - `ring_buffer` — 固定容量回滚环(M2 回放源)
//! - `runtime`   — 多线程 tokio 入口 + 运行辅助
//!
//! 控制面复用 `pureterm-jsonrpc` 的传输无关类型。
//!
//! ## 平台边界
//!
//! 守护进程的 PTY 核心(`session` / `control` / `tunnel` 等)基于
//! Unix 域 socket、`openpty`、`libc` 信号与裸 fd,目前只支持 Unix。
//! Windows 上这些模块整体不编译;`puretermd` 二进制仍会产出,
//! 但 `runtime::run()` 明确返回 Unsupported,提示该平台上终端
//! 会话由 GUI 直连(ConPTY)管理、不经过守护进程。
//!
//! PureTerm GUI 在**所有**平台都依赖本 crate 的路径 / 帧编解码 /
//! shell 集成辅助层(`discovery` / `frame` / `shell_integration` /
//! `autostart` / `osc7`),故这些模块始终导出,内部 Unix-specific
//! 行由各文件内的 `#[cfg]` 隔离。

// ── 跨平台:GUI 依赖的配置 / 编解码 / shell 辅助层 ──────────────
pub mod autostart;
pub mod discovery;
pub mod frame;
pub mod osc7;
pub mod shell_integration;

// ── 仅 Unix:PTY 会话核心与控制面 ────────────────────────────────
#[cfg(unix)]
pub mod control;
#[cfg(unix)]
pub mod idle_probe;
#[cfg(unix)]
pub mod known_hosts;
#[cfg(unix)]
pub mod persist;
#[cfg(unix)]
pub mod remote_fs;
#[cfg(unix)]
pub mod ring_buffer;
#[cfg(unix)]
pub mod session;
#[cfg(unix)]
pub mod pipe_pane;
#[cfg(unix)]
pub mod tunnel;
#[cfg(unix)]
pub mod wait_for;

/// T-core-shell-integ v3 E2E (checklist D.5): preamble → OSC 133 解析器
/// 闭环验证。fish/pwsh/elv/nu 4 个 #[ignore] + 1 个 always-on 子项覆盖。
#[cfg(all(test, unix))]
mod shell_integ_e2e;

// runtime 是两个平台各一份实现(puretermd 二进制入口需要它)。
#[cfg(unix)]
#[path = "runtime.rs"]
mod runtime_impl;
#[cfg(windows)]
#[path = "runtime_windows.rs"]
mod runtime_impl;
pub mod runtime {
    pub use super::runtime_impl::run;
}

pub use discovery::{data_dir, known_hosts_file, lock_path, sessions_dir, socket_path, tunnels_file};
