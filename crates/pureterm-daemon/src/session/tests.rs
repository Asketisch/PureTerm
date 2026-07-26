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

/// GAP-5.1 M2/M5: `scrollback_frames` serializes the ring as `SCROLLBACK`
/// wire frames — what a freshly-attached subscriber receives to rebuild
/// scrollback. (Eviction itself is covered by `ring_buffer` tests.)
#[test]
fn scrollback_frames_encode_ring_as_scrollback() {
    let mut ring = RingBuffer::new(1024);
    ring.push(b"the quick brown fox");

    // Concatenate the payloads of every produced frame; assert all SCROLLBACK.
    let mut got = Vec::new();
    for wire in scrollback_frames(&ring) {
        let (frame, _consumed) = next_frame(&wire, 0).expect("one frame per entry");
        assert_eq!(frame.0, frame_type::SCROLLBACK);
        got.extend_from_slice(frame.1);
    }
    assert_eq!(got, b"the quick brown fox");
}

/// Minimal frame reader for tests: returns `((ftype, payload), consumed)`.
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
    // create_local spawns a real shell; skip shape-assertions if it fails
    // (e.g. minimal CI without a usable login shell).
    if let Ok(id) = mgr.create_local(80, 24, None, None, None, true) {
        let infos = mgr.list();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, id);
        assert_eq!(infos[0].kind, "local");
        assert!(mgr.kill(&id));
        assert!(mgr.list().is_empty());
    }
}

/// GAP-7.3: hibernate SIGSTOPs the session child (flag → true), resume
/// SIGCONTs it (flag → false), both idempotent, and the child stays alive
/// throughout (SIGSTOP/SIGCONT never close fds or kill the process).
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

    // hibernate → flag true, child still alive (stopped, not killed).
    assert!(mgr.hibernate(&id));
    assert!(mgr.list().iter().find(|i| i.id == id).unwrap().hibernated);
    assert!(pid_alive(child_pid), "child still alive after SIGSTOP");

    // resume → flag false, child still alive.
    assert!(mgr.resume(&id));
    assert!(!mgr.list().iter().find(|i| i.id == id).unwrap().hibernated);
    assert!(pid_alive(child_pid), "child still alive after SIGCONT");

    // idempotent: hibernate/resume on the same state don't fail.
    assert!(mgr.hibernate(&id));
    assert!(mgr.hibernate(&id));
    assert!(mgr.resume(&id));
    assert!(mgr.resume(&id));

    let _ = mgr.kill(&id);
}

/// GAP-5.1 M1 core proof: a session's child shell survives the GUI
/// (subscriber) disconnecting, because the master fd is owned by the pump
/// thread inside the `SessionHandle` — not by the subscriber. Dropping the
/// subscriber must NOT close the master, so the child keeps running and a
/// fresh subscriber can re-attach.
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

    // Sanity: child alive right after creation.
    assert!(pid_alive(child_pid), "child not alive after create");

    // Attach a subscriber (mimics the GUI's DaemonPty) and drain the banner.
    {
        let mut sub = UnixStream::connect(&data_path).expect("connect data socket");
        sub.set_read_timeout(Some(std::time::Duration::from_secs(1))).unwrap();
        let mut buf = [0u8; 8192];
        let _ = std::io::Read::read(&mut sub, &mut buf);
        // `sub` drops here → the subscriber connection closes (GUI exits).
    }

    // Let the pump notice the closed subscriber (poll wait ≤ 250ms).
    std::thread::sleep(std::time::Duration::from_millis(600));

    // CORE ASSERTION: the child shell is still alive after the subscriber
    // (GUI) dropped. If this fails, the master fd was closed on detach.
    assert!(
        pid_alive(child_pid),
        "child died after subscriber dropped — master fd closed prematurely"
    );

    // A fresh subscriber can re-attach to the still-live session.
    {
        let sub = UnixStream::connect(&data_path).expect("re-attach data socket");
        drop(sub);
    }

    assert!(mgr.kill(&id));
}

/// Authoritative liveness check via `ps` (kill(pid,0) can't distinguish a
/// live child from a not-yet-reaped zombie; `ps` reads kernel state).
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
