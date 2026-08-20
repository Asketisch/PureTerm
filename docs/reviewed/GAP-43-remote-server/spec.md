---
gap: GAP-43
title: 远程服务器
category: SSH/网络
plan_type: feature
priority: P2
effort: XL
status: done
closed: 2026-06-29
---

# Remote Server（远程服务器集成）

## 概述

SSH 连接后，远程服务器运行一个轻量 daemon，提供：远程文件浏览、协作编辑、repo metadata sync。类似业界 Remote Server 架构。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | SSH + protobuf daemon，远程文件操作，协作编辑，repo metadata sync | `crates/remote_server/src/`, `app/src/remote_server/` |
| **参考实现 B** | SSH with file browser, folder picker, file transfer | `sources/SSH/` |

## PureTerm 现状

**Done（2026-06-29）：** `puretermd` 控制面 `remote.listdir` / `remote.read` / **`remote.write`** / **`remote.find`**。本地 session 用 `read_dir`/`read`/`write`/`find`；SSH session 复用 ControlMaster（`ls`/`head`/`cat`/`find`）。GUI 侧栏 **Files** 标签：目录浏览、文件名搜索、文件预览、保存预览。

## 优先级

**P2 — 终端增强**

Remote Server 是 SSH 体验的重大升级，但需要远程端安装 daemon。

## 工作量

**XL（2-3 周）** — MVP 控制面 RPC 已落地；完整 Remote Server 仍待后续。

## 实现方向

1. ~~远程 daemon~~ → MVP 复用 OpenSSH ControlMaster，无需独立 remote binary
2. ~~通信：protobuf over SSH channel~~ → MVP 为 daemon JSON-RPC
3. **MVP 功能**：`remote.listdir`（目录列表）
4. 待做：文件读写、目录搜索、repo 信息、侧边栏文件树、文件预览
5. 安全：通过 SSH 认证，无需额外密钥
6. 可选：远程 LSP server（代码补全/诊断）

## 验证

```bash
# 需运行中的 puretermd + 已知 session_id
cargo test -p pureterm-daemon remote_listdir
# 集成：daemon_client::remote_listdir(&socket, session_id, ".")
```
