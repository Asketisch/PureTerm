---
gap: GAP-46
title: i18n
category: 平台
plan_type: feature
priority: P3
effort: L
status: reviewed
closed: 2026-06-29
reviewed: 2026-07-01
---

# Internationalization（国际化）

## 概述

支持多语言界面：菜单、状态栏、命令面板、设置 UI 等所有文本可翻译。支持从右到左（RTL）语言布局。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Tabby** | ngx-translate，10+ 语言，Crowdin 工作流 | `locale/` |

## PureTerm 现状

**Done（2026-06-29）。** `pureterm-settings/src/i18n/mod.rs` 提供 `I18n`（`t` / `t_fmt` / `available_locales` / `set_locale` / `current_locale`），内置 `en` + `zh` 翻译表与 `PickMapping` 助手。主应用通过 `pureterm_settings::i18n::I18n` 持有 `self.i18n: I18n`（`iced_app.rs`），启动时按 `AppConfig.locale`（`config_io` 读取 `pureterm.locale`）调用 `set_locale`，59+9 处 `t`/`t_fmt` 覆盖 SSH 错误、context menu、tab title、palette、feed 提示等。`pureterm-settings` 进程内也使用同一 i18n 模块渲染设置面板文案。

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
