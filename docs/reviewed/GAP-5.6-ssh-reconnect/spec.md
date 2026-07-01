---
gap: GAP-5.6
title: SSH Reconnect
category: SSH/网络
plan_type: integration
priority: P0
effort: S
status: done
closed: 2026-06-25
e2e: manual-e2e/gap-5.6-ssh-reconnect.md
---

# SSH Reconnect/Disconnect 状态机

## 概述

完整的 SSH 连接生命周期管理：`Connecting → Authenticating → Connected → Disconnected`。支持自动重连（指数退避）、多 pane 共享 session 退出重连、重连成功分隔线、stderr 分类（Auth/Network/HostKey/Unknown）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | 完整事件流状态机 | — |
| **iTerm2** | SSH 重连支持 | — |
| **Termius** | 商业 SSH 客户端，完整重连 | — |

## PureTerm 现状

**✅ 已完成（代码闭环 + harness 绿；GUI 真机 E2E 后补）。**

**已完成**（2026-06-25）：
1. `Authenticating` 独立状态 — spawn 后异步认证，首个远端字节 → `Connected`
2. Socket API event `ssh.state_changed` + `ssh.state` RPC + `pureterm ssh state` CLI
3. 多 pane tab SSH 退出重连 — 撤销 `pane_count<=1` 门控
4. 重连成功分隔线 — `inject_reconnect_marker`
5. stderr 分类器 `classify_ssh_failure`（Auth/Network/HostKey/Unknown）
6. 用户主动断开 → `Disconnected` 终态
7. CLI `ssh disconnect` 补 `--pane-id`
8. 初始连接接入 `Connecting`

**455 harness 绿（+4 新单测）**

**待补（非阻塞）**：GUI 真机 E2E（runbook `../../manual-e2e/gap-5.6-ssh-reconnect.md`）—— 用户授权下先行 flip：~27 个相关单测覆盖状态机/分类器/分隔线/RPC 逻辑不变量，GUI 交互验证作为后续 follow-up。

**Review ✅（2026-06-30）**：状态机本身经逐行核实正确；审计出 3 个重连 worker 生命周期 bug 并全修 —— ① `Fx::SshDisconnect` 补 `reconnect_hub.cancel`（此前不取消在飞 worker → 用户断开被 worker 成功后 reattach「复活」、或被 `Failed` 覆盖 `Disconnected`）；② `close_active_pane` 补 `mark_user_initiated_close`+`cancel`（对齐另两个销毁路径，此前关掉重连中的 pane 残留 worker ~225s + 泄漏 slot/flap_history；`detach_active_pane` 是 move 非 destroy，正确地不动）；③ worker 末次失败抽 `final_attempt()` 纯缝 + `break`，不再睡满 60s 才报 `Failed`、也不再发误导性「将在 60s 后重试」`Progress`。+ 新增 `final_attempt_*` 单测（TDD red→green）；746 pureterm 单测全绿 0 回归，release 编译过。

## 优先级

**P0 — AI 核心管道**

SSH 重连是远程开发的基础保障，AI agent 工作流不能因网络波动中断。

## 工作量

**✅ 收尾 S（已完成 2026-06-25）**：8 项代码闭环 + harness 绿

## 实现方向

1. ✅ flip 2026-06-25（单元测试 + 代码审计 + 用户授权先行）
2. 待补：GUI 真机 E2E 5 场景（正常重连、多 pane、用户断开、初始连接、stderr 分类）—— Claude 自验中
