# Notification System（通知系统）

## 概述

解析终端通知协议（OSC 9/99/777），在 GUI 层面提供视觉提示（蓝色环覆盖、Dock 徽章、通知面板），让用户知道哪个 pane 需要关注。支持 `notify` CLI 命令发送自定义通知。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | OSC 9/99/777 解析，蓝色环覆盖，通知面板（`Cmd+I`），Dock 徽章，`cmux notify` CLI hook | `Sources/NotificationCenter/` |
| **iTerm2** | macOS 通知，trigger-based，bell，command completion | `sources/Notifications/` (iTermNotificationController, iTermNotificationCenter) |
| **Tabby** | 通过 xterm.js addon 支持基本通知 | — |

## PureTerm 现状

**未实现。** PureTerm 的 Event System（`crates/pureterm/src/feed_log.rs`）记录事件到 JSONL，但没有 OSC 通知解析和 GUI 视觉提示。

## 优先级

**P0 — AI 核心管道**

AI agent 多路复用场景下，用户需要知道哪个 pane 的 agent 完成了任务或需要输入。通知系统是多 agent 工作流的核心基础设施。

## 工作量

**M（1 周）**

OSC 解析在 alacritty_terminal 已有基础，主要工作在 GUI 层的通知展示和视觉提示。

## 实现方向

1. 在 alacritty_terminal 层解析 OSC 9/99/777 序列
2. 通过 Event Bus 通知 GUI 层
3. GUI 层：pane 边框蓝色环覆盖（需修改 `terminal_shader.rs`）
4. 通知面板：侧边栏显示通知历史
5. macOS：`NSUserNotification` / `UNUserNotificationCenter`
6. Linux：`dbus` 通知
7. CLI：`pureterm notify "message"` 发送自定义通知
