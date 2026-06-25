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

**未实现。** PureTerm 的 agent_hooks 仅在 agent 主动请求时提供终端读写能力，不会自动检测命令失败并触发 AI 分析。

## 优先级

**P0 — AI 核心管道**

错误恢复是 AI 终端最直接的价值体现——用户打错命令，终端自动告诉他怎么改。Kaku 将此作为核心功能。

## 工作量

**M（1 周）**

核心是 shell integration 检测 exit code + 调用 LLM API + 显示建议 UI。如果已有 AI Chat Panel（01），工作量大幅减少。

## 实现方向

1. 依赖 Shell Integration（15）检测命令退出码
2. 捕获失败命令 + stderr 输出（最近 N 行）
3. 调用 LLM provider（复用 01 的 ChatProvider）生成修复建议
4. 在终端下方显示 inline 建议卡片（iced widget）
5. 用户点击"应用"将建议命令写入 PTY
6. 快捷键：`Cmd+Shift+E` 显示/隐藏建议面板
