---
gap: GAP-46
title: i18n
category: 平台
plan_type: feature
priority: P3
effort: L
status: done
closed: 2026-06-29
---

# Internationalization（国际化）

## 概述

支持多语言界面：菜单、状态栏、命令面板、设置 UI 等所有文本可翻译。支持从右到左（RTL）语言布局。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Tabby** | ngx-translate，10+ 语言，Crowdin 工作流 | `locale/` |

## PureTerm 现状

**未实现。** PureTerm 的 UI 文本硬编码为英文。

## 优先级

**P3 — 兼容性/国际化**

国际化对非英语用户有需求，但 PureTerm 的主要用户群（开发者）大多能接受英文 UI。

## 工作量

**L（1.5-2 周）**

需要：i18n 框架、翻译文件、UI 文本提取。

## 实现方向

1. 框架：`fluent` crate（Mozilla 的 i18n 库）
2. 翻译文件：`locales/en/`, `locales/zh-CN/`, `locales/ja/` 等
3. UI 文本提取：所有硬编码字符串替换为 `t!("key")`
4. 语言检测：系统语言 > 配置 > 默认英文
5. 配置：`locale = "zh-CN"`
6. 热切换：设置 UI 中切换语言即时生效
