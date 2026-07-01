---
gap: GAP-33
title: Port Scanner
category: 开发者工具
plan_type: feature
priority: P2
effort: S
status: done
closed: 2026-06-29
---

# Port Scanner（端口扫描器）

## 概述

在侧边栏或 Status Bar 显示当前系统上所有监听端口及其对应的进程名/PID。帮助开发者快速查看哪些服务在运行。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | 侧边栏显示每个 workspace 的监听端口 | — |

## PureTerm 现状

**已实现（2026-06-29）。** 右侧边栏 **Ports** 标签页：`lsof -nP -iTCP -sTCP:LISTEN` 周期扫描，显示端口 / 进程 / PID；点击复制 `localhost:PORT`，↗ 打开浏览器。

## 优先级

**P2 — 终端增强**

## 工作量

**S（3-5 天）**

## 实现

- `crates/pureterm/src/port_scanner.rs` — lsof 解析 + iced subscription
- `crates/pureterm/src/sidebar/right.rs` — Ports 标签 UI
- `[port_scanner]` 配置：`enabled` / `interval` / `min_port`（默认 1024，过滤系统端口）
- 单测：`port_scanner::tests` 4 + `app_config::port_scanner_parses_fields`
