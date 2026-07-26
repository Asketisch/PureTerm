---
gap: GAP-1.3
title: 标题栈 (CSI 22/23t + OSC 0/2)
category: 终端交互
plan_type: feature
priority: P2
effort: S
status: done
closed: 2026-06-25
e2e: manual-e2e/gap-1.3-title-stack.md
---

# 标题栈 / OSC 动态标题

## 概述

支持 OSC 0/2 程序设标题与 CSI 22t/23t 压弹栈，反映到窗口标题与 Tab 标题；空标题回退 PureTerm 派生标题。

## 竞品参考

| 竞品 | 功能 |
|------|------|
| **参考实现 A** | 动态标题 + 标题栈 |
| **Alacritty** | OSC 0/2 + push/pop title |

## PureTerm 现状

**✅ 已闭环（2026-06-25）。** GUI E2E 通过。

- 核心栈复用上游 `alacritty_terminal`（`Term::{push,pop,set}_title`）
- 实现：`crates/pureterm/src/terminal_context.rs` + `iced_app.rs`（`pane_osc_title` / `resolve_display_title`）
- 单测：桥接转发 3 + `resolve_display_title` 3
- 手动 E2E：[`manual-e2e/gap-1.3-title-stack.md`](../../manual-e2e/gap-1.3-title-stack.md)
- 完整实现记录：[`FEATURES.md`](../../../FEATURES.md) §GAP-1.3

## 优先级

**P2 — 终端增强**（路线图归类；实现已提前交付）

## 工作量

**S（2-3 天）** — 已完成
