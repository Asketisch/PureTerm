//! `puretermd` — 常驻 PTY 守护进程二进制(GAP-5.1)。

fn main() {
    if let Err(e) = pureterm_daemon::runtime::run() {
        eprintln!("puretermd: {e}");
        std::process::exit(1);
    }
}
