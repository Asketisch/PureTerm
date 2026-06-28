---
gap: GAP-2.1
title: Surface/Pane/Workspace/Window 抽象
category: 核心架构
plan_type: arch
priority: P0
effort: XL
status: done
closed: 2026-06-25
e2e: manual-e2e/gap-2.1-workspace.md
plan: plan.md
---

# Surface / Pane / Workspace / Window 抽象层

## 概述

引入 Window → Workspace → Pane → Surface 四层头等抽象，使命名工作区成为一等公民；切换 Workspace 不丢 PTY/Web 状态。

## 竞品参考

| 竞品 | 功能 |
|------|------|
| **cmux** | Workspace 命名分组 + 切换 |
| **iTerm2** | Window / Tab / Session 分层 |

## PureTerm 现状

**✅ 已闭环（2026-06-25）。** 4 层模型 + 快捷键切换 + `workspaces.json` v2 持久化 + detached 键盘路由。

- 实现：`crates/pureterm/src/iced_app.rs` + `session_restore.rs`
- 单测：430 pureterm 单测绿（`workspace_e2e` / `keyboard_routing_e2e` / v1→v2 迁移）
- 详细实施计划见 [plan.md](plan.md)
- 手动 E2E：[`manual-e2e/gap-2.1-workspace.md`](../../manual-e2e/gap-2.1-workspace.md)
- 完整实现记录：[`FEATURES.md`](../../../FEATURES.md) §GAP-2.1

## 未做（独立后续）

- 侧边栏 Workspace 切换器（依赖 GAP-19 Dual Sidebar）
- CLI `workspace` 命令组（Phase 5，可独立排期）
