//! GAP-6.6: `tmux_cli` 模块的单元测试 + round-trip e2e。
//!
//! 拆分自原 `tmux_cli.rs` 末尾的 `mod tests`,逻辑不变:
//! - 顶层 9 个用例覆盖纯函数 `parse_target` / `map_key` / `render_format`。
//! - `round_trip` 子模块拉起一个 fake-GUI Unix socket(跑真实的 `SocketRouter`),
//!   直接驱动 [`super::sub::run`] 端到端验证 capture-pane / send-keys /
//!   join-pane 把正确的 method+params 送到底层。
//!
//! `super` 在这里指向模块根 `tmux_cli`;各被测符号通过子模块的 `pub(crate)` /
//! `pub(super)` 暴露,这里用具体 `use` 而不是 glob,保持依赖清晰。

use super::sub::{run, TmuxSub};
use super::target::{parse_target, PaneInfo};
use super::format::{map_key, render_format};

use serde_json::Value;

#[cfg(unix)]
use crate::ipc::methods::{AppSnapshot, IpcSideEffect, SocketRouter};
#[cfg(unix)]
use std::io::{Read, Write};

fn pane(id: u64, idx: usize, active: bool) -> PaneInfo {
    PaneInfo {
        pane_id: id,
        pane_index: idx,
        tab: 0,
        tab_title: "tab".into(),
        active,
        cwd: Some("/home/u".into()),
    }
}

fn sample() -> Vec<PaneInfo> {
    vec![pane(1, 0, true), pane(2, 1, false), pane(3, 2, false)]
}

#[test]
fn target_omitted_returns_active() {
    let panes = sample();
    assert_eq!(parse_target(&None, &panes).unwrap(), 1);
}

#[test]
fn target_percent_exact_pane_id() {
    let panes = sample();
    assert_eq!(parse_target(&Some("%3".into()), &panes).unwrap(), 3);
}

#[test]
fn target_percent_zero_falls_back_to_first() {
    // pane id 从 1 开始,因此 `%0` 无精确匹配 → DFS 索引 0。
    let panes = sample();
    assert_eq!(parse_target(&Some("%0".into()), &panes).unwrap(), 1);
}

#[test]
fn target_percent_out_of_range_errors() {
    let panes = sample(); // id 1,2,3 — 无 id 9,且 DFS 索引 9 越界
    assert!(parse_target(&Some("%9".into()), &panes).is_err());
}

#[test]
fn target_next_wraps() {
    let panes = sample(); // 活动 = 窗格 1(idx 0)
    assert_eq!(parse_target(&Some(":+".into()), &panes).unwrap(), 2);
}

#[test]
fn target_next_wraps_around() {
    let mut panes = sample();
    panes[0].active = false;
    panes[2].active = true; // 窗格 3 活动(idx 2,最后一个)
    assert_eq!(parse_target(&Some(":+".into()), &panes).unwrap(), 1);
}

#[test]
fn target_prev_wraps() {
    let panes = sample(); // 活动 = 窗格 1(idx 0)
    assert_eq!(parse_target(&Some(":-".into()), &panes).unwrap(), 3);
}

#[test]
fn target_unsupported_errors() {
    let panes = sample();
    assert!(parse_target(&Some("session:0.1".into()), &panes).is_err());
}

#[test]
fn map_key_translates_names() {
    assert_eq!(map_key("Enter"), "\r");
    assert_eq!(map_key("Tab"), "\t");
    assert_eq!(map_key("Escape"), "\x1b");
    assert_eq!(map_key("Up"), "\x1b[A");
    assert_eq!(map_key("ls"), "ls");
    assert_eq!(map_key("-la"), "-la");
}

#[test]
fn render_format_substitutes() {
    let p = pane(7, 2, true);
    let out =
        render_format("#{pane_id} #{pane_index} #{pane_active} #{pane_current_path}", &p);
    assert_eq!(out, "7 2 1 /home/u");
}

#[test]
fn render_format_inactive() {
    let p = pane(7, 2, false);
    assert_eq!(render_format("#{pane_active}", &p), "0");
}

// ── GAP-6.6: CLI ↔ SocketRouter round-trip(跨模块接缝) ──────────────────────────────
//
// 上面的单元测试单独覆盖 parse_target / map_key / render_format。
// 以下用例通过一个运行真实 `SocketRouter` 的 fake-GUI Unix socket,端到端驱动
// `run()`(即 `dispatch` 下的私有入口),证明 CLI 序列化出正确的 method+params,
// 证明 CLI 序列化出正确的 method+params,且路由器发出
// 且路由器发出正确的 `IpcSideEffect`。复用 `agent_e2e.rs` 的 fake-socket 模式。
#[cfg(unix)]
mod round_trip {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    /// 假 GUI 观察到的内容:它分发过的每个 (method, params) 以及
    /// 路由器发出的每个副作用。
    #[derive(Default)]
    struct Recorded {
        calls: Vec<(String, Value)>,
        effects: Vec<IpcSideEffect>,
    }

    fn unique_socket_path(label: &str) -> PathBuf {
        static COUNTER: OnceLock<AtomicU64> = OnceLock::new();
        let n = COUNTER
            .get_or_init(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("pureterm_tmux_{label}_{n}_{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    /// 2 个标签页:标签 0 = 窗格 {10(活动), 11};标签 1 = 窗格 {20(活动)}。
    /// `%0` 解析到第一个窗格(id 10);标签索引 1 有效。
    fn tmux_snap() -> AppSnapshot {
        AppSnapshot {
            window_count: 1,
            window_tab_counts: vec![1],
            active_window_index: 0,
            tab_titles: vec!["a".into(), "b".into()],
            tab_ids: vec![1, 2],
            pane_counts: vec![2, 1],
            active_tab_index: 0,
            version: "0.1.0".into(),
            ssh_profile_names: vec![],
            config_path: None,
            tab_cwds: vec![None, None],
            active_pane_ids: vec![10, 20],
            pane_ids: vec![vec![10, 11], vec![20]],
            ssh_pane_states: Vec::new(),
            pane_shell_contexts: std::collections::HashMap::new(),
            triggers: Vec::new(),
            workspace_ids: vec![1],
            workspace_names: vec!["Default".into()],
            active_workspace_id: 1,
        }
    }

    /// 假 GUI:绑定 socket,对每个连接运行真实的 `SocketRouter`,
    /// 记录方法 + 副作用。每次 `send_rpc` 打开一条新连接(客户端在请求后
    /// 关闭其写端,因此 `read_to_string` 恰好返回一个请求)。
    /// 请求,因此 `read_to_string` 恰好返回一个请求)。
    /// 采用非阻塞轮询,以便 `run()` 返回后停止该测试线程。
    #[allow(clippy::type_complexity)]
    /// 启动假 GUI。返回 `(记录, 停止标志, **就绪信号**, 线程句柄)`。
    ///
    /// 就绪信号在监听器**真的绑定成功之后**才发出 —— 早前这里靠一个
    /// 固定的 `sleep(60ms)` 等绑定,在高负载(1513 个测试 / 18 个
    /// worker 并行)下 60ms 经常不够,于是客户端连不上,用例随机失败。
    /// 信号是确定性的:绑定成功就发,绑定失败就发一个「失败」标记。
    fn spawn_fake_gui(
        path: PathBuf,
        snap: AppSnapshot,
    ) -> (
        Arc<Mutex<Recorded>>,
        Arc<AtomicBool>,
        Arc<(Mutex<bool>, std::sync::Condvar)>,
        thread::JoinHandle<()>,
    ) {
        let rec = Arc::new(Mutex::new(Recorded::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let ready = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let (rec_cl, stop_cl) = (Arc::clone(&rec), Arc::clone(&stop));
        let ready_cl = Arc::clone(&ready);
        let handle = thread::spawn(move || {
            let listener = match UnixListener::bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("fake_gui bind failed: {e}");
                    // 绑定失败也要唤醒等待者,免得它干等。
                    let (lock, cvar) = &*ready_cl;
                    *lock.lock().unwrap() = true;
                    cvar.notify_all();
                    return;
                }
            };
            // 监听器就绪 —— 唤醒等待者。
            {
                let (lock, cvar) = &*ready_cl;
                *lock.lock().unwrap() = true;
                cvar.notify_all();
            }
            let _ = listener.set_nonblocking(true);
            let router = SocketRouter::new();
            while !stop_cl.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // **关键**:macOS / BSD 上 `accept()` 返回的流会
                        // **继承**监听器的 O_NONBLOCK(Linux 不会)。于是
                        // 客户端还没把请求写过来,`read_to_string` 就先撞上
                        // EAGAIN("Resource temporarily unavailable")→
                        // 连接被丢弃 → 客户端的 write 收到 EPIPE。
                        //
                        // 早前这里靠调用方固定 `sleep(60ms)` 让请求先落在
                        // 缓冲区里才碰巧不炸 —— 那是竞态,不是同步。
                        // 正确做法:把接受的流恢复为阻塞。
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut buf = String::new();
                        if Read::read_to_string(&mut stream, &mut buf).is_err() {
                            continue;
                        }
                        let req: Value = match serde_json::from_str(buf.trim()) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let id = req.get("id").and_then(|v| v.as_u64()).unwrap_or(1);
                        let method = req
                            .get("method")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let params = req.get("params").cloned();
                        let (resp, fx) = router.dispatch(id, &method, params, &snap);
                        {
                            let mut r = rec_cl.lock().unwrap();
                            r.calls
                                .push((method, req.get("params").cloned().unwrap_or(Value::Null)));
                            if let Some(fx) = fx {
                                r.effects.push(fx);
                            }
                        }
                        let mut line = serde_json::to_string(&resp).unwrap_or_default();
                        line.push('\n');
                        let _ = Write::write_all(&mut stream, line.as_bytes());
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        (rec, stop, ready, handle)
    }

    /// 等假 GUI 的监听器绑定完成(确定性,不用固定 sleep)。
    fn wait_ready(ready: &Arc<(Mutex<bool>, std::sync::Condvar)>) {
        let (lock, cvar) = &**ready;
        let mut done = lock.lock().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !*done {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let (guard, _timeout) = cvar.wait_timeout(done, remaining).expect("condvar");
            done = guard;
        }
    }

    /// `capture-pane -p`(不带 `--lines`)默认 `lines=50`(DoD 要求)并
    /// 定位到解析出的窗格。
    #[test]
    fn capture_pane_round_trip_marshals_snapshot_with_lines_default() {
        let path = unique_socket_path("cap");
        let (rec, stop, ready, handle) = spawn_fake_gui(path.clone(), tmux_snap());
        wait_ready(&ready);
        let cmd = TmuxSub::CapturePane {
            target: Some("%0".into()),
            print: true,
            start: None,
            end: None,
            lines: None,
        };
        let res = run(cmd, &Some(path));
        stop.store(true, Ordering::Relaxed);
        let _ = handle.join();
        assert!(res.is_ok(), "capture-pane round-trip failed: {res:?}");

        let calls = rec.lock().unwrap().calls.clone();
        let snap_call = calls
            .iter()
            .find(|(m, _)| m == "surface.snapshot")
            .expect("surface.snapshot was called");
        assert_eq!(snap_call.1["lines"], 50, "-p defaults to 50 lines (DoD)");
        assert_eq!(snap_call.1["pane_id"], 10, "%0 → first pane id");
    }

    /// `send-keys ping` 以解析出的窗格和(map_key 转换后的)文本序列化出
    /// `surface.send_text`。
    #[test]
    fn send_keys_round_trip_marshals_target_and_text() {
        let path = unique_socket_path("sk");
        let (rec, stop, ready, handle) = spawn_fake_gui(path.clone(), tmux_snap());
        wait_ready(&ready);
        let cmd = TmuxSub::SendKeys { target: Some("%0".into()), keys: vec!["ping".into()] };
        let res = run(cmd, &Some(path));
        stop.store(true, Ordering::Relaxed);
        let _ = handle.join();
        assert!(res.is_ok(), "{res:?}");

        let calls = rec.lock().unwrap().calls.clone();
        let st = calls
            .iter()
            .find(|(m, _)| m == "surface.send_text")
            .expect("surface.send_text was called");
        assert_eq!(st.1["pane_id"], 10);
        assert_eq!(st.1["text"], "ping");
    }

    /// `join-pane -s %0 -t 1` 序列化出 `pane.attach`,路由器发出
    /// `AttachPane { src_pane_id: 10, dst_tab_index: 1, Vertical }`。
    #[test]
    fn join_pane_round_trip_emits_attach_pane_effect() {
        let path = unique_socket_path("jp");
        let (rec, stop, ready, handle) = spawn_fake_gui(path.clone(), tmux_snap());
        wait_ready(&ready);
        let cmd = TmuxSub::JoinPane {
            source: Some("%0".into()),
            target: Some("1".into()),
            horizontal: false,
            vertical: false,
        };
        let res = run(cmd, &Some(path));
        stop.store(true, Ordering::Relaxed);
        let _ = handle.join();
        assert!(res.is_ok(), "{res:?}");

        let guard = rec.lock().unwrap();
        let call = guard
            .calls
            .iter()
            .find(|(m, _)| m == "pane.attach")
            .expect("pane.attach was called");
        assert_eq!(call.1["src_pane_id"], 10);
        assert_eq!(call.1["dst_tab_index"], 1);
        let (src, dst, dir) = guard
            .effects
            .iter()
            .find_map(|fx| match fx {
                IpcSideEffect::AttachPane { src_pane_id, dst_tab_index, direction } =>
                    Some((*src_pane_id, *dst_tab_index, *direction)),
                _ => None,
            })
            .expect("AttachPane side effect emitted");
        assert_eq!(src, 10);
        assert_eq!(dst, 1);
        assert_eq!(dir, crate::split_tree::SplitDirection::Vertical);
    }
}
