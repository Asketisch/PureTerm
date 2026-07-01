---
gap: GAP-02
title: Error Recovery
category: AI Agent
plan_type: wrap-up
priority: P0
effort: M
status: done
closed: 2026-06-28
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

**✅ 代码闭环 + Review ✅（2026-06-30）。GUI E2E + 真实 API key 验证待人工（非阻塞）。** Shell Integration 非零 exit 自动触发 + inline 卡片 + Apply 写入 PTY + EventBus + ⌘⇧E 已接线。

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

单测：error_recovery **13** + config **2** + events filter **1** 绿。

## Review（2026-06-30）✅

修 4 类问题 + 记 2 项设计边界（核心 = 对照 GAP-01 补 prompt 注入加固；GAP-02 有 Apply，比 GAP-01 更敏感）：

**修复：**
- **[HIGH/安全] prompt 注入加固**：`build_prompt` 将不可信的 cwd/command/output 包进 `<terminal_context>` 标签、指令置于标签外，`SYSTEM_PROMPT` 加 SECURITY 不可信数据条款（移植自 GAP-01 `ai_chat/context.rs`）；并中和输出里伪造的开/闭标签防越界注入。
- **[MED/正确性] 假审计**：`ErrorRecoveryApply` 原在 PTY 写入分支之外仍 emit `error_recovery.applied` + dismiss → pane 已关时静默 no-op 却谎报「已应用」；改为仅在实际写入活 pane 时 emit + dismiss。
- **[MED/正确性] 关 pane 残留卡片**：error_recovery 为全局单一状态，关 pane 不清理 → 卡片指向死 pane；新增可测方法 `ErrorRecoveryState::dismiss_for_pane`，两处 pane 销毁点按 pane_id 匹配才 dismiss + cancel 在飞流（仿 GAP-15 disjoint-field 内联，避 `&mut self` 与 `tab` 借 `self.windows` 冲突）。
- **[LOW/安全] 收紧 `extract_fix_command`**：去掉「任意 fenced block」回退（仅认 bash/sh/shell/zsh 标签）+ `MAX_FIX_COMMAND_CHARS=512` / `MAX_FIX_COMMAND_LINES=5` 上限，超限不显示 Apply。
- **[LOW/安全] prompt 输出字节上限**：`cap_output` 截断到 `MAX_OUTPUT_CHARS=8KiB`（`capture_row_range` 只限行数不限行宽）。

**设计边界（非 bug，已接受）：**
- **可伪造触发**：退出码来自 shell OSC 133 标记，程序/远端可伪造非零 `OutputEnd` 强制触发一轮 LLM。固有待于 shell 集成；由 prompt 注入隔离 + Apply 需用户点击可见命令缓解。
- **远程 Apply 允许**：Apply 写入失败的那个 pane（本地失败 → 本地，SSH 失败 → 远程）。远程攻击者本就持有该 shell，阻断收益小反而破坏合法 SSH Error Recovery；故不加 `allow_exec_on_remote` 式门控（与 GAP-10 不同）。

单测：error_recovery **13**（+6：注入包裹/中和、输出截断、拒绝未标签块/超长、`dismiss_for_pane`）+ config 2 + events 1 绿，0 回归；release 构建过。

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
