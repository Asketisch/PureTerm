---
gap: GAP-14
title: Enhanced Vim Mode
category: 终端交互
plan_type: enhance
priority: P3
effort: M
status: not-started
closed: null
---

# Enhanced Vim Mode（增强 Vim 模式）

## 概述

在现有 Vi Copy Mode 基础上，扩展为完整的 Vim 模式：支持 motions、operators、text objects、registers、brackets、宏录制/回放。不仅是复制模式，而是完整的 Vim 键绑定体验。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Warp** | 完整 vim 状态机：motions, operators, text objects, registers, brackets | `crates/vim/src/` |
| **iTerm2** | Copy Mode：Vim-like keyboard-driven text selection with cursor navigation, selection modes, mark jumping | `sources/CopyMode/` (iTermCopyModeHandler, iTermCopyModeState) |
| **Tabby** | 通过 xterm.js addon 支持基本 vim 键绑定 | — |

## PureTerm 现状

**已实现基础版。** PureTerm 的 Vi Copy Mode（`crates/pureterm/src/vi_mode_handler.rs`）支持：
- Normal/Visual/VisualLine/VisualBlock 模式
- hjkl 导航
- 18 个 Vi 动作
- yank 操作

**缺少**：完整 operators（d/c/y/p）、text objects（i"/i(/i{）、registers、宏录制。

## 优先级

**P2 — 终端增强**

当前 Vi Copy Mode 已覆盖 80% 使用场景，增强部分面向重度 Vim 用户。

## 工作量

**M（1 周）**

在现有 `vi_mode_handler.rs` 基础上扩展，不需要新架构。

## 实现方向

1. 扩展 `ViAction` 枚举：添加 operators（`Delete`, `Change`, `Put`）
2. 添加 text objects：`InnerWord`, `InnerParen`, `InnerBrace`, `InnerQuote`
3. 实现 registers：`"a`-`"z` 寄存器，`"+` 系统剪贴板
4. 宏录制：`qa` 开始录制，`q` 停止，`@a` 回放
5. 保持与现有 Copy Mode 的兼容性
