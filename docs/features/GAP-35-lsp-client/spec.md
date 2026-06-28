---
gap: GAP-35
title: LSP Client
category: 开发者工具
plan_type: integration
priority: P1
effort: XL
status: not-started
closed: null
---

# LSP Client（语言服务器协议客户端）

## 概述

内置 LSP client，连接语言服务器（rust-analyzer, gopls, pyright, clangd, ts-ls），提供代码补全、诊断信息、跳转定义、hover 文档等 IDE 功能。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Warp** | 内置 LSP client（rust-analyzer, gopls, pyright, clangd, ts-ls） | `crates/lsp/src/`, `crates/lsp/src/servers/` |

## PureTerm 现状

**未实现。**

## 优先级

**P1 — AI 差异化能力**

LSP Client 是终端内代码补全和诊断的基础，Warp 将其作为 AI 开发环境的核心能力。

## 工作量

**XL（2-3 周）**

需要：LSP 协议实现、语言服务器进程管理、UI 集成（补全弹窗、诊断标记）。

## 实现方向

1. 新增 `crates/pureterm-lsp/` crate
2. LSP 协议：JSON-RPC over stdin/stdout
3. 语言服务器管理：启动/连接/重启
4. 功能：`textDocument/completion`, `textDocument/hover`, `textDocument/definition`, `textDocument/diagnostics`
5. UI：补全弹窗（类似 IDE）、诊断下划线、hover 文档卡片
6. 配置：`lsp.servers = { rust = "rust-analyzer", go = "gopls" }`
7. 与 AI Chat 联动：LSP 诊断可作为 AI 上下文
