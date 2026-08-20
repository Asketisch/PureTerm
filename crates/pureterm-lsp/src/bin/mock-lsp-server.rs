//! 测试夹具:一个基于 stdio 的极简 LSP 服务器。
//!
//! 供 `client.rs` 的回归测试使用:初始化握手(BUG-2)、
//! 优雅关停(BUG-4)与崩溃/读线程退出生命周期(BUG-3)。它是
//! 独立的二进制,测试可经 `CARGO_BIN_EXE_mock_lsp_server` 拉起。
//!
//! 参数:
//! - argv[1] = 场景
//! - argv[2] = 标记文件路径(可选);收到 `shutdown` 请求时写入。
//!
//! 场景:
//! - 未设置/"ok"            :完整的 `initialize`→`initialized`→`shutdown`→`exit` 流程。
//! - "fail-init"            :以 JSON-RPC 错误回复 `initialize`。
//! - "publish"              :同 "ok",但在 `didOpen` 后发布一条诊断。
//! - "crash-after-publish"  :在 `didOpen` 后发布一条诊断,然后关闭 stdout
//!                             (模拟服务器崩溃 → 读线程 EOF)。

use std::fs;
use std::io::{BufRead, Write};

use serde_json::{json, Value};

fn main() {
    let scenario = std::env::args().nth(1).unwrap_or_default();
    let marker = std::env::args().nth(2);

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let mut out = stdout.lock();

    while let Some(msg) = read_message(&mut reader) {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        match method {
            "initialize" => {
                let resp = if scenario == "fail-init" {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32603, "message": "init refused" }
                    })
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "capabilities": { "hoverProvider": true } }
                    })
                };
                let _ = write_msg(&mut out, &resp);
            }
            "shutdown" => {
                // 先写标记再回复,使在响应上解锁后
                // 即刻 SIGKILL 我们的客户端仍能观察到标记。
                if let Some(p) = &marker {
                    let _ = fs::write(p, "shutdown\n");
                }
                if let Some(id) = id {
                    let _ = write_msg(&mut out, &json!({ "jsonrpc": "2.0", "id": id, "result": null }));
                }
            }
            "exit" => break,
            "textDocument/didOpen" if scenario == "publish" || scenario == "crash-after-publish" => {
                let _ = write_msg(&mut out, &json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/publishDiagnostics",
                    "params": {
                        "uri": "file:///nonexistent/mock.rs",
                        "diagnostics": [{
                            "message": "mock diag",
                            "severity": 1,
                            "range": {
                                "start": { "line": 0, "character": 0 },
                                "end": { "line": 0, "character": 1 }
                            }
                        }]
                    }
                }));
                let _ = out.flush();
                if scenario == "crash-after-publish" {
                    // 模拟崩溃:停止读取并关闭 stdout。
                    return;
                }
            }
            _ => {}
        }
    }
}

/// 读取一条 Content-Length 分帧的 JSON 消息。EOF 时返回 `None`。
fn read_message<R: BufRead>(r: &mut R) -> Option<Value> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line).ok()?;
        if n == 0 {
            return None;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse::<usize>().ok();
        }
    }
    let len = content_length?;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn write_msg<W: Write>(w: &mut W, v: &Value) -> std::io::Result<()> {
    let body = serde_json::to_string(v)?;
    write!(w, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    w.flush()
}
