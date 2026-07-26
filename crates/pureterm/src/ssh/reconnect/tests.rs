//! ssh/reconnect 模块单元测试 —— 从原 reconnect.rs 内联测试迁出。

use super::*;


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
        // BUG-3: a worker that has failed its final attempt must give up at
        // once — neither sleep out the last backoff slot nor emit a misleading
        // "retry in Ns" Progress. `final_attempt()` is the 1-based index the
        // worker breaks on, so it must stay equal to `BACKOFF.len()` (a
        // schedule edit can't silently resurrect the wasted final sleep).
        assert_eq!(final_attempt(), BACKOFF.len() as u8);
        assert_eq!(final_attempt(), 8);
        // Every earlier attempt still retries; only the final one gives up.
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
            // Sleep 60s, but expect cancel to wake us fast.
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
        // Should not panic.
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
        // Verify the tx→rx plumbing without an async runtime: claim the rx,
        // send via tx, then poll with `try_recv` (sync).
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
        // GAP-5.6: Spawned replaces Success — ssh forked but not yet connected.
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
        // Nothing deposited; cancel drains completed; take_success is None.
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
        // GAP-5.6: slugs are a wire contract for events / IPC — lock them.
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

    // --- GAP-5.6 follow-up: anti-hammer (flap) detector ----------------------

    #[test]
    fn flap_constants_are_sane() {
        // Window must be long enough to catch a flapper but the cap low enough
        // to stop it quickly; cap must be high enough to spare a recovering
        // host from false positives.
        assert!(FLAP_WINDOW >= Duration::from_secs(30), "window too short: {:?}", FLAP_WINDOW);
        assert!(MAX_FLAPS >= 3, "cap too low (false positives): {}", MAX_FLAPS);
        assert!(MAX_FLAPS <= 12, "cap too high (flapper hammers too long): {}", MAX_FLAPS);
    }

    #[test]
    fn flap_first_n_triggers_spawn() {
        // The first MAX_FLAPS triggers within the window all spawn.
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
        // MAX_FLAPS triggers spawn; the next trigger, still inside the window,
        // is refused (Flapping) instead of starting another min-backoff cycle.
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
        // A host that drops occasionally must NOT accumulate toward the cap:
        // triggers older than FLAP_WINDOW age out, so a later drop starts fresh.
        let hub = ReconnectHub::new();
        let t0 = Instant::now();
        let mut t = t0;
        for _ in 0..MAX_FLAPS {
            assert_eq!(hub.record_reconnect_trigger(PaneId(1), t), SpawnDecision::Spawned);
            t += Duration::from_secs(10);
        }
        // Advance well past the window so every prior trigger is stale.
        let later = t0 + FLAP_WINDOW + Duration::from_secs(1);
        assert_eq!(
            hub.record_reconnect_trigger(PaneId(1), later),
            SpawnDecision::Spawned,
            "after the window, a fresh trigger must not flap",
        );
    }

    #[test]
    fn flap_cancel_clears_history() {
        // A user giving up (close / disconnect) must forget the flap history so
        // a later reconnect of the same pane id is not unfairly throttled.
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
        // One pane flapping must not poison a different pane's reconnect.
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
