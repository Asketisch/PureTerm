//! session 模块单测:订阅者最小尺寸、会话 id 唯一性、scrollback 帧编码、
//! manager create/list/kill 闭环、hibernate/resume 往返、关 GUI 不丢会话。

use std::collections::VecDeque;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use super::io_helpers::{min_subscriber_size, new_session_id};
use super::subscribers::{scrollback_frames, Subscriber};
use super::{SessionManager};
use crate::frame::frame_type;
use crate::ring_buffer::RingBuffer;

#[test]
fn min_subscriber_size_picks_smallest() {
    let subs = vec![
        Subscriber {
            stream: UnixStream::pair().unwrap().0,
            queue: VecDeque::new(),
            off: 0,
            cols: 120,
            rows: 40,
        },
        Subscriber {
            stream: UnixStream::pair().unwrap().0,
            queue: VecDeque::new(),
            off: 0,
            cols: 80,
            rows: 24,
        },
    ];
    assert_eq!(min_subscriber_size(&subs), (80, 24));
}

#[test]
fn session_ids_are_unique() {
    let a = new_session_id();
    let b = new_session_id();
    assert_ne!(a, b);
}

/// GAP-5.1 M2/M5:`scrollback_frames` 把环序列化为 `SCROLLBACK`
/// 线上帧——新挂接的订阅者收到的就是这些,用以重建
/// 回滚。(逐出本身由 `ring_buffer` 测试覆盖。)
#[test]
fn scrollback_frames_encode_ring_as_scrollback() {
    let mut ring = RingBuffer::new(1024);
    ring.push(b"the quick brown fox");

    // 拼接每个产出帧的载荷;断言全部是 SCROLLBACK。
    let mut got = Vec::new();
    for wire in scrollback_frames(&ring) {
        let (frame, _consumed) = next_frame(&wire, 0).expect("one frame per entry");
        assert_eq!(frame.0, frame_type::SCROLLBACK);
        got.extend_from_slice(frame.1);
    }
    assert_eq!(got, b"the quick brown fox");
}

/// 供测试用的极简帧读取器:返回 `((ftype, payload), consumed)`。
fn next_frame(data: &[u8], pos: usize) -> Option<((u8, &[u8]), usize)> {
    if pos + 4 > data.len() {
        return None;
    }
    let ln = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
    if pos + 4 + ln > data.len() {
        return None;
    }
    Some(((data[pos + 4], &data[pos + 5..pos + 4 + ln]), 4 + ln))
}

#[test]
fn manager_list_reflects_create_and_kill() {
    let _iso = crate::discovery::IsolatedDataDir::new();
    let mgr = SessionManager::default();
    assert!(mgr.list().is_empty());
    // create_local 会拉起真实 shell;若失败(如极简 CI 缺少可用的
    // 登录 shell)则跳过形状断言。
    if let Ok(id) = mgr.create_local(80, 24, None, None, None, true) {
        let infos = mgr.list();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, id);
        assert_eq!(infos[0].kind, "local");
        assert!(mgr.kill(&id));
        assert!(mgr.list().is_empty());
    }
}

/// GAP-7.3:hibernate 对会话子进程 SIGSTOP(标志 → true),resume
/// 对其 SIGCONT(标志 → false),两者幂等,且子进程全程
/// 存活(SIGSTOP/SIGCONT 绝不关闭 fd 或杀死进程)。
#[test]
fn hibernate_then_resume_round_trip() {
    let _iso = crate::discovery::IsolatedDataDir::new();
    let mgr = SessionManager::default();
    let id = match mgr.create_local(80, 24, None, None, None, true) {
        Ok(id) => id,
        Err(_) => return, // no usable login shell in this environment
    };
    let child_pid = mgr.list().into_iter().find(|i| i.id == id).unwrap().child_pid;
    assert!(pid_alive(child_pid), "child alive before hibernate");

    // hibernate → 标志为 true,子进程仍存活(被停而非被杀)。
    assert!(mgr.hibernate(&id));
    assert!(mgr.list().iter().find(|i| i.id == id).unwrap().hibernated);
    assert!(pid_alive(child_pid), "child still alive after SIGSTOP");

    // resume → 标志为 false,子进程仍存活。
    assert!(mgr.resume(&id));
    assert!(!mgr.list().iter().find(|i| i.id == id).unwrap().hibernated);
    assert!(pid_alive(child_pid), "child still alive after SIGCONT");

    // 幂等:同一状态下 hibernate/resume 不失败。
    assert!(mgr.hibernate(&id));
    assert!(mgr.hibernate(&id));
    assert!(mgr.resume(&id));
    assert!(mgr.resume(&id));

    let _ = mgr.kill(&id);
}

/// GAP-5.1 M1 核心证明:会话的子 shell 在 GUI
///(订阅者)断开后依然存活,因为 master fd 由 `SessionHandle` 内的泵
/// 线程持有——而不是订阅者。丢弃
/// 订阅者绝不能关闭 master,子进程得以继续运行,
/// 新的订阅者可以重新挂接。
#[test]
fn local_session_child_survives_subscriber_drop() {
    let _iso = crate::discovery::IsolatedDataDir::new();
    let mgr = SessionManager::default();
    let id = match mgr.create_local(80, 24, None, None, None, true) {
        Ok(id) => id,
        Err(_) => return, // no usable login shell in this environment
    };
    let (child_pid, data_path) = {
        let info = mgr.list().into_iter().find(|i| i.id == id).unwrap();
        (info.child_pid, PathBuf::from(&info.data_path))
    };

    // 健全性:创建后子进程存活。
    assert!(pid_alive(child_pid), "child not alive after create");

    // 挂接一个订阅者(模拟 GUI 的 DaemonPty)并排空 banner。
    {
        let mut sub = UnixStream::connect(&data_path).expect("connect data socket");
        sub.set_read_timeout(Some(std::time::Duration::from_secs(1))).unwrap();
        let mut buf = [0u8; 8192];
        let _ = std::io::Read::read(&mut sub, &mut buf);
        // `sub` 在此丢弃 → 订阅连接关闭(GUI 退出)。
    }

    // 让泵注意到订阅者已关闭(poll 等待 ≤ 250ms)。
    std::thread::sleep(std::time::Duration::from_millis(600));

    // 核心断言:订阅者(GUI)丢弃后子 shell 仍存活。
    // 若失败,说明 master fd 在脱离时被关闭了。
    assert!(
        pid_alive(child_pid),
        "child died after subscriber dropped — master fd closed prematurely"
    );

    // 新的订阅者可以重新挂接到仍存活的会话。
    {
        let sub = UnixStream::connect(&data_path).expect("re-attach data socket");
        drop(sub);
    }

    assert!(mgr.kill(&id));
}

/// 经 `ps` 的权威存活检查(kill(pid,0) 无法区分存活子进程与
/// 尚未收殓的僵尸;`ps` 读内核状态)。
fn pid_alive(pid: u32) -> bool {
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .stdout(std::process::Stdio::piped())
        .output();
    match out {
        Ok(o) => {
            let stat = String::from_utf8_lossy(&o.stdout).trim().to_string();
            !stat.is_empty() && !stat.starts_with('Z')
        }
        Err(_) => false,
    }
}
