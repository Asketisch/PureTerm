---
gap: GAP-10
title: Triggers
category: 终端交互
plan_type: feature
priority: P1
effort: L
status: in-progress
closed: null
plan: ../../superpowers/plans/2026-05-15-triggers-system.md
---

# Triggers（模式匹配自动化）

## 概述

定义规则匹配终端输出中的模式（正则表达式），自动执行动作：高亮文本、添加注释、发送通知、执行脚本、折叠区域等。67+ 种触发器类型。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | 67+ 种 trigger 类型：HighlightTrigger, AnnotateTrigger, MarkTrigger, NotificationTrigger, ScriptTrigger, iTermRPCTigger, iTermHyperlinkTrigger, BellTrigger, BounceTrigger, SendTextTrigger, BrowserTrigger, FoldTrigger, SGRTrigger, SetTabStatusTrigger, SetUserVariableTrigger 等 | `sources/Triggers/` |
| **cmux** | 无明确 Triggers 系统 | — |

## PureTerm 现状

**🚧 库级（核心引擎 + 配置解析 + Message stub；Wakeup 扫描 / highlight 渲染未接）。** 用户尚不可通过 `[[triggers]]` 配置获得端到端行为。

**v0 已交付（2026-06-28）：**
- `triggers.rs` — `scan_lines()` / `next_scan_window()` / `TriggerPaneState` / `HighlightRegion` / `spawn_trigger_exec()`；动作类型 `highlight` / `notify` / `send_text` / `exec`
- `app_config.rs` — `load_triggers()` + `[[triggers]]` TOML 解析（容错跳过坏条目）
- `iced_app.rs` — `Message::TriggerSendText` / `TriggerExecResult` handler（供未来扫描循环 `Task::done` 派发）

**未接线（端到端缺口）：**
- `load_triggers()` 未在 `iced_app` 启动调用；无 `compiled_triggers` / `trigger_pane_states` 字段
- 无 `TerminalEvent::Wakeup` 扫描循环
- `terminal_shader.rs` 无 `HighlightRegion` 渲染
- 无 `trigger.matched` EventBus emit
- `default-config.toml` 无 `[[triggers]]` 示例
- 单测 `scan_multi_trigger_independent` 当前失败（库 bug，待修）

## 优先级

**P1 — AI 差异化能力**

Triggers 是 iTerm2 最强大的功能之一，将终端从"被动显示"升级为"主动响应"。对 AI agent 工作流尤其重要——可以自动检测 agent 输出中的特定模式并触发动作。

## 工作量

**L（1.5-2 周）**

库 + 配置已落地；剩余 Wakeup 扫描 + highlight 渲染 + notify 动作 + 状态栏计数。

## 实现方向

1. ✅ 配置格式：`[[triggers]]` 段落在 `alacritty.toml`
2. ✅ 匹配器：`regex` crate，`scan_lines` 纯函数
3. 🚧 动作类型：`highlight` / `notify` / `send_text` / `exec`（库定义齐全，运行时未派发）
4. ❌ Wakeup 扫描循环 + highlight GPU  overlay
5. ❌ UI：触发的 trigger 在状态栏显示计数
6. ❌ 可扩展：通过 MCP 暴露 trigger 事件给 AI agent
