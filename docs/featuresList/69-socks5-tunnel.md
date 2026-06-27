# SOCKS5/HTTP CONNECT 隧道

## 概述

通过 SSH 创建本地 SOCKS5/HTTP CONNECT 代理，支持域名透传（`atyp=3` domain 原样转给远端解析 DNS）。CLI：`pureterm ssh tunnel user@host --local-port 1080 --socks5`。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | SSH Tunnel（`Sources/SSH/Tunnel.swift`） | `Sources/SSH/Tunnel.swift` |
| **Tabby** | 通过 SSH 插件支持端口转发 | — |

## PureTerm 现状

**✅ 代码闭环 + 单测绿（2026-06-27）。** 真实 SSH 网关 E2E 写入手册，作为**非阻塞**人工 follow-up（GAP-5.6/5.4 先例）。

**核心**（2026-06-23, Phase B）：
- `puretermd` 持有 russh 0.60 SSH 会话 + 本地 SOCKS5 listener
- `TunnelManager` + per-tunnel async task
- 状态机：connecting → connected/need_auth/failed/stopped
- 三路认证：identity(publickey)/password/`oneshot`-parked need_auth
- keepalive 30s（R6 死网关检测）
- `tunnel.{start,list,stop,provide_auth}` JSON-RPC
- `ssh tunnel/tunnel-stop/tunnel-list` CLI（daemon-direct）

**与 PTY SSH 双轨**（不能复用 ControlMaster socket）

**收尾闭环**（2026-06-27）：
- **TOFU known_hosts**：`check_server_key` 不再 accept-all —— `known_hosts.rs` 隔离文件（`data_dir()/known_hosts`，不动用户 `~/.ssh/known_hosts`），首连记录 / 复连 Match / 换 key 严格拒绝（Mismatch 经 `Failed` 状态 + `tunnel.list` 浮现，无需新 RPC）。OpenSSH 行格式（`[host]:port algo blob`），仅字面 host 匹配（wildcard/hashed 不在 MVP）。
- **持久化**：`tunnels.json`（daemon 自有，`data_dir()/tunnels.json`）跨 daemon 重启。`persist.rs` 的 `TunnelConfig` **不存密码**（对齐 SSH-profile）；`start`/`stop` 写入点 + 启动**仅恢复配置**（注册为 `stopped`，用户手动 `tunnel.start`；不自动重连，避免 hammering 抖动网关 + 不泄漏密码）。
- **agent 转发**：GAP-5.4 Track B 本就是隧道路径，早已落地（`try_agent_auth` + `agent_forward` + server-agent 桥），无需新增。

## 优先级

**P1 — AI 差异化能力**

SOCKS5 隧道是 AI agent 访问受限网络的基础能力。

## 工作量

**收尾 M（1 周）**：E2E 验证 + 持久化 —— ✅ 持久化 + TOFU 已闭环

## 实现方向

1. E2E：`curl --socks5-hostname localhost:1080 example.com` 经 SSH 代理成功 —— 手册待人工跑
2. 持久化：tunnels 配置写入 daemon `tunnels.json` —— ✅
3. TOFU：首次连接记录 host key，后续验证 —— ✅
