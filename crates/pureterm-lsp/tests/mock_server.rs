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
/// Cargo 把测试可执行文件放在 `<target>/<profile>/deps/<name>-<hash>`,
/// 而包的二进制在 `<target>/<profile>/<bin>`,因此从测试
/// 可执行文件向上走两级即到 profile 目录。(手动完成,因为本 workspace
/// 不注入 `CARGO_BIN_EXE_*`。)
fn mock_exe() -> PathBuf {
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
