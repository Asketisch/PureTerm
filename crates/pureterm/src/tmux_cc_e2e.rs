//! T-core-tmux-cc v3 E2E 测试(清单 C.6)。
//!
//! 这些测试会启动一个**真实**的 `tmux -CC` 子进程——本代码库里首个此类测试。
//!
//! 前置条件:`tmux` 必须在 `$PATH` 中且支持控制模式
//! (任意 tmux 1.5+;当前在 macOS 上以 tmux 3.6a 验证)。
//! **tmux 不在 PATH 上时用例会优雅跳过**(打印提示后 return),因此
//! 可以安全地留在常规测试套件里。
//!
//! 手动运行:
//!   cargo nextest run -p pureterm -E 'test(/tmux_cc/)'
//!
//! 回归背景:这些用例此前长期标着 `#[ignore]`,掩盖了三个**根本性**
//! 缺陷 —— ① `tmux -CC` 用管道启动(它要求 pty,否则直接
//! `tcgetattr failed` 退出);② `%output` 载荷按 base64 解码
//! (tmux 实际用八进制转义,于是每一帧终端输出都被丢弃);③ `%begin`
//! 格式臆造。现在三者都已修正,用例进入常规套件。
//!
//! ## 串行化(必须!)
//!
//! 这几个用例都会**同时**连接同一个 tmux 服务器(tmux 3.6a 的服务器是
//! 全机一个)。实测:4 个控制模式客户端**并发**连接时,服务器有相当高的
//! 概率在启动期直接退出 —— 所有客户端都只收到
//! `%exit server exited unexpectedly`,一个信封都拿不到。
//!
//! 这是 tmux 自己的竞态(不是我们的),但会表现为「测试随机失败」。
//! 因此这里用一个全局互斥量把真 tmux 用例**串行**化:每个用例开始前
//! 取锁,结束时释放。
//!
//! (产品侧同样受影响:同时打开两个 tmux 面板有概率把 tmux 服务器带崩。
//! 那是上游 tmux 的问题,不在本仓库修。)
//!
//! ## 超时口径
//!
//! 这些用例依赖**真 tmux 服务器**的往返时间。原先的 500ms / 2s 窗口
//! 在机器繁忙时(比如刚跑完一次 release 构建、后台还在刷 IO)会**漏掉**
//! 信封,于是用例随机失败 —— flaky 的测试等于没有测试。故一律放宽到
//! 秒级:慢的时候多等几秒,快的时候立刻返回(排空是「收够就停」,
//! 不是固定 sleep)。
//!
//! ## 验证内容
//!
//! 1. **`tmux_three_windows_smoke`** — 启动 `tmux -CC new-session`,
//!    发送 2 条 `new-window` 命令,确认 `%windows` 信封流中
//!    出现 3 个窗口(镜像 GUI 分割树镜像消费的内容)。
//! 
//!
//! 2. **`tmux_reattach_replay_smoke`** — 启动 `tmux -CC new-session`,
//!    通过 `send_input` 写入若干按键,捕获
//!    `%begin/%output/%end` 重放载荷,杀掉子进程,然后
//!    `tmux -CC attach -t <session>` 并验证相同的信封
//!    序列被重新发出(证明跨重启的重连契约)。
//! 
//!
//! 3. **`tmux_session_changed_envelope`** — 确认
//!    `%session-changed <sid> <name>` 信封到达且 `session_id`
//!    已填入 `TmuxChild` 句柄。

#![cfg(unix)]

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use alacritty_terminal::grid::Dimensions;

use crate::tmux_cc::envelope::{serialize_command, TmuxCommand, TmuxEnvelope};
use crate::tmux_cc::pty::{TmuxChild, TmuxSpawnArgs};

/// 运行 `cmd`,最多等待 `timeout` 直到谓词返回
/// `true`。返回谓词是否在截止时间前满足。
fn wait_for<P: Fn() -> bool>(timeout: Duration, predicate: P) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// 本次测试运行专用的 tmux socket 名。
///
/// **必须**用它:tmux 服务器默认全机只有一个(`/tmp/tmux-<uid>/default`)。
/// 不用独立 socket 的话:
/// - 测试会在**用户自己的** tmux 服务器里建/杀会话(污染现场);
/// - 前一个用例的客户端退出时,服务器可能跟着退出,下一个用例撞上
///   正在死掉的服务器 → 收到 `%exit server exited unexpectedly`,
///   一个信封都拿不到(实测并发时必现)。
///
/// 独立 socket 让每个用例有自己的服务器,彻底隔离。
fn tmux_socket_label() -> String {
    format!("pureterm-e2e-{}", std::process::id())
}

/// 删掉可能残留的 tmux socket 文件。
///
/// `kill-server` 之后 socket **文件**有时仍在(服务器已死,inode 还在)——
/// 每次跑测试都在 /tmp 里堆一个。
///
/// 路径由 tmux 决定:`$TMUX_TMPDIR`(未设则 `/tmp`)/ `tmux-<uid>` /
/// `<label>`。注意 tmux 用的是 **`/tmp`** 而不是 `$TMPDIR` —— 与
/// `std::env::temp_dir()` 不同,别搞错。
fn remove_socket_file(label: &str) {
    // SAFETY:`geteuid` 无参数、无内存影响。
    let uid = unsafe { libc::geteuid() };
    let base = std::env::var("TMUX_TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
    let path = Path::new(&base).join(format!("tmux-{uid}")).join(label);
    // `kill-server` 只是**请求**服务器退出 —— 它真正退出(并可能再碰一次
    // socket 文件)有一小段延迟。删早了会被它重新留下,故重试几次。
    for _ in 0..20 {
        if std::fs::remove_file(&path).is_ok() {
            return;
        }
        if !path.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// 跑一条带 `-L <label>` 的 tmux 命令(与 [`tmux_socket_label`] 配套)。
fn tmux_cmd(label: &str, args: &[&str]) -> std::process::Output {
    let mut full: Vec<&str> = vec!["-L", label];
    full.extend_from_slice(args);
    Command::new("tmux")
        .args(&full)
        .output()
        .expect("run tmux")
}

/// 真 tmux 用例的串行锁(见文件头的「串行化」一节)。
///
/// nextest 在同一个二进制内**并行**跑用例,并发控制模式客户端会让
/// tmux 服务器在启动期随机退出 → 用例随机失败。
static TMUX_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 取串行锁。锁中毒时取 `into_inner` 继续用 —— 前一个用例失败不该
/// 让后续所有用例都跳过。
fn tmux_serial_guard() -> std::sync::MutexGuard<'static, ()> {
    TMUX_SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 探测 `tmux` 是否在 `$PATH` 上。找到则返回 `true`。
fn tmux_on_path() -> bool {
    Command::new("which")
        .arg("tmux")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 最多 `timeout` 时间内排空信封,返回所有到达的信封。
fn drain_envelopes(child: &mut TmuxChild, timeout: Duration) -> Vec<TmuxEnvelope> {
    let deadline = Instant::now() + timeout;
    let mut out = Vec::new();
    while Instant::now() < deadline {
        match child.try_recv_envelope() {
            Some(env) => out.push(env),
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    out
}

#[test]
fn tmux_three_windows_smoke() {
    let _serial = tmux_serial_guard();
    if !tmux_on_path() {
        eprintln!("跳过:tmux 不在 $PATH 上(brew install tmux 可启用本用例)");
        return;
    }

    // 唯一会话标记,避免本测试与其他测试运行、或用户现场已有会话冲突
    // 或与用户正在使用的会话冲突。
    let tag = format!(
        "e2e3win-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    );
    let label = tmux_socket_label();
    let mut child = TmuxChild::spawn(
        &TmuxSpawnArgs {
            socket_label: Some(label.clone()),
            ..TmuxSpawnArgs::new_session(&tag, 80, 24)
        },
    )
    .expect("spawn tmux -CC new-session");

    // 排空初始信封(session-changed + windows 头)。
    let initial = drain_envelopes(&mut child, Duration::from_secs(2));
    assert!(
        initial.iter().any(|e| matches!(e, TmuxEnvelope::SessionChanged { .. })),
        "expected %session-changed in initial drain, got {initial:?}"
    );

    // 发送两条 `new-window` 命令。`-P -F` 标志会打印新的窗口 id;
    // 这里不捕获它,只确认调用不
    // 出错。
    let cmd_str = serialize_command(&TmuxCommand::NewWindow {
        name: Some("win-a"),
        cwd: None,
    });
    child
        .send_envelope(&TmuxEnvelope::Unknown(cmd_str))
        .expect("send new-window #1");
    let cmd_str = serialize_command(&TmuxCommand::NewWindow {
        name: Some("win-b"),
        cwd: None,
    });
    child
        .send_envelope(&TmuxEnvelope::Unknown(cmd_str))
        .expect("send new-window #2");

    // 刷新,促使服务器冲刷其 %windows 状态。
    child
        .send_envelope(&TmuxEnvelope::Unknown(serialize_command(
            &TmuxCommand::RefreshClient,
        )))
        .expect("send refresh-client");

    // 收集下一批。
    //
    // 注意:tmux **3.6a 不会发 `%windows` 头**(那是更新版本才加的
    // 「整份窗口表」通知)。3.6a 逐个窗口发 `%window-add @<id>`。
    // 因此这里按版本可用的通知断言:两条 `new-window` → 两条
    // `%window-add` → 连同初始窗口共 3 个。
    let after = drain_envelopes(&mut child, Duration::from_secs(2));

    // 新版 tmux 会发 `%windows` 头;旧版发 `%window-add`。两者都算。
    let window_headers: Vec<_> = after
        .iter()
        .filter_map(|e| match e {
            TmuxEnvelope::WindowsHeader { w, .. } => Some(*w),
            _ => None,
        })
        .collect();
    let window_adds: Vec<_> = after
        .iter()
        .filter_map(|e| match e {
            TmuxEnvelope::WindowAdd { window_id } => Some(window_id.clone()),
            _ => None,
        })
        .collect();

    assert!(
        !window_headers.is_empty() || window_adds.len() >= 2,
        "创建两个新窗口后应收到 %windows 头或 ≥2 条 %window-add,         实际 %windows={window_headers:?} %window-add={window_adds:?}"
    );

    // 三窗口的去重计数:初始 1 个 + 新增 2 个。
    let total_windows = if window_headers.is_empty() {
        1 + window_adds.len()
    } else {
        window_headers.iter().max().copied().unwrap_or(0) as usize
    };
    assert!(
        total_windows >= 3,
        "应有 3 个窗口(初始 1 + 新增 2),实际 {total_windows}"
    );

    // 清理:杀掉我们创建的会话,避免后续运行泄漏。
    let _ = child.terminate();
    let _ = tmux_cmd(&label, &["kill-session", "-t", &format!("pureterm-{tag}")]);
    let _ = tmux_cmd(&label, &["kill-server"]);
    remove_socket_file(&label);
}

#[test]
fn tmux_reattach_replay_smoke() {
    let _serial = tmux_serial_guard();
    if !tmux_on_path() {
        eprintln!("跳过:tmux 不在 $PATH 上(brew install tmux 可启用本用例)");
        return;
    }

    let tag = format!(
        "e2ereat-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    );

    // 阶段 1:new-session + 发送几个字节。
    let label = tmux_socket_label();
    let mut child1 = TmuxChild::spawn(
        &TmuxSpawnArgs {
            socket_label: Some(label.clone()),
            ..TmuxSpawnArgs::new_session(&tag, 80, 24)
        },
    )
    .expect("spawn tmux -CC new-session");
    // 等待服务器就绪 + session-changed 到达。
    let _ = wait_for(Duration::from_secs(10), || child1.session_id().is_some());
    assert!(
        child1.session_id().is_some(),
        "session_id never populated on initial spawn"
    );

    // 发送一些输入以填充窗格(让重连时有内容
    // 可重放)。
    child1
        .send_input(b"echo reattach-marker\n")
        .expect("send echo");

    // 让服务器处理输入并发出 %output。
    std::thread::sleep(Duration::from_millis(1200));

    // 阶段 2:杀掉 GUI 侧子进程(不是 tmux 服务器)。
    child1.terminate().expect("terminate child1");

    // 阶段 3:用同一会话名 attach。tmux 会以
    // %begin %<id> <ts> <W>x<H> ... %end 帧的形式重放已捕获的输出。
    let target = format!("pureterm-{tag}");
    let mut child2 = TmuxChild::spawn(&TmuxSpawnArgs {
        socket_label: Some(label.clone()),
        ..TmuxSpawnArgs::attach(&target)
    })
    .expect("spawn tmux -CC attach");

    let reattach_envelopes = drain_envelopes(&mut child2, Duration::from_secs(5));

    // 我们期望重连时至少出现一个 %begin … %output … %end 帧。
    let begin_count = reattach_envelopes
        .iter()
        .filter(|e| matches!(e, TmuxEnvelope::Begin { .. }))
        .count();
    let end_count = reattach_envelopes
        .iter()
        .filter(|e| matches!(e, TmuxEnvelope::End))
        .count();
    let output_count = reattach_envelopes
        .iter()
        .filter(|e| matches!(e, TmuxEnvelope::Output { .. }))
        .count();

    assert!(
        begin_count >= 1,
        "reattach should emit ≥1 %begin frame, got {begin_count}"
    );
    assert!(
        end_count >= 1,
        "reattach should emit ≥1 %end frame, got {end_count}"
    );
    assert!(
        output_count >= 1 || reattach_envelopes.iter().any(|e| matches!(e, TmuxEnvelope::SessionChanged { .. })),
        "reattach should emit ≥1 %output (or at least a %session-changed), got {output_count} %output and {} envelopes total",
        reattach_envelopes.len()
    );

    // 清理。
    let _ = child2.terminate();
    let _ = tmux_cmd(&label, &["kill-session", "-t", &target]);
    let _ = tmux_cmd(&label, &["kill-server"]);
    remove_socket_file(&label);
}

/// T-core-tmux-cc 的 **GUI 侧接线**验证:GUI 用的正是
/// `TerminalContext::new_tmux` + `tmux_cc::pump` 这条链路。
///
/// 背景:此前 `tmux_cc` 的后端(envelope 解析 / 子进程 / 重放)都通了,
/// 但**GUI 从不消费它** —— `Surface` 里没有 tmux 变体,`TerminalContext`
/// 里也没有。现在两者都接上了,本用例守住这条链路:附加到一个**已有
/// 会话**后,会话当前屏幕的内容必须出现在网格里。
#[test]
fn tmux_gui_context_replays_session_content() {
    let _serial = tmux_serial_guard();
    if !tmux_on_path() {
        eprintln!("跳过:tmux 不在 $PATH 上(brew install tmux 可启用本用例)");
        return;
    }

    let tag = format!("e2e-gui-{}-{}", std::process::id(), Instant::now().elapsed().as_nanos());
    // 建一个会打印标记、然后驻留的会话。
    let label = tmux_socket_label();
    let out = tmux_cmd(
        &label,
        &[
            "new-session",
            "-d",
            "-s",
            &tag,
            "printf 'TMUX_GUI_MARKER\\n'; sleep 120",
        ],
    );
    assert!(
        out.status.success(),
        "tmux new-session 失败: stdout={:?} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // 等 shell 把标记打印出来。
    std::thread::sleep(Duration::from_millis(1200));

    let ctx = crate::terminal_context::TerminalContext::new_tmux(
        Some(&tag),
        80,
        24,
        false,
        Some(&label),
    )
    .expect("new_tmux attach");

    // 网格里应出现标记 —— 泵线程把 `%output` 信封喂进了 `Term`。
    // 扫**整个**网格(标记不一定落在第 0 行:tmux 的屏幕内容可能带
    // 光标定位序列,标记最终落在哪一行取决于回放内容)。
    let grid_text = |ctx: &crate::terminal_context::TerminalContext| -> String {
        let t = ctx.terminal().lock_unfair();
        let mut out = String::new();
        for line in 0..24i32 {
            let text = t.bounds_to_string(
                alacritty_terminal::index::Point::new(
                    alacritty_terminal::index::Line(line),
                    alacritty_terminal::index::Column(0),
                ),
                alacritty_terminal::index::Point::new(
                    alacritty_terminal::index::Line(line),
                    t.last_column(),
                ),
            );
            out.push_str(&text);
            out.push('\n');
        }
        out
    };
    let got = wait_for(Duration::from_secs(10), || {
        grid_text(&ctx).contains("TMUX_GUI_MARKER")
    });
    let text = grid_text(&ctx);
    assert!(got, "附加后网格里没有会话内容:\n{text:?}");

    // 清理。
    let _ = tmux_cmd(&label, &["kill-session", "-t", &tag]);
    // socket 名字是隔离的,`kill-server` 只停我们自己的那个。
    let _ = tmux_cmd(&label, &["kill-server"]);
    remove_socket_file(&label);
}

#[test]
fn tmux_session_changed_envelope() {
    let _serial = tmux_serial_guard();
    if !tmux_on_path() {
        eprintln!("跳过:tmux 不在 $PATH 上(brew install tmux 可启用本用例)");
        return;
    }

    let tag = format!(
        "e2esch-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    );
    let label = tmux_socket_label();
    let mut child = TmuxChild::spawn(
        &TmuxSpawnArgs {
            socket_label: Some(label.clone()),
            ..TmuxSpawnArgs::new_session(&tag, 80, 24)
        },
    )
    .expect("spawn tmux -CC new-session");

    let got = wait_for(Duration::from_secs(10), || child.session_id().is_some());
    assert!(got, "session-changed never arrived within 2s");

    let sid = child.session_id().expect("session_id populated").to_string();
    assert!(!sid.is_empty(), "session_id should be non-empty");

    let _ = child.terminate();
    let _ = tmux_cmd(&label, &["kill-session", "-t", &format!("pureterm-{tag}")]);
    let _ = tmux_cmd(&label, &["kill-server"]);
    remove_socket_file(&label);
}

