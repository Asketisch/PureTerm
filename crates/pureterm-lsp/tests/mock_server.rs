//! GAP-35 LSP 客户端生命周期缺陷的回归测试。
//!
//! 这些测试驱动一个模拟 LSP 服务器(`src/bin/mock-lsp-server.rs`)走真实 stdio,
//! 覆盖初始化握手(BUG-2)、优雅关停(BUG-4)与
//! 崩溃/读线程退出生命周期(BUG-3)。放在 `tests/` 是为了让 Cargo 注入
//! `CARGO_BIN_EXE_mock_lsp_server`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use pureterm_lsp::{LspClient, LspClientConfig};

/// 定位与本测试二进制一同构建的 `mock-lsp-server` 二进制。
///
/// 首选 Cargo 为集成测试注入的 `CARGO_BIN_EXE_mock_lsp_server`
/// (bin target 名中的 `-` 会被替换成 `_`)。它始终指向**本次构建**
/// 产出的二进制;早先用「从 `current_exe` 向上走两级猜 `target/<profile>`」
/// 的启发式,会在 `target/` 里残留旧版本二进制时静默拿到过期文件 ——
/// mock server 源码改动后测试仍连到旧二进制,表现为 initialize 握手
/// 超时,而服务端本身并无问题。
///
/// 回退到路径启发式仅用于没有该环境变量的场景(例如直接 `rustc` 编译)。
fn mock_exe() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_mock_lsp_server") {
        let path = PathBuf::from(path);
        if path.exists() {
            return path;
        }
    }
    let exe = std::env::current_exe().expect("current_exe");
    let profile_dir = exe
        .parent()
        .and_then(|deps| deps.parent())
        .expect("locate target profile dir");
    profile_dir.join("mock-lsp-server")
}

fn mock_cfg(scenario: &str, marker: Option<&str>) -> LspClientConfig {
    let mut args = vec![scenario.to_string()];
    if let Some(m) = marker {
        args.push(m.to_string());
    }
    LspClientConfig {
        command: mock_exe().to_string_lossy().into_owned(),
        args,
        workspace_root: PathBuf::from("."),
    }
}

/// BUG-2:`initialize` 必须读取服务器响应并让拒绝浮出水面,
/// 而不是发后即忘。
#[test]
fn spawn_fails_when_server_rejects_initialize() {
    let result = LspClient::spawn(mock_cfg("fail-init", None));
    match result {
        Ok(_) => panic!("spawn must surface a server-side initialize error, but succeeded"),
        Err(_e) => {}
    }
}

/// BUG-4:`Drop` 必须完成优雅的 shutdown/exit 握手,而不是
/// 直接 SIGKILL 服务器。
#[test]
fn drop_sends_graceful_shutdown() {
    let dir = tempfile::tempdir().expect("tempdir");
    let marker = dir.path().join("shutdown.marker");

    {
        let _client = LspClient::spawn(mock_cfg("ok", marker.to_str())).expect("spawn ok");
    } // dropped here

    let mut content = String::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        content = std::fs::read_to_string(&marker).unwrap_or_default();
        if content.contains("shutdown") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        content.contains("shutdown"),
        "Drop must send a `shutdown` request the server observes; marker was {content:?}"
    );
}

/// BUG-3:服务器崩溃后,诊断必须被清空且客户端
/// 标记为已死(不再向状态栏 / AI 聊天提供过期诊断)。
#[test]
fn clears_diagnostics_and_marks_dead_after_server_crash() {
    let client = LspClient::spawn(mock_cfg("crash-after-publish", None)).expect("spawn ok");

    // didOpen 使模拟服务器发布一条诊断,然后关闭 stdout。
    let file = tempfile::Builder::new()
        .suffix(".rs")
        .tempfile()
        .expect("tempfile");
    std::fs::write(file.path(), "fn main() {}").expect("write");
    let _ = client.did_open(file.path());

    // 等待读线程抵达 EOF 并作出反应。
    let deadline = Instant::now() + Duration::from_secs(3);
    while client.is_alive() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!client.is_alive(), "client must be marked dead after crash");

    let summary = client.summary();
    assert_eq!(
        summary.errors, 0,
        "stale diagnostics must be cleared after the server died"
    );
}
