# SOCKS5/HTTP CONNECT 隧道

## 概述

通过 SSH 创建本地 SOCKS5/HTTP CONNECT 代理，支持域名透传（`atyp=3` domain 原样转给远端解析 DNS）。CLI：`pureterm ssh tunnel user@host --local-port 1080 --socks5`。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | SSH Tunnel（`Sources/SSH/Tunnel.swift`） | `Sources/SSH/Tunnel.swift` |
| **Tabby** | 通过 SSH 插件支持端口转发 | — |

## PureTerm 现状

**🚧 进行中（代码闭环，待 E2E）。** 

**已完成**（2026-06-23, Phase B）：
- `puretermd` 持有 russh 0.60 SSH 会话 + 本地 SOCKS5 listener
- `TunnelManager` + per-tunnel async task
- 状态机：connecting → connected/need_auth/failed
- 三路认证：identity(publickey)/password/`oneshot`-parked need_auth
- keepalive 30s×3（R6 死网关检测）
- `tunnel.{start,list,stop,provide_auth}` JSON-RPC
- `ssh tunnel/tunnel-stop/tunnel-list` CLI（daemon-direct）

**与 PTY SSH 双轨**（不能复用 ControlMaster socket）

**未做**：
- `tunnels` 段持久化进 gui-sessions.json（跨 daemon 重启）
- TOFU known_hosts（`check_server_key` MVP 全接受）
- SSH agent forwarding 证书认证
- dist .app 重打包 + 真实 SSH 网关 E2E

## 优先级

**P1 — AI 差异化能力**

SOCKS5 隧道是 AI agent 访问受限网络的基础能力。

## 工作量

**收尾 M（1 周）**：E2E 验证 + 持久化

## 实现方向

1. E2E：`curl --socks5 localhost:1080 example.com` 经 SSH 代理成功
2. 持久化：tunnels 配置写入 `gui-sessions.json`
3. TOFU：首次连接记录 host key，后续验证
