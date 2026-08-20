---
gap: GAP-13
title: 广播输入
category: 终端交互
plan_type: feature
priority: P3
effort: S
status: done
closed: 2026-06-29
---

# Broadcast Input（广播输入）

## 概述

将键盘输入同时发送到多个终端 session/pane。支持选择广播范围：当前 tab 所有 pane、所有 tab、指定 pane 组。支持密码广播（安全输入同步）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | Send keyboard to multiple sessions with password broadcast | `sources/InputBroadcasting/` (BroadcastInputHelper) |
| **参考实现 B** | 当前 tab / 所有 tab 广播 | — |

## PureTerm 现状

**未实现。** PureTerm 的 tmux 兼容层（`crates/pureterm/src/tmux_cli.rs`）支持 `send-keys` 到指定 pane，但没有 GUI 层的广播输入功能。

## 优先级

**P3 — 兼容性/国际化**

Broadcast Input 在多服务器管理场景下有用（如同时在多个服务器执行相同命令），但非核心功能。

## 工作量

**S（3-5 天）**

核心是键盘事件分发逻辑，不需要复杂的 UI。

## 实现方向

1. 配置：`Cmd+Shift+B` 切换广播模式
2. 广播范围选择 UI：下拉菜单选择（当前 tab / 所有 tab / 自定义组）
3. 键盘事件分发：将 keystroke 同时写入多个 PTY
4. 密码广播：加密传输（防止明文密码泄露到其他 pane）
5. 状态栏显示：广播模式激活时显示 "BROADCAST" 标识
