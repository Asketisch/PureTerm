---
gap: GAP-02
title: Error Recovery
category: AI Agent
plan_type: wrap-up
priority: P0
effort: M
status: in-progress
closed: null
e2e: manual-e2e/gap-02-error-recovery.md
---

# Error Recovery（错误自动恢复）

## 概述

当命令执行失败（非零退出码）时，自动将失败命令和错误输出发送给 LLM 分析，在终端内显示修复建议。用户可一键应用建议的修复命令。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Kaku** | 非零退出时自动发送失败命令给 LLM，显示修复建议（`Cmd+Shift+E`） | `Kaku-main/crates/kaku/src/error_recovery.rs` |
| **Warp** | Block-based output 中每个 command block 记录 exit code，AI 可基于上下文分析 | `app/src/terminal/model/block.rs` (exit_code 字段) |
| **iTerm2** | Triggers 系统可匹配 exit code 并触发通知/脚本，但无内置 AI 分析 | `sources/Triggers/` |

## PureTerm 现状

**🚧 代码闭环（~90% 端到端，待 GUI E2E + API key 真机验证）。** Shell Integration 非零 exit 自动触发 + inline 卡片 + Apply 写入 PTY + EventBus + ⌘⇧E 已接线。

**v1 已交付（2026-06-28）：**
- `error_recovery.rs` — `FailureContext` / `ErrorRecoveryState` / `build_prompt` / `extract_fix_command` / `should_auto_analyze`
- `surface_snapshot.rs` — `capture_visible_tail` / `capture_row_range`（命令输出捕获）
- `app_config.rs` — `[error_recovery]` 配置（provider/model/auto_analyze/max_output_lines）
- `iced_app.rs` — ShellMark OutputEnd 自动触发、`spawn_error_recovery_stream`（复用 `ai_chat` Anthropic/OpenAI provider）、`view_error_recovery_card`、Apply/Dismiss/Stop
- `keymap.rs` — `⌘⇧E` + 命令面板 `error_recovery.toggle`
- EventBus — `error_recovery.triggered` / `suggested` / `applied`（`Category::ErrorRecovery`）

**待补（非阻塞 flip ✅）：**
- pixel smoke：[`../../manual-e2e/gap-02-error-recovery.md`](../../manual-e2e/gap-02-error-recovery.md)（需真实 API key）
- Settings UI 配置页（defer GAP-01 Phase 3）

单测：error_recovery **7** + config **2** + events filter **1** 绿。

## 优先级

**P0 — AI 核心管道**

错误恢复是 AI 终端最直接的价值体现——用户打错命令，终端自动告诉他怎么改。Kaku 将此作为核心功能。

## 工作量

**M（1 周）** — MVP 代码已落地；剩余 GUI E2E + 文档 flip。

## 实现方向

1. ✅ 依赖 Shell Integration（GAP-15）检测命令退出码
2. ✅ 捕获失败命令 + 输出 tail（grid 可见区 / C..D row range）
3. ✅ 调用 LLM provider（复用 GAP-01 的 `ChatProvider`）
4. ✅ 终端底部 inline 建议卡片（iced overlay）
5. ✅ 用户点击 Apply 将建议命令写入 PTY
6. ✅ 快捷键：`⌘⇧E` 显示/重分析
