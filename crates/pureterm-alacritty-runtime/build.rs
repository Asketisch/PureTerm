use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let mut version = String::from(env!("CARGO_PKG_VERSION"));
    if let Some(commit_hash) = commit_hash() {
        version = format!("{version} ({commit_hash})");
    }
    println!("cargo:rustc-env=VERSION={version}");

    // t-core-windows-conpty (E.1): the original build.rs unconditionally
    // calls `embed_resource::compile("src/alacritty/windows/alacritty.rc",
    // …)` on Windows, but the file doesn't exist in this crate's tree —
    // the vendored Alacritty sources in `crates/alacritty_terminal/` are
    // the actual conpty home, not this one. The Windows build therefore
    // fails before reaching the main source. Fix: skip the embed if the
    // file isn't on disk; this keeps the v1 cargo build green on both
    // Linux and Windows, and the conpty resources can be re-attached
    // in a follow-up.
    #[cfg(windows)]
    {
        let rc_path = PathBuf::from("src/alacritty/windows/alacritty.rc");
        if rc_path.exists() {
            embed_resource::compile(rc_path.to_str().unwrap(), embed_resource::NONE)
                .manifest_required()
                .unwrap();
        } else {
            // The `.rc` is missing in this minimal runtime crate; the
            // main `pureterm` binary's Windows resources live in
            // `crates/pureterm/windows/`, not here. v1 is a no-op; the
            // build emits no manifest-required assertion, but cargo
            // still produces a valid exe. Re-enable in v2 once the
            // resource layout is decided.
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
