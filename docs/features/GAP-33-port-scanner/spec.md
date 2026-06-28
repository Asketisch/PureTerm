---
gap: GAP-33
title: Port Scanner
category: 开发者工具
plan_type: feature
priority: P2
effort: S
status: not-started
closed: null
---

# Port Scanner（端口扫描器）

## 概述

在侧边栏或 Status Bar 显示当前系统上所有监听端口及其对应的进程名/PID。帮助开发者快速查看哪些服务在运行。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | 侧边栏显示每个 workspace 的监听端口 | — |

## PureTerm 现状

**未实现。**

## 优先级

**P2 — 终端增强**

端口扫描器在微服务开发中很实用，但可通过 `lsof -i -P` 命令替代。

## 工作量

**S（3-5 天）**

需要：定期扫描监听端口（`sysinfo` crate 或解析 `/proc/net/tcp`）。

## 实现方向

1. 定期扫描：每 5s 读取监听端口（`net2` crate 或 `sysinfo`）
2. 侧边栏 Panel：显示端口列表（端口号、进程名、PID）
3. 点击端口：复制 `localhost:PORT` 或打开浏览器
4. 配置：`port_scanner.enabled = true`, `port_scanner.interval = 5`
5. 过滤：排除系统端口（< 1024）
