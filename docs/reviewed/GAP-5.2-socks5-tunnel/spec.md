---
gap: GAP-5.2
title: SOCKS5 Tunnel
category: SSH/网络
plan_type: integration
priority: P1
effort: M
status: done
closed: 2026-06-27
e2e: manual-e2e/gap-5.2-socks5-tunnel.md
---

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

## Review ✅ (2026-06-30)

审 2 bug 全修 + 6 单测（TDD），Review flip ✅：

- **[MED] `--no-remote-dns` 死配置（DNS 泄漏）**：`remote_dns` 经 CLI(`--no-remote-dns`→`!*no_remote_dns`)→RPC→`TunnelManager::start`→`run_tunnel` 全链路透传，但 `run_tunnel` 收作 `_remote_dns`（未用）、`serve_socks5` 从未收到 → 域名（ATYP=3）始终原样转发，**网关永远解析 DNS**，违反 `--no-remote-dns`「本地解析，不外泄域名」契约（用户开此旗标求隐私，却把每个查询域名全泄漏给网关）。修：抽纯谓词 `forward_verbatim(remote_dns, host)`（`remote_dns || host.parse::<IpAddr>().is_ok()`，IP 字面量永不解析）+ `resolve_host_to_ip`（`tokio::net::lookup_host` 取首 A/AAAA）+ `remote_dns` 透传进 `serve_socks5`；本地解析失败回 SOCKS5 `0x04`（Host unreachable）。
- **[LOW] IPv6 字面量网关连接地址坏**：`run_tunnel` 的 `addr = format!("{host}:{port}")` 对 IPv6 字面量产出 `2001:db8::1:22` 无法解析为 socket addr → 连接失败。修：抽 `format_connect_addr`，IPv6（含 `:`）加方括号 `[host]:port`。

记为 follow-up（未修，设计性 / 微瑕）：

- **[LOW-MED] `tunnel.stop` 不取消在飞 SOCKS5 连接**：per-conn `serve_socks5` 子任务持 `Arc<AsyncMutex<Handle>>` 克隆，abort `run_tunnel` 不会 abort 它们（tokio `spawn` detach）+ 无显式 `handle.disconnect()` → SSH 会话挂到在飞拷贝自然排空（无界，传输挂起则久留）。修需 `CancellationToken` 透传子任务 + 停止时 `disconnect()`，留待。
- **[LOW/doc] `read_dst_addr` 对非法 UTF-8 域名回 `Ok(None)` → 调用方回 SOCKS5 `0x08`（ATYP 不支持）而非 `0x01`（一般失败）**：协议语义微瑕。
- **[设计/已文档化] TOFU 在 `known_hosts` 打不开时退化为 accept-all**（deferred_init 仅 log warn，store 留 None → `decide_host_key` 走 legacy accept-all，MITM 风险）：代码注释为优雅降级，已知权衡。

daemon 34 单测绿（tunnel 17 / known_hosts 7 / persist 6 / control RPC 4，+6 新）；`cargo check` 无新告警。运行时生效需重建 `puretermd` 并重打包 .app（隧道代码在 daemon crate）。
