//! ssh/reconnect 模块单元测试 —— 从原 reconnect.rs 内联测试迁出。

use super::*;
use std::thread;

    #[test]
    fn backoff_is_nondecreasing_and_capped() {
        for w in BACKOFF.windows(2) {
            assert!(w[0] <= w[1], "backoff must be non-decreasing: {:?}", w);
        }
        assert_eq!(*BACKOFF.last().unwrap(), Duration::from_secs(60));
        assert!(BACKOFF.len() >= 6, "should give a reasonable number of retries");
    }

    #[test]
    fn final_attempt_is_last_backoff_index_and_gives_up_immediately() {
        // BUG-3:最后一次尝试失败的 worker 必须立刻放弃 ——
        // 既不睡满最后的退避槽,也不发出误导性的
        // "retry in Ns" 进度。`final_attempt()` 是从 1 开始计的索引,
        // worker 依此中断,因此它必须始终等于 `BACKOFF.len()`(
        // 修改调度表不能悄悄复活被浪费的最后一次睡眠)。
        assert_eq!(final_attempt(), BACKOFF.len() as u8);
        assert_eq!(final_attempt(), 8);
        // 更早的每次尝试仍会重试;只有最后一次放弃。
        for a in 1..final_attempt() {
            assert_ne!(a, final_attempt(), "attempt {a} must not be the give-up boundary");
        }
    }

    #[test]
    fn snapshot_roundtrip_preserves_attempt() {
        let cases: Vec<SshState> = vec![
            SshState::Disconnected,
            SshState::Connecting,
            SshState::Authenticating,
            SshState::Connected,
            SshState::Reconnecting { attempt: 3, next_retry: Instant::now() },
            SshState::Failed { kind: SshFailureKind::Auth, reason: "x".into() },
        ];
        let snaps = vec![
            SshStateSnapshot::Disconnected,
            SshStateSnapshot::Connecting,
            SshStateSnapshot::Authenticating,
            SshStateSnapshot::Connected,
            SshStateSnapshot::Reconnecting { attempt: 3 },
            SshStateSnapshot::Failed { kind: SshFailureKind::Auth },
        ];
        for (state, snap) in cases.iter().zip(snaps.iter()) {
            assert_eq!(SshStateSnapshot::from(state), *snap);
        }
    }

    #[test]
    fn is_terminal_for_reconnect_classification() {
        assert!(SshState::Disconnected.is_terminal_for_reconnect());
        assert!(SshState::Failed { kind: SshFailureKind::Unknown, reason: "x".into() }.is_terminal_for_reconnect());
        assert!(!SshState::Connected.is_terminal_for_reconnect());
        assert!(!SshState::Connecting.is_terminal_for_reconnect());
        assert!(!SshState::Authenticating.is_terminal_for_reconnect());
        assert!(!SshState::Reconnecting { attempt: 1, next_retry: Instant::now() }.is_terminal_for_reconnect());
    }

    #[test]
    fn first_byte_flips_authenticating_with_separator() {
        // 重连:Authenticating → Connected,且盖一条重连分隔线。
        assert_eq!(
            on_first_remote_byte(SshStateSnapshot::Authenticating),
            (true, true)
        );
    }

    #[test]
    fn first_byte_flips_connecting_without_separator() {
        // 初始连接:Connecting → Connected,但**不**盖分隔线(分隔线仅重连注入)。
        assert_eq!(
            on_first_remote_byte(SshStateSnapshot::Connecting),
            (true, false)
        );
    }

    #[test]
    fn first_byte_noop_on_connected_or_terminal() {
        // 已连/终态:首字节到达不应再触发翻转,也不盖分隔线。
        assert_eq!(
            on_first_remote_byte(SshStateSnapshot::Connected),
            (false, false)
        );
        assert_eq!(
            on_first_remote_byte(SshStateSnapshot::Disconnected),
            (false, false)
        );
        assert_eq!(
            on_first_remote_byte(SshStateSnapshot::Reconnecting { attempt: 1 }),
            (false, false)
        );
        assert_eq!(
            on_first_remote_byte(SshStateSnapshot::Failed { kind: SshFailureKind::Auth }),
            (false, false)
        );
    }

    #[test]
    fn cancel_signal_wakes_sleep_immediately() {
        let signal = CancelSignal::new();
        let s = signal.clone();
        let handle = thread::spawn(move || {
            // 睡眠 60 秒,但预期取消能很快唤醒。
            s.sleep_or_cancel(Duration::from_secs(60))
        });
        thread::sleep(Duration::from_millis(50));
        let start = Instant::now();
        signal.cancel();
        let cancelled = handle.join().unwrap();
        let elapsed = start.elapsed();
        assert!(cancelled, "should report cancelled after wake");
        assert!(elapsed < Duration::from_secs(2), "cancel took too long: {:?}", elapsed);
    }

    #[test]
    fn cancel_signal_sleep_completes_when_not_cancelled() {
        let signal = CancelSignal::new();
        let start = Instant::now();
        let cancelled = signal.sleep_or_cancel(Duration::from_millis(80));
        let elapsed = start.elapsed();
        assert!(!cancelled);
        assert!(elapsed >= Duration::from_millis(70), "slept only {:?}", elapsed);
    }

    #[test]
    fn hub_cancel_for_unknown_pane_is_noop() {
        let hub = ReconnectHub::new();
        // 不应 panic。
        hub.cancel(PaneId(9999));
    }

    #[test]
    fn hub_take_updates_receiver_yields_once() {
        let hub = ReconnectHub::new();
        assert!(hub.take_updates_receiver().is_some(), "first take should yield the receiver");
        assert!(hub.take_updates_receiver().is_none(), "second take should be None");
    }

    #[test]
    fn hub_updates_tx_delivers_to_receiver() {
        // 在没有异步运行时的情况下验证 tx→rx 管道:先认领 rx,
        // 再经 tx 发送,然后用 `try_recv`(同步)轮询。
        let hub = ReconnectHub::new();
        let mut rx = hub.take_updates_receiver().expect("receiver present");
        let _ = hub.updates_tx.send(ReconnectUpdate::Failed {
            pane_id: PaneId(1),
            kind: SshFailureKind::Auth,
            reason: "test".into(),
        });
        match rx.try_recv() {
            Ok(ReconnectUpdate::Failed { pane_id, kind, reason }) => {
                assert_eq!(pane_id, PaneId(1));
                assert_eq!(kind, SshFailureKind::Auth);
                assert_eq!(reason, "test");
            }
            other => panic!("expected Ok(Failed), got {:?}", other.map(|_| "ok")),
        }
    }

    #[test]
    fn reconnect_update_spawned_travels_channel() {
        // GAP-5.6:Spawned 取代 Success —— ssh 已 fork 但尚未连接。
        let hub = ReconnectHub::new();
        let mut rx = hub.take_updates_receiver().expect("receiver present");
        let _ = hub.updates_tx.send(ReconnectUpdate::Spawned { pane_id: PaneId(7) });
        match rx.try_recv() {
            Ok(ReconnectUpdate::Spawned { pane_id }) => assert_eq!(pane_id, PaneId(7)),
            other => panic!("expected Ok(Spawned), got {:?}", other.map(|_| "ok")),
        }
    }

    #[test]
    fn take_success_returns_none_after_cancel_drain() {
        let hub = ReconnectHub::new();
        // 未存入任何内容;cancel 排空已完成项;take_success 为 None。
        hub.cancel(PaneId(5));
        assert!(hub.take_success(PaneId(5)).is_none());
    }

    #[test]
    fn reconnect_params_connection_key_defaults() {
        let p = ReconnectParams {
            host: "h".into(),
            user: None,
            port: None,
            key_path: None,
            password: None,
            forward_agent: false,
            forwards: SshForwards::default(),
            jump_hosts: Vec::new(),
        };
        let k = p.connection_key();
        assert_eq!(k.host, "h");
        assert_eq!(k.port, 22);
        assert_eq!(k.user, "");
    }

    // --- GAP-5.6 收尾:stderr 失败分类器 -------------------------------------

    #[test]
    fn classify_ssh_failure_recognizes_auth() {
        assert_eq!(classify_ssh_failure("Permission denied (publickey)."), SshFailureKind::Auth);
        assert_eq!(
            classify_ssh_failure("admin@host: Permission denied (publickey,password)."),
            SshFailureKind::Auth
        );
    }

    #[test]
    fn classify_ssh_failure_recognizes_network() {
        assert_eq!(
            classify_ssh_failure("ssh: connect to host 10.0.0.1 port 22: Connection refused"),
            SshFailureKind::Network
        );
        assert_eq!(
            classify_ssh_failure("ssh: connect to host 10.0.0.1 port 22: Connection timed out"),
            SshFailureKind::Network
        );
        assert_eq!(
            classify_ssh_failure("ssh: Could not resolve hostname foo: nodename nor servname"),
            SshFailureKind::Network
        );
        assert_eq!(
            classify_ssh_failure("ssh: connect to host 10.0.0.1 port 22: Network is unreachable"),
            SshFailureKind::Network
        );
    }

    #[test]
    fn classify_ssh_failure_recognizes_host_key() {
        assert_eq!(classify_ssh_failure("Host key verification failed."), SshFailureKind::HostKey);
        assert_eq!(
            classify_ssh_failure("@@    REMOTE HOST IDENTIFICATION HAS CHANGED    @@"),
            SshFailureKind::HostKey
        );
    }

    #[test]
    fn classify_ssh_failure_unknown_for_unrelated() {
        assert_eq!(classify_ssh_failure(""), SshFailureKind::Unknown);
        assert_eq!(classify_ssh_failure("something else entirely"), SshFailureKind::Unknown);
    }

    #[test]
    fn failure_kind_and_snapshot_slugs_are_stable() {
        // GAP-5.6:slug 是事件 / IPC 的线上契约 —— 锁定它们。
        assert_eq!(SshFailureKind::Auth.as_str(), "auth");
        assert_eq!(SshFailureKind::Network.as_str(), "network");
        assert_eq!(SshFailureKind::HostKey.as_str(), "host_key");
        assert_eq!(SshFailureKind::Unknown.as_str(), "unknown");
        assert_eq!(SshStateSnapshot::Authenticating.as_str(), "authenticating");
        assert_eq!(
            SshStateSnapshot::Failed { kind: SshFailureKind::Auth }.as_str(),
            "failed"
        );
        assert_eq!(
            SshStateSnapshot::Reconnecting { attempt: 2 }.attempt(),
            Some(2)
        );
        assert_eq!(SshStateSnapshot::Connected.attempt(), None);
    }

    // ── GAP-5.6 跟进:防连击(flap)检测 ───────────────────────────────────────────────────

    #[test]
    fn flap_constants_are_sane() {
        // 窗口要足够长以捕捉连击者,上限要足够低
        // 以快速叫停;上限又要足够高,避免误伤
        // 正在恢复的主机。
        assert!(FLAP_WINDOW >= Duration::from_secs(30), "window too short: {:?}", FLAP_WINDOW);
        assert!(MAX_FLAPS >= 3, "cap too low (false positives): {}", MAX_FLAPS);
        assert!(MAX_FLAPS <= 12, "cap too high (flapper hammers too long): {}", MAX_FLAPS);
    }

    #[test]
    fn flap_first_n_triggers_spawn() {
        // 窗口内前 MAX_FLAPS 次触发全部放行。
        let hub = ReconnectHub::new();
        let mut t = Instant::now();
        for i in 0..MAX_FLAPS {
            assert_eq!(
                hub.record_reconnect_trigger(PaneId(1), t),
                SpawnDecision::Spawned,
                "trigger {} should spawn",
                i,
            );
            t += Duration::from_secs(10);
        }
    }

    #[test]
    fn flap_exceeding_max_within_window_refuses() {
        // MAX_FLAPS 次触发已放行;下一次触发(仍在窗口内)
        // 被拒绝(Flapping),而不是再开启一轮最小退避。
        let hub = ReconnectHub::new();
        let mut t = Instant::now();
        for _ in 0..MAX_FLAPS {
            assert_eq!(hub.record_reconnect_trigger(PaneId(1), t), SpawnDecision::Spawned);
            t += Duration::from_secs(10);
        }
        assert_eq!(
            hub.record_reconnect_trigger(PaneId(1), t),
            SpawnDecision::Flapping,
            "trigger over the cap within the window must flap",
        );
    }

    #[test]
    fn flap_window_self_decays_for_healthy_host() {
        // 偶尔掉线的主机绝不能向上限累计:
        // 超过 FLAP_WINDOW 的触发会过期,后来的掉线从零开始。
        let hub = ReconnectHub::new();
        let t0 = Instant::now();
        let mut t = t0;
        for _ in 0..MAX_FLAPS {
            assert_eq!(hub.record_reconnect_trigger(PaneId(1), t), SpawnDecision::Spawned);
            t += Duration::from_secs(10);
        }
        // 推进到远超窗口,使此前的所有触发都过期。
        let later = t0 + FLAP_WINDOW + Duration::from_secs(1);
        assert_eq!(
            hub.record_reconnect_trigger(PaneId(1), later),
            SpawnDecision::Spawned,
            "after the window, a fresh trigger must not flap",
        );
    }

    #[test]
    fn flap_cancel_clears_history() {
        // 用户主动放弃(关闭 / 断开)时必须遗忘连击历史,
        // 以免同一窗格后续重连被不公平地限流。
        let hub = ReconnectHub::new();
        let mut t = Instant::now();
        for _ in 0..MAX_FLAPS {
            assert_eq!(hub.record_reconnect_trigger(PaneId(1), t), SpawnDecision::Spawned);
            t += Duration::from_secs(10);
        }
        hub.cancel(PaneId(1));
        assert_eq!(
            hub.record_reconnect_trigger(PaneId(1), t),
            SpawnDecision::Spawned,
            "after cancel, the same pane must reconnect fresh",
        );
    }

    #[test]
    fn flap_counter_is_per_pane() {
        // 一个窗格连击绝不能污染另一窗格的重连。
        let hub = ReconnectHub::new();
        let mut t = Instant::now();
        for _ in 0..MAX_FLAPS {
            assert_eq!(hub.record_reconnect_trigger(PaneId(1), t), SpawnDecision::Spawned);
            t += Duration::from_secs(10);
        }
        assert_eq!(
            hub.record_reconnect_trigger(PaneId(2), t),
            SpawnDecision::Spawned,
            "a different pane must be independent of pane 1's flap history",
        );
    }
