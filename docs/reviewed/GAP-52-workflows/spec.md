---
gap: GAP-52
title: Workflows
category: 集成
plan_type: feature
priority: P4
effort: M
status: reviewed
closed: 2026-06-29
reviewed: 2026-07-01
---

# Workflows（可复用工作流）

## 概述

将常用命令序列保存为 Workflows，支持参数化、类别、别名、云端共享。类似 Warp 的 Workflows 功能。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Warp** | 可复用命令序列，参数化，类别，别名，云端共享 | `app/src/workflows/` |

## PureTerm 现状

**Done（2026-06-29）。** `workflows.rs` 提供 `Workflow`（`name` / `description` / `steps` / `params`）和 `render_steps`（`{{param}}` 占位符替换），`load_workflows_from_toml` 从 `[[workflows]]` 段落反序列化。`[workflows] enabled` 默认 `true` 控制开关。云端共享（Gist）按 spec 留待后续。

## 优先级

**P2 — 终端增强**

Workflows 是开发者效率工具，cmux 的 Custom Commands 也是类似功能。

## 工作量

**M（1 周）**

在现有 Custom Commands 基础上扩展参数化和 UI。

## 实现方向

1. 配置格式：`[[workflows]]` 段落
2. 参数化：`workflow.deploy = { cmd = "ssh prod 'cd /app && git pull && systemctl restart {{service}}'", params = ["service"] }`
3. UI：Command Palette 中显示 Workflows 分类
4. 执行：参数输入弹窗
5. 别名：`wd` → `workflow deploy`
6. 云端共享：GitHub Gist 导入/导出
