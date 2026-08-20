---
gap: GAP-24
title: 增强状态栏
category: UI/外壳
plan_type: enhance
priority: P2
effort: L
status: done
closed: 2026-06-26
e2e: manual-e2e/gap-32-git-integration.md
---

# Enhanced Status Bar（增强状态栏）

## 概述

在现有 Status Bar 基础上扩展组件：CPU/内存/网络利用率、Git 详细状态、电池、时钟、任务状态、搜索框、代码片段、进度条、sparklines。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | 40+ 状态栏组件：CPU/memory/network utilization, git state, battery, clock, job status, search field, snippets, sparklines, progress, composer, triggers | `sources/StatusBar/Components/` (CpuUtilization, MemoryUtilization, NetworkUtilization, GitState, StatusBarBatteryComponent, StatusBarClockComponent, StatusBarJobComponent, StatusBarSearchFieldComponent, StatusBarSnippetComponent, StatusBarSparklinesComponent, StatusBarProgressComponent, StatusBarComposerComponent, StatusBarTriggersComponent) |
| **参考实现 B** | 无明确状态栏 | — |

## PureTerm 现状

**✅ 已闭环（2026-06-26）。** trait 架构 + CPU/Memory/Network/Battery/Clock + sparkline + porcelain git 增强；495+ pureterm 单测绿。pixel smoke 见 [`../../manual-e2e/`](../../manual-e2e/)（非 CI gate）。

**架构（GAP-24，2026-06-26）：**
- `StatusBarComponent` trait + `Vec<Box<dyn StatusBarComponent>>` 注册表；`view_bottom_bar` 只组装 `BarData` 快照
- 新增段：CPU / Memory / Network（sparkline）/ Clock / Battery；原有 Cwd/Git/Ssh/ViMode/Process/Agent/Cursor 全部移植
- Git 增强：`git status --porcelain=v2 --branch` 一条子进程（branch + dirty + ahead/behind）；见 GAP-32 进一步扩展
- 配置：`[statusbar]`（`components` 顺序 / `refresh_interval_secs` / `show_sparklines` / `bar_height`）
- 实现：`crates/pureterm/src/status_bar/` + `app_config.rs`

**GAP-09 扩展（2026-06-28）：** 状态栏新增 `Notifications` 段（🔔 未读计数，`[notification].enabled` 门控）。

**defer 后续：** 搜索框 / snippet / composer / triggers progress 计数（业界终端有、PureTerm MVP 未做）。

## 优先级

**P2 — 终端增强**

Status Bar 是开发者实时信息的入口，业界终端的 40+ 组件是其核心优势。

## 工作量

**L（1.5-2 周）**

需要：系统信息采集、可扩展组件架构、性能优化（避免频繁刷新）。

## 实现方向

1. 组件化架构：`StatusBarComponent` trait
2. 新增组件：CPU utilization（`sysinfo` crate）、Memory、Network、Battery、Clock
3. Git 增强：分支 + dirty 状态 + 当前 commit
4. 搜索框：Status Bar 内嵌搜索输入
5. Sparklines：ASCII 迷你图（CPU 历史等）
6. 配置：`statusbar.components = ["cpu", "memory", "git", "clock"]`
7. 刷新频率：可配置（默认 1s）
