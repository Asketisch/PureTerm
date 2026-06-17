//! `puretermd` — persistent PTY daemon binary (GAP-5.1).

fn main() {
    if let Err(e) = pureterm_daemon::runtime::run() {
        eprintln!("puretermd: {e}");
        std::process::exit(1);
    }
}
