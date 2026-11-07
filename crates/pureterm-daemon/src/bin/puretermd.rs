//! `puretermd` — 常驻 PTY 守护进程二进制(GAP-5.1)。

fn main() {
    // 最小的 `--version` / `--help` 处理。
    // 此前本程序完全不解析参数:`puretermd --version` 会被静默忽略
    // 并**直接启动守护进程** —— 用户以为在问版本,实际起了后台服务。
    // 守护进程无 clap 依赖,这里手写判断即可(只认第一个参数)。
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("puretermd {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                println!("puretermd {} — PureTerm 持久化 PTY 守护进程", env!("CARGO_PKG_VERSION"));
                println!();
                println!("用法: puretermd");
                println!();
                println!("无参数启动守护进程(通常由 pureterm 自动拉起,无需手动运行)。");
                println!("选项:");
                println!("  -h, --help     显示本帮助");
                println!("  -V, --version  显示版本");
                return;
            }
            other => {
                eprintln!("puretermd: 未知参数 {other:?}");
                eprintln!("尝试 'puretermd --help'。");
                std::process::exit(2);
            }
        }
    }
    if let Err(e) = pureterm_daemon::runtime::run() {
        eprintln!("puretermd: {e}");
        std::process::exit(1);
    }
}
