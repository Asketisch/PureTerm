---
gap: GAP-25
title: 多窗口
category: UI/外壳
plan_type: feature
priority: P2
effort: L
status: done
closed: 2026-06-29
---

# Multi-Window（多窗口）

## 概述

支持创建多个独立窗口，每个窗口有自己的 Tab/Pane 布局。支持跨窗口通知路由、窗口间 drag-and-drop。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | `Cmd+Shift+N` 新建窗口，跨窗口通知路由 | — |
| **参考实现 A** | 完整多窗口支持 | — |
| **参考实现 B** | 窗口快照（自动保存/恢复多窗口布局） | `参考实现源码/crates/kaku/src/snapshots.rs` |

## PureTerm 现状

**未实现。** PureTerm 是单窗口应用，所有 Tab/Pane 在一个 Iced 窗口内。

## 优先级

**P2 — 终端增强**

多窗口在多显示器场景下有用，但 PureTerm 的 Tab + 分屏已能满足大部分需求。

## 工作量

**L（1.5-2 周）**

需要：Iced 多窗口支持（Iced 0.14 支持多窗口）、窗口间通信、状态同步。

## 实现方向

1. Iced 0.14 多窗口 API：`window::Id` 管理
2. 每个窗口独立的 Tab/Pane 状态
3. 共享状态：Config, Event Bus, Daemon 连接
4. `Cmd+Shift+N` 新建窗口
5. 跨窗口通知：Event Bus broadcast
6. 窗口间 drag-and-drop：Tab 拖到另一个窗口
