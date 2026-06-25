# PTY Resize（Smallest Screen Wins）

## 概述

多个 Pane attach 同一 SSH session 时，PTY 大小 = min(所有 attached pane 的行列数)。`SshSessionManager` 维护 `attached_surfaces` 列表，每次 resize 取最小值。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | 多 surface 共享 SSH PTY，resize 取最小值 | — |

## PureTerm 现状

**未实现。** PureTerm 的 SSH 模块支持多 pane 共享 session（通过 `SshSessionManager`），但 resize 逻辑未实现 smallest-screen-wins 策略。

**依赖**：GAP-5.1（daemon 化）

## 优先级

**P2 — 终端增强**

PTY resize 在多 pane 共享 SSH 场景下是必要的，否则某个 pane 的内容可能被截断。

## 工作量

**S（2-3 天）**

核心是 resize 事件处理 + min 计算。

## 实现方向

1. `SshSessionManager` 维护 `attached_surfaces: Vec<SurfaceId>`
2. 每个 surface resize 时：计算 `min(cols, rows)` across all attached surfaces
3. 通过 SSH channel 发送 `pty-req` request（新尺寸）
4. 配置：`ssh.smallest_screen_wins = true`
