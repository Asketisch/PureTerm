---
gap: GAP-40
title: SSH Forwarding
category: SSH/网络
plan_type: integration
priority: P2
effort: M
status: done
closed: 2026-06-29
aliases: [GAP-5.4]
---

# SSH Forwarding（SSH 转发）

## 概述

支持 SSH 端口转发（本地/远程/动态）和 X11 转发。本地转发：`-L localPort:remoteHost:remotePort`。远程转发：`-R`。动态转发：`-D`（SOCKS5 代理）。X11 转发：`-X`/`-Y`。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | X11 转发 + 端口转发 | `tabby-ssh/src/` |
| **参考实现 B** | SSH with file browser, folder picker, file transfer | `sources/SSH/` |

## PureTerm 现状

**部分实现。** 子能力分拆如下：

| 子能力 | GAP | 状态 |
|--------|-----|------|
| SOCKS5/HTTP CONNECT 动态转发 | GAP-5.2 | ✅ `pureterm ssh tunnel` + daemon `TunnelManager` |
| SSH Agent Forwarding | GAP-5.4 | ✅ Track A DoD（`-o ForwardAgent=yes` + `SSH_AUTH_SOCK` 注入）；Track B daemon russh 隧道待真机 E2E |
| 本地/远程端口转发 `-L/-R` | — | ❌ |
| X11 转发 `-X/-Y` | — | ❌ |

Agent Forwarding 实现：`pureterm-ssh/src/agent_sock.rs`（macOS `launchctl getenv` 回退）+ profile `forward_agent` / CLI `-A` → daemon `ssh -tt … -o ForwardAgent=yes`。

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
