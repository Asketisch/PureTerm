---
gap: GAP-39
title: Serial & Telnet
category: SSH/网络
plan_type: feature
priority: P3
effort: L
status: done
closed: 2026-06-29
---

# Serial & Telnet（串口和 Telnet 连接）

## 概述

支持串口（Serial）连接和 Telnet 连接，用于嵌入式开发、网络设备管理等场景。串口支持波特率/数据位/校验位配置，Telnet 支持 socket 连接。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Tabby** | 完整 Serial 连接 + Telnet 连接 + Hex 输入/输出 + 自动重连 | `tabby-serial/src/`, `tabby-telnet/src/` |

## PureTerm 现状

**未实现。** PureTerm 的 SSH 模块（`crates/pureterm-ssh/src/`）仅支持 SSH 协议。

## 优先级

**P3 — 兼容性/国际化**

Serial/Telnet 是嵌入式/网络设备开发的专用需求，受众较窄。

## 工作量

**L（1.5-2 周）**

需要：串口库（`tokio-serial`）、Telnet 协议、新 profile 类型。

## 实现方向

1. 新增 `pureterm-serial` crate（或集成到 `pureterm-ssh`）
2. 串口：`tokio-serial` crate，配置波特率/数据位/校验位/停止位
3. Telnet：`telnet` crate 或手写协议解析
4. Profile 类型扩展：`profile.type = "serial" | "telnet" | "ssh"`
5. Hex 输入/输出：`Ctrl+Shift+H` 切换 hex 模式
6. 自动重连：串口断开后自动重试
