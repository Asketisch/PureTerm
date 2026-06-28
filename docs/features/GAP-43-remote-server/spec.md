---
gap: GAP-43
title: Remote Server
category: SSH/网络
plan_type: feature
priority: P2
effort: XL
status: not-started
closed: null
---

# Remote Server（远程服务器集成）

## 概述

SSH 连接后，远程服务器运行一个轻量 daemon，提供：远程文件浏览、协作编辑、repo metadata sync。类似 Warp 的 Remote Server 架构。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Warp** | SSH + protobuf daemon，远程文件操作，协作编辑，repo metadata sync | `crates/remote_server/src/`, `app/src/remote_server/` |
| **iTerm2** | SSH with file browser, folder picker, file transfer | `sources/SSH/` |

## PureTerm 现状

**未实现。** PureTerm 的 SSH 仅提供 PTY 交互，远程文件操作需通过命令行。

## 优先级

**P2 — 终端增强**

Remote Server 是 SSH 体验的重大升级，但需要远程端安装 daemon。

## 工作量

**XL（2-3 周）**

需要：远程 daemon（Rust binary）、protobuf 协议、文件系统抽象。

## 实现方向

1. 远程 daemon：`pureterm-remote` binary，通过 SSH 安装/启动
2. 通信：protobuf over SSH channel（非 TCP）
3. 功能：文件列表、文件读写、目录搜索、repo 信息
4. 本地集成：侧边栏文件树、文件预览
5. 安全：通过 SSH 认证，无需额外密钥
6. 可选：远程 LSP server（代码补全/诊断）
