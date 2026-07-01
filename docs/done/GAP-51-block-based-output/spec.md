---
gap: GAP-51
title: Block-based Output
category: 集成
plan_type: arch
priority: P2
effort: XXL
status: done
closed: 2026-06-29
---

# Block-based Output（块级输出）

## 概述

Warp 的核心创新——将终端输出按命令分割为独立 Block，每个 Block 包含：命令文本、输出内容、exit code、执行时间、session ID。支持 Block 过滤、搜索、元数据查看。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Warp** | Block-based output，sum-tree O(log n) 查找，Block 过滤/搜索/metadata，Rich Content（Block 内嵌 AI 视图） | `app/src/terminal/model/block.rs` (3345 lines), `app/src/terminal/block_list_viewport.rs` (1600+ lines) |

## PureTerm 现状

**Done（2026-06-29）：** `[block_output].enabled` 配置。`output_blocks.rs`：`OutputBlock` / `PaneBlockStore` / **`BlockSumTree`**（O(log n) 行查找）。OSC 133 D mark 追加 block（含 **grid 切片 output_text**）并 emit `block.finished`。右侧边栏 **Blocks** 标签：列表、Fail/OK 过滤、命令+输出搜索、点击跳转。

## 优先级

**P2 — 终端增强**

Block-based Output 是 Warp 的核心差异化功能，但需要重构终端输出模型，工程量巨大。

## 工作量

**XXL（4+ 周）** — scaffold 已落地；完整重构仍待后续。

## 实现方向

1. 数据结构：Sum-tree 存储 Block 列表，O(log n) 按行查找
2. Block 元数据：命令文本、exit code、duration、session ID
3. 输出解析：检测 shell prompt 分割 Block
4. UI：Block 列表视图（替代传统 scrollback）
5. 过滤：按 exit code / 命令文本 / 时间范围过滤
6. 搜索：Block 级别搜索（命令 + 输出）
7. Rich Content：Block 内嵌 AI 视图、markdown 渲染
8. 与 Shell Integration（15）联动：prompt 检测是 Block 分割的基础
