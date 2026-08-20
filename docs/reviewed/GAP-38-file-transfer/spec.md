---
gap: GAP-38
title: 文件传输
category: SSH/网络
plan_type: feature
priority: P4
effort: L
status: done
closed: 2026-06-29
---

# File Transfer（文件传输）

## 概述

通过 SSH 连接传输文件：SCP/SFTP 上传/下载。支持拖拽上传、Zmodem 协议。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | SCP/SFTP support，拖拽上传 | `sources/FileTransfer/` (FileTransferManager, SCPFile, SCPPath, TerminalFile, TransferrableFile) |
| **参考实现 B** | Zmodem 文件传输（通过 zmodem.js） | — |

## PureTerm 现状

**未实现。** PureTerm 的 SSH 模块（`crates/pureterm-ssh/src/`）支持 PTY 交互，但没有文件传输能力。

## 优先级

**P3 — 兼容性/国际化**

文件传输可通过 `scp`/`rsync` 命令替代，但 GUI 集成更方便。

## 工作量

**L（1.5-2 周）**

需要：SCP/SFTP 协议实现、拖拽事件处理、传输进度 UI。

## 实现方向

1. SCP：通过 `russh-scp` crate 实现
2. SFTP：通过 `russh` 的 SFTP subsystem
3. 拖拽上传：iced drop event → 解析文件路径 → SCP 上传
4. 下载：智能选择路径 → 右键"下载" → SCP 下载到 ~/Downloads
5. 进度条：Status Bar 显示传输进度
6. Zmodem：可选，通过 `zmodem` crate
