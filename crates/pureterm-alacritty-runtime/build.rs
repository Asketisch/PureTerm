use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let mut version = String::from(env!("CARGO_PKG_VERSION"));
    if let Some(commit_hash) = commit_hash() {
        version = format!("{version} ({commit_hash})");
    }
    println!("cargo:rustc-env=VERSION={version}");

    // t-core-windows-conpty(E.1):原 build.rs 在 Windows 上无条件
    // 调用 `embed_resource::compile("src/alacritty/windows/alacritty.rc",
    // …)`,但该文件在本 crate 的目录树里不存在——
    // `crates/alacritty_terminal/` 里 vendored 的 Alacritty 源码
    // 才是 conpty 的真正所在地,不是这里。Windows 构建因此在
    // 进入主源码之前就失败。修复:文件不在磁盘上时跳过
    // embed;这使 v1 的 cargo 构建在 Linux 与
    // Windows 上都能保持绿色,conpty 资源可在后续
    // 重新接回。
    #[cfg(windows)]
    {
        let rc_path = PathBuf::from("src/alacritty/windows/alacritty.rc");
        if rc_path.exists() {
            embed_resource::compile(rc_path.to_str().unwrap(), embed_resource::NONE)
                .manifest_required()
                .unwrap();
        } else {
            // 本极简运行时 crate 中缺少 `.rc`;主 `pureterm`
            // 二进制的 Windows 资源位于
            // `crates/pureterm/windows/`,不在这里。v1 为空操作;
            // 构建不发出任何需要 manifest 的断言,但 cargo
            // 仍产出有效可执行文件。待 v2 资源
            // 布局确定后再启用。
        }
    }
}

fn commit_hash() -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|hash| hash.trim().into())
}
