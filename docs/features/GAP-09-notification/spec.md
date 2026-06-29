---
gap: GAP-09
title: Notification System
category: 终端交互
plan_type: wrap-up
priority: P0
effort: M
status: done
closed: 2026-06-28
e2e: manual-e2e/gap-09-notification.md
---

# Notification System（通知系统）

## 概述

解析终端通知协议（OSC 9/99/777），在 GUI 层面提供视觉提示（attention ring、Dock 徽章、通知面板），让用户知道哪个 pane 需要关注。支持 `notify` CLI 命令发送自定义通知。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | OSC 9/99/777 解析，蓝色环覆盖，通知面板（`Cmd+I`），Dock 徽章，`cmux notify` CLI hook | `Sources/NotificationCenter/` |
| **iTerm2** | macOS 通知，trigger-based，bell，command completion | `sources/Notifications/` (iTermNotificationController, iTermNotificationCenter) |
| **Tabby** | 通过 xterm.js addon 支持基本通知 | — |

## PureTerm 现状

**✅ 代码闭环（macOS-only v1，全链路：OSC 9/99/777 + Bell → ring / 通知面板 / Dock 角标 / 桌面 toast / `pureterm notify` CLI / `events` 审计）。** GUI 内环 + EventBus + Dock 角标 + 桌面 toast + CLI 全部接线。

**Review ✅（2026-06-29）：** 审计修 3 处 ——
- BUG-1（HIGH）：`NotificationStore::post()` 的 `bool` 返回值在 OSC / Bell / CLI 三个 handler 被丢弃，导致**节流铃铛仍弹桌面 toast + 写 `notification.posted` 审计**（`while true; printf '\a'` 每铃一 toast）。已用 `let posted = …; if posted { fire_toast; events.emit }` 门控。
- BUG-2（LOW）：`scan_osc_notify` 文档注释「re-scanned on the next read」错误（跨两次 PTY 读的通知实际被丢弃，扫描器无跨读状态）—— 订正注释 + 加 `split_across_reads_is_dropped` pin 测试。
- BUG-3（LOW）：`EventBus::emit` 的 `log_lock.lock().unwrap()` 改为 `unwrap_or_else(|e| e.into_inner())`，避免 Mutex 中毒在主线程 panic。

**v1 已交付（2026-06-28）：**
- `alacritty_terminal/event_loop.rs` — `scan_osc_notify()` 解析 OSC 9/99/777（含 kitty OSC 99 分块参数 `i`/`d`/`p`/`e`/`a`/`u`/`s`/`c`/`r`/`t`/`m`/`h`/`o`/`w` 等）
- `terminal_context.rs` — 桥接 `TerminalEvent::Notification` / `Bell`（此前 `_ => return` 丢弃）
- `notification_store.rs` — 内存真源（历史环 200 / per-pane unread / attention set / bell 节流）
- `iced_app.rs` — amber attention ring、状态栏 🔔 段、`⌘⇧M` 通知面板、聚焦清 unread、Dock 角标刷新
- `macos_dock_badge.rs` — `NSDockTile.setBadgeLabel`（`[notification].dock_badge` 门控）
- EventBus — `notification.posted` / `panel_opened` / `panel_closed` / `all_dismissed`（`Category::Notification`）
- `[notification]` 配置 + `default-config.toml` 示例块

**v1 后续补齐（已闭环）：**
- ✅ `fire_toast()` + `macos_notifications::post` — 已在 OSC / Bell / CLI 三个 handler 调用（`iced_app.rs`）
- ✅ 启动时 `request_authorization()`（`UNUserNotificationCenter` 授权，`iced_app.rs` 启动 + 配置热重载调用）
- ✅ `pureterm notify "msg"` CLI（`cli.rs` → `notification.post` RPC → `Fx::ShowNotification`，`NotifySource::Cli`）

**未做（非阻塞）：**
- Linux `dbus` 通知（v1 macOS-only，有意为之）
- pixel smoke：`../../manual-e2e/gap-09-notification.md`（手动 GUI E2E，非 gate）
- 已知低危限制：跨两次 PTY 读的通知会丢失（扫描器无跨读状态，与 `scan_osc7`/`scan_osc133` 同源；见 BUG-2 订正的注释 + `split_across_reads_is_dropped` 测试）

单测：scanner **18**（OSC 9/99/777 + 跨读 pin，`alacritty_terminal`）+ store **9** + `should_toast` **2**（`pureterm`）。

## 优先级

**P0 — AI 核心管道**

AI agent 多路复用场景下，用户需要知道哪个 pane 的 agent 完成了任务或需要输入。通知系统是多 agent 工作流的核心基础设施。

## 工作量

**M（1 周）**

OSC 解析在 alacritty_terminal 已有基础，主要工作在 GUI 层的通知展示和视觉提示。

## 实现方向

1. ✅ 在 alacritty_terminal 层解析 OSC 9/99/777 序列
2. ✅ 通过 Event Bus 通知 GUI 层（`notification.posted` 等）
3. ✅ GUI 层：pane 边框 amber attention ring（`view_pane` border）
4. ✅ 通知面板：`⌘⇧M`  overlay 显示历史
5. ✅ macOS：`UNUserNotificationCenter` 桌面 toast（`fire_toast` 已在 handler 调用 + 启动 `request_authorization`）
6. ❌ Linux：`dbus` 通知（v1 macOS-only）
7. ✅ CLI：`pureterm notify "message"` 发送自定义通知（`notification.post` RPC）
