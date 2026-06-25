# Enhanced Status Bar（增强状态栏）

## 概述

在现有 Status Bar 基础上扩展组件：CPU/内存/网络利用率、Git 详细状态、电池、时钟、任务状态、搜索框、代码片段、进度条、sparklines。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | 40+ 状态栏组件：CPU/memory/network utilization, git state, battery, clock, job status, search field, snippets, sparklines, progress, composer, triggers | `sources/StatusBar/Components/` (iTermCPUUtilization, iTermMemoryUtilization, iTermNetworkUtilization, iTermGitState, iTermStatusBarBatteryComponent, iTermStatusBarClockComponent, iTermStatusBarJobComponent, iTermStatusBarSearchFieldComponent, iTermStatusBarSnippetComponent, iTermStatusBarSparklinesComponent, iTermStatusBarProgressComponent, iTermsStatusBarComposerComponent, iTermStatusBarTriggersComponent) |
| **Kaku** | 无明确状态栏 | — |

## PureTerm 现状

**基础实现。** PureTerm 的 Status Bar（`crates/pureterm/src/status_bar.rs`）显示 CWD、Git branch、SSH、进程、agent 状态、光标位置（6 个组件）。

## 优先级

**P2 — 终端增强**

Status Bar 是开发者实时信息的入口，iTerm2 的 40+ 组件是其核心优势。

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
