# SSH Forwarding（SSH 转发）

## 概述

支持 SSH 端口转发（本地/远程/动态）和 X11 转发。本地转发：`-L localPort:remoteHost:remotePort`。远程转发：`-R`。动态转发：`-D`（SOCKS5 代理）。X11 转发：`-X`/`-Y`。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Tabby** | X11 转发 + 端口转发 | `tabby-ssh/src/` |
| **iTerm2** | SSH with file browser, folder picker, file transfer | `sources/SSH/` |

## PureTerm 现状

**部分实现。** PureTerm 已实现 SOCKS5 隧道（GAP-5.2，`pureterm ssh tunnel`），但没有完整的端口转发和 X11 转发。

## 优先级

**P2 — 终端增强**

端口转发是 SSH 的核心功能，开发者日常高频使用。X11 转发使用较少。

## 工作量

**M（1 周）**

在现有 SSH 模块基础上扩展端口转发参数。

## 实现方向

1. 本地转发：`pureterm ssh -L 8080:localhost:80 user@host`
2. 远程转发：`pureterm ssh -R 8080:localhost:80 user@host`
3. 动态转发：`pureterm ssh -D 1080 user@host`（已有 SOCKS5 实现）
4. X11 转发：`pureterm ssh -X user@host`（需 X11 依赖，可能仅 Linux/macOS）
5. 配置：`[[ssh.profiles]]` 添加 `local_forward`, `remote_forward`, `dynamic_forward`
6. GUI：转发管理面板（显示活跃转发列表）
