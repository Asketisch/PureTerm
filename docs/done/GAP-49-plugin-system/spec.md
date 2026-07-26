---
gap: GAP-49
title: Plugin System
category: 集成
plan_type: arch
priority: P4
effort: XL
status: done
closed: 2026-07-02
---

# Plugin System（插件系统）

## 概述

可扩展的插件架构，允许第三方开发者通过插件添加功能：新工具、新主题、新 profile provider、新状态栏组件。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | Angular NgModule 插件架构，NPM 包管理，社区插件（Docker, Quick Cmds, Save Output 等） | `tabby-plugin-manager/src/`, `app/src/plugins.ts` |

## PureTerm 现状

**Done（2026-07-02）。** plugin_system.rs 目录扫描 + plugin.toml 清单解析/校验 + install/uninstall + CLI 表面 + 9 单测。 PureTerm 的扩展方式是通过配置（SSH profiles, Commands, Agent hooks），没有运行时插件系统。

## 优先级

**P3 — 兼容性/国际化**

插件系统是长期生态建设，但需要稳定的 API 和大量文档。当前优先级不高。

## 工作量

**XL（3+ 周）**

需要：插件运行时（WASM 或动态链接）、插件 API 设计、插件市场。

## 实现方向

1. 轻量方案：配置文件插件（`~/.config/pureterm/plugins/` 下的脚本）
2. 中量方案：WASM 插件（`wasmtime` crate）
3. 重量方案：动态链接（`.dylib`/`.so`/`.dll`）
4. 扩展点：ProfileProvider, ThemeProvider, StatusBarComponent, ToolProvider
5. 插件管理：CLI `pureterm plugin install <name>`
6. 社区：GitHub 仓库 + 简单审核流程
