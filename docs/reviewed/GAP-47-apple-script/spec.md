---
gap: GAP-47
title: AppleScript 桥
category: 平台
plan_type: platform
priority: P2
effort: M
status: done
closed: 2026-06-29
---

# AppleScript Bridge（AppleScript 桥）

## 概述

macOS 上通过 AppleScript 实现外部自动化——其他应用（如 Alfred、SketchyBar）可通过 `osascript` 控制 PureTerm：创建 tab、发送命令、获取状态。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | 只读 AppleScript：get name/version/frontmost, quit | `参考实现源码/crates/kaku/src/applescript.rs` |
| **cmux** | AppleScriptBridge（完整读写） | `Sources/AppleScriptBridge.swift` |

## PureTerm 现状

**未实现。**

## 优先级

**P2 — 终端增强**

AppleScript 是 macOS 自动化的标准方式，对 macOS 用户有实用价值。

## 工作量

**M（1 周）**

需要：`objc2` 注册 AppleScript 事件处理器。

## 实现方向

1. macOS：通过 `objc2` 注册 `NSAppleEventManager` 处理器
2. 支持的命令：
   - `tell application "PureTerm" to create ssh tab` → 创建 SSH tab
   - `tell application "PureTerm" to send text "ls"` → 发送命令
   - `tell application "PureTerm" to get current directory` → 获取 CWD
3. 跨平台替代：Linux 用 D-Bus（`zbus` crate），Windows 用 COM
4. 配置：`applescript.enabled = true`
