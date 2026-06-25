# Shared Sessions（共享会话）

## 概述

多个用户共享同一个终端会话，支持角色权限（观察者/编辑者）、网络传输层、实时同步。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Warp** | 终端会话共享，多人协作，角色权限，网络传输层 | `app/src/terminal/shared_session/` |

## PureTerm 现状

**未实现。**

## 优先级

**P4 — 视觉/分发**

Shared Sessions 是协作场景功能，对个人用户无需求。tmux 的 `tmux-server` 已提供类似能力。

## 工作量

**XL（2-3 周）**

需要：WebSocket 服务器、终端状态同步、权限管理。

## 实现方向

1. WebRTC 或 WebSocket 传输
2. 会话主持者运行 `pureterm serve --share`
3. 参与者通过 URL 加入
4. 权限：只读 / 读写 / 管理员
5. 同步：终端输出 + 键盘输入 + 窗口大小
6. 安全：token 认证 + 加密传输
