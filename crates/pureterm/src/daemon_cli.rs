//! `pureterm daemon install|uninstall|status` — manage the persistent `puretermd`
//! daemon and its login autostart (GAP-5.1 M4).
//!
//! Unlike the other CLI groups these commands do NOT go through the running
//! GUI's IPC socket: `install`/`uninstall` touch the OS service definition
//! directly, and `status` talks to the daemon's own control socket.

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
