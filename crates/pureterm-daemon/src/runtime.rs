//! `puretermd` 的多线程 tokio 入口。

use std::sync::Arc;

use crate::control::{self, ControlState};
use crate::discovery;
use crate::session::SessionManager;

/// 运行守护进程直到进程被杀 / socket 被拆除。
///
/// 获取单实例锁,构建多线程 tokio runtime,并
/// 服务 JSON-RPC 控制面。每会话的 PTY 泵运行在普通
/// std 线程上(见 `session`);tokio 只驱动控制监听器。
pub fn run() -> std::io::Result<()> {
    let _ = env_logger::try_init();
    discovery::ensure_dirs()?;

    // 单实例:若其它 puretermd 持锁,安静退出(GUI
    // 的按需拉起随后会连上既有实例)。
    // 清扫上一次运行(崩溃 / 被杀 / 正常退出)留下的临时目录。
    // 只在启动时做一次 —— 扫描临时目录不该出现在热路径上。
    crate::shell_integration::sweep_stale_temp_dirs();

    let _lock = match discovery::acquire_instance_lock()? {
        Some(file) => file,
        None => {
            log::info!("another puretermd instance is running; exiting");
            return Ok(());
        }
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("puretermd-worker")
        .build()?;

    // 先于控制状态构建 runtime,使 GAP-5.2 隧道任务能
    // 派发到它上面(ControlState 持有句柄)。
    let state = Arc::new(ControlState::with_runtime(
        SessionManager::default(),
        runtime.handle().clone(),
    ));

    // 非关键初始化(known_hosts 打开、GAP-5.2 隧道恢复、GAP-7.3 CPU
    // 采样器)推迟到 `control::serve` 内控制 socket 绑定
    // 之后(见 `control::deferred_init`)。这避免冷启动 GUI
    // 在 spawn 轮询上阻塞约 5 秒等守护进程完成
    // 这些初始化后 socket 才可 ping。
    runtime.block_on(control::serve(discovery::socket_path(), state))
}
