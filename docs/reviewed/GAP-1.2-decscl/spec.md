---
gap: GAP-1.2
title: DECSCL(请求光标形状/颜色)
category: 终端交互
plan_type: feature
priority: P3
effort: M
status: done
closed: 2026-06-29
---

# DECSCL 设备控制

## 概述

实现 DECSCL（Set Conformance Level）CSI 序列处理，支持 VT100/VT200/VT300/VT400 一致性级别切换。控制终端模拟器的行为模式（字符集、转义序列支持范围）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | 完整的终端模式管理，包括 DECSCL | `src/terminal/modes.zig` |

## PureTerm 现状

**未实现（P3）。** PureTerm 的 VT100 parser（基于 alacritty_terminal）支持标准转义序列，但不支持 DECSCL 级别切换。

## 优先级

**P3 — 兼容性/国际化**

DECSCL 是遗留终端应用的兼容性需求，现代应用很少使用。

## 工作量

**M（1 周）**

需要：CSI 序列扩展、模式切换逻辑。

## 实现方向

1. 解析 `CSI <Ps> Ps"p` 序列（DECSCL）
2. 存储当前 conformance level（`TermState` 字段）
3. 根据 level 启用/禁用特定转义序列
4. VT100：基本转义 + 单字节字符集
5. VT200：添加 8-bit 控制字符
6. VT300：添加 Sixel、DCS 等
7. VT400：完整 DEC VT 兼容
