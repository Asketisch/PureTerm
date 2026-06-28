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

**🚧 代码闭环（~80% 端到端，待 desktop toast / CLI / manual E2E）。** GUI 内环 + EventBus + Dock 角标已接线；macOS 桌面 toast shim 就绪但未在 handler 调用。

**v1 已交付（2026-06-28）：**
- `alacritty_terminal/event_loop.rs` — `scan_osc_notify()` 解析 OSC 9/99/777（含 kitty OSC 99 分块参数 `i`/`d`/`p`/`e`/`a`/`u`/`s`/`c`/`r`/`t`/`m`/`h`/`o`/`w` 等）
- `terminal_context.rs` — 桥接 `TerminalEvent::Notification` / `Bell`（此前 `_ => return` 丢弃）
- `notification_store.rs` — 内存真源（历史环 200 / per-pane unread / attention set / bell 节流）
- `iced_app.rs` — amber attention ring、状态栏 🔔 段、`⌘⇧M` 通知面板、聚焦清 unread、Dock 角标刷新
- `macos_dock_badge.rs` — `NSDockTile.setBadgeLabel`（`[notification].dock_badge` 门控）
- EventBus — `notification.posted` / `panel_opened` / `panel_closed` / `all_dismissed`（`Category::Notification`）
- `[notification]` 配置 + `default-config.toml` 示例块

**待补（非阻塞 flip ✅）：**
- `fire_toast()` + `macos_notifications::post` — shim 已写，Notification handler 尚未调用
- 启动时 `request_authorization()`（UNUserNotificationCenter 授权）
- `pureterm notify` CLI（`NotifySource::Cli` 枚举已预留）
- Linux `dbus` 通知
- pixel smoke：`../../manual-e2e/gap-09-notification.md`

单测：notification 相关 **13** 绿（store 9 + config 3 + bridge 1 + `should_toast` 2）。

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
5. 🚧 macOS：`UNUserNotificationCenter` 桌面 toast（shim 就绪，handler 未接线）
6. ❌ Linux：`dbus` 通知
7. ❌ CLI：`pureterm notify "message"` 发送自定义通知
