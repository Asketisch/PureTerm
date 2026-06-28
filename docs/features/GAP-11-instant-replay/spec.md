---
gap: GAP-11
title: Instant Replay
category: 终端交互
plan_type: feature
priority: P2
effort: L
status: not-started
closed: null
---

# Instant Replay（即时回放）

## 概述

DVR 风格的会话回放——自动记录终端输出的时间序列，用户可以拖动时间轴回放任意时刻的终端状态。支持 run-length 压缩、时间戳标记。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | DVR with playback UI，run-length compression，scrubbing and timestamps | `sources/DVR/` (DVR.h/m, DVREncoder.h/m, DVRDecoder.h/m, iTermInstantReplayWindowController.h/m) |
| **Kaku** | Window Snapshots（保存/恢复布局），但非 DVR 回放 | `Kaku-main/crates/kaku/src/snapshots.rs` |

## PureTerm 现状

**未实现。** PureTerm 的 daemon 有 1MB ring buffer 用于崩溃恢复，但没有时间序列记录和回放 UI。

## 优先级

**P2 — 终端增强**

Instant Replay 是调试利器，但实现复杂度高（需要高效的时间序列存储和回放引擎）。

## 工作量

**L（1.5-2 周）**

需要：时间序列存储（delta encoding）、回放引擎、回放 UI（时间轴 + 终端快照）。

## 实现方向

1. 在 daemon 层记录输出时间戳（每个 output chunk 附带 `Instant`）
2. 存储：delta encoding + run-length compression（类 iTerm2 DVR）
3. 回放 UI：独立窗口，时间轴 slider + 终端内容快照
4. 快捷键：`Cmd+Shift+R` 进入回放模式
5. 性能：后台压缩，存储上限可配置（默认 100MB）
