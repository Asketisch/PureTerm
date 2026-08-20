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

pub mod autostart;
pub mod control;
pub mod discovery;
pub mod frame;
pub mod idle_probe;
pub mod known_hosts;
pub mod osc7;
pub mod persist;
pub mod remote_fs;
pub mod ring_buffer;
pub mod shell_integration;
pub mod runtime;
pub mod session;
/// T-core-shell-integ v3 E2E (checklist D.5): preamble → OSC 133 解析器
/// 闭环验证。fish/pwsh/elv/nu 4 个 #[ignore] + 1 个 always-on 子项覆盖。
#[cfg(all(test, unix))]
mod shell_integ_e2e;
pub mod pipe_pane;
pub mod tunnel;
pub mod wait_for;

pub use discovery::{data_dir, known_hosts_file, lock_path, sessions_dir, socket_path, tunnels_file};
