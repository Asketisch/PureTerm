//! `pureterm daemon install|uninstall|status` — 管理常驻的 `puretermd`
//! 守护进程及其登录自启(GAP-5.1 M4)。
//!
//! 与其它 CLI 组不同,这些命令不经过运行中的
//! GUI 的 IPC socket:`install`/`uninstall` 直接改动 OS 服务定义,
//! 而 `status` 与守护进程自身的控制 socket 通信。

use crate::cli::DaemonSub;

pub fn dispatch(cmd: DaemonSub) {
    use pureterm_daemon::autostart;

    match cmd {
        DaemonSub::Status => {
            let socket = pureterm_daemon::socket_path();
            let installed = autostart::is_installed();
            let loaded = autostart::is_loaded();
            let running = crate::daemon_client::ping(&socket);
            println!("puretermd status:");
            println!("  autostart installed : {installed}");
            println!("  service loaded      : {loaded}");
            println!("  daemon reachable    : {running}");
            println!("  socket              : {}", socket.display());
            std::process::exit(if running { 0 } else { 1 });
        }
        DaemonSub::Install => match autostart::install() {
            Ok(()) => println!(
                "installed puretermd login autostart → {}",
                autostart::puretermd_path().display()
            ),
            Err(e) => {
                eprintln!("install failed: {e}");
                std::process::exit(1);
            }
        },
        DaemonSub::Uninstall => match autostart::uninstall() {
            Ok(()) => println!("removed puretermd login autostart"),
            Err(e) => {
                eprintln!("uninstall failed: {e}");
                std::process::exit(1);
            }
        },
    }
}
