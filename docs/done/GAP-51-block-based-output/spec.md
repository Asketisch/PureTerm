---
gap: GAP-51
title: 块式输出
category: 集成
plan_type: arch
priority: P2
effort: XXL
status: done
v1_closed: 2026-07-27
priority: P2
arch: true
moved_to_todo: 2026-07-02
moved_reason: "Block-based output 跟后端 shell integration / OSC 133 / grid 切片耦合（arch 类），scaffold 已 done 但完整重构 (XXL 4+ 周) 暂不在视觉/平台冲刺范围内，按用户指示后端相关移到 todo"
---

# Block-based Output（块级输出）

## 概述

block 输出模型的核心创新——将终端输出按命令分割为独立 Block，每个 Block 包含：命令文本、输出内容、exit code、执行时间、session ID。支持 Block 过滤、搜索、元数据查看。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|------|
| **参考实现 A** | Block-based output，sum-tree O(log n) 查找，Block 过滤/搜索/metadata，Rich Content（Block 内嵌 AI 视图） | `app/src/terminal/model/block.rs` (3345 lines), `app/src/terminal/block_list_viewport.rs` (1600+ lines) |

## PureTerm 现状

**v1 闭环（2026-06-29 → 2026-07-27）：**

1. **数据模型**：`OutputBlock` 补齐全部字段（`session_id` / `cwd` / `started_at_us` / `kind` / `tags`）。
2. **配置**：`[block_output].enabled` 开关 + 完整 metadata 采集。
3. **OSC 133 D mark 处理**：`PaneBlockStore::push()` 从 `FinishedCommand` + grid slice 拼出 block。
4. **Grid slice 异步化**：`capture_row_range_async` → `spawn_blocking`，不再卡 GUI 线程。
5. **Sum-tree 增量更新**：`BlockSumTree::push()` O(1) amortized，仅在裁剪时 O(n) drain。
6. **视图过滤**：`PaneBlockStore::filtered()` 为单一数据源，视图只渲染分页后的 chunk（`PAGE_SIZE = 50`）。
7. **事件名统一**：`emit("block.finished", ...)`（原 `shell.block_finished`，无外部消费者，直更）。
8. **测试覆盖**：output_blocks 7 单测 + shell_state 10 单测 + surface_snapshot 9 单测。

## 优先级

**P2 — 终端增强**

Block-based Output 是 block 输出模型的差异化功能，但需要重构终端输出模型，工程量巨大。

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

## 后续工作（v2/v3）

- Fuzzy search（`nucleo_matcher` 复用 palette 模板）
- 时间范围过滤 / inline 子串高亮（`rich_text` 复用 markdown）
- Block 展开详情面板（不替代 scrollback，但点开看完整 output + markdown）
- OutputBlock 持久化（`Serialize` on `OutputBlock`，存 `~/.config/pureterm/blocks/<session>.jsonl`）
- Action / 快捷键（`JumpToNextBlock` / `JumpToPrevBlock` / `ToggleBlocksTab`）
- cloud_sync 集成（`SyncBundle.blocks` 字段）
- 主视图替换 scrollback（block stream 渲染层）
