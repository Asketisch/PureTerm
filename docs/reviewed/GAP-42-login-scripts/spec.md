---
gap: GAP-42
title: 登录脚本
category: SSH/网络
plan_type: feature
priority: P3
effort: S
status: done
closed: 2026-06-29
---

# Login Scripts（登录脚本）

## 概述

SSH 连接成功后自动执行预定义的命令序列（如加载环境变量、激活 virtualenv、切换目录）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | SSH 登录自动化脚本 | `tabby-ssh/src/` |

## PureTerm 现状

**未实现。**

## 优先级

**P3 — 兼容性/国际化**

登录脚本是便利性功能，可通过 `.bashrc`/`.zshrc` 替代。

## 工作量

**S（2-3 天）**

核心是配置解析 + PTY 写入。

## 实现方向

1. Profile 扩展：`login_commands = ["cd /app", "source venv/bin/activate", "docker ps"]`
2. SSH 连接成功后：等待 shell prompt → 逐条执行命令
3. 执行间隔：可配置（默认 500ms）
4. 错误处理：某条命令失败时的策略（继续/中断）
5. 配置 UI：Profile 编辑器中添加"登录脚本"字段
