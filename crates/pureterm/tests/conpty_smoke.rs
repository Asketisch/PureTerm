//! T-core-windows-conpty v3 E2E(检查清单 E.6)。
//!
//! 仅 Windows 的集成测试:启动一个真实 ConPTY 支撑的
//! `cmd.exe` + `powershell.exe`,并确认子进程把横幅
//! 写回 conout 管道。这是第一个端到端验证
//! Windows 专属启动路径的测试;工作区其余
//! 测试都是 Unix 友好(或 `#[cfg(unix)]` 限定)的。
//!
//! 在 `windows-2022` runner 上运行:
//!   运行命令:cargo test -p pureterm --test conpty_smoke -- --nocapture
//!
//! 非 Windows 主机上整个文件编译为空——
//! 顶部的 `#![cfg(windows)]` 守卫会短路整个模块。
//!
//! ## 验证方式
//!
//! 1. 调用 `windows::open_conpty()`(`crates/pureterm/src/windows.rs`
//!    中的 v3 真实 ConPTY 实现)。函数返回一个
//!    拥有 `HPCON` 加两条匿名管道的 `ConptyHandle`。
//! 2. 以 `cmd.exe /c echo CONPTY-SMOKE-MARKER` 作为 ConPTY 子进程
//!    启动(经 `CreateProcessW` 加伪控制台属性列表)。
//! 3. 从 conout 端读取,直到出现标记(或 2 秒
//!    超时)。
//! 4. 关闭 ConPTY,回收子进程。
//!
//! 测试刻意保持最小——完整 PTY 语义(调整大小、
//! 备用屏幕、鼠标)由终端 crate 自带的
//! 测试套件覆盖;这里只验证 PureTerm 侧封装
//! 正确链接了 Win32 ConPTY API。

#![cfg(windows)]

use std::process::Command;
use std::time::{Duration, Instant};

/// 排空 conout,直到出现 `marker` 或 `timeout` 超时。
///
/// 真实 ConPTY 接通前没有调用点,故显式 allow(dead_code)。
/// 早期用 `let _ = wait_for_output_contains;` 压制告警,那行
/// 会让泛型参数 `impl Read` 无法推导(E0283),Windows 上直接
/// 编译失败 —— 故改为在定义处标注。
#[allow(dead_code)]
fn wait_for_output_contains(
    out: &mut impl std::io::Read,
    marker: &str,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    let mut buf = String::new();
    let mut chunk = [0u8; 1024];
    while Instant::now() < deadline {
        match out.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
                if buf.contains(marker) {
                    return true;
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
    false
}

#[test]
fn cmd_exe_smoke() {
    // 标记由 cmd 回显。预期 ConPTY 把 stdout
    // 透明地转发到我们的 conout 管道。
    const MARKER: &str = "CONPTY-CMD-SMOKE";

    // v3 实现说明:`windows::open_conpty` 存在于
    // `crates/pureterm/src/windows.rs`。v2 中它返回
    // `Err("ConPTY not wired on Windows in v2…")`;v3 换成真实的
    // `CreatePseudoConsole` + `CreateProcessW` 链路。
    //
    // 在完整 v3 ConPTY 实现落地之前,此测试体
    // 刻意使用 `todo!()`:文件在 Windows 上可编译,
    // 但运行时会 panic——让 CI 矩阵如实反映这一缺口。
    let _ = Command::new("cmd.exe")
        .args(["/c", &format!("echo {MARKER}")])
        .output();
    // 冒烟:cmd.exe 二进制本身可达。
    // TODO(t-core-windows-conpty v3+1):等 `windows::open_conpty` 接通后,
    // 替换为真实 ConPTY 启动。
    todo!("real ConPTY spawn pending windows::open_conpty v3+1");
}

#[test]
fn powershell_smoke() {
    const MARKER: &str = "CONPTY-PS-SMOKE";
    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &format!("Write-Output '{MARKER}'")])
        .output();
    // TODO(t-core-windows-conpty v3+1):等 `windows::open_conpty` 接通后,
    // 替换为真实 ConPTY 启动。
    todo!("real ConPTY spawn pending windows::open_conpty v3+1");
}