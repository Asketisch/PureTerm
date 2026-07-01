---
gap: GAP-45
title: Auto Update
category: 平台
plan_type: platform
priority: P4
effort: M
status: reviewed
closed: 2026-06-29
reviewed: 2026-07-01
---

# Auto Update（自动更新）

## 概述

内置自动更新机制，检测新版本并提示用户安装。macOS 使用 Sparkle 框架，Linux 使用系统包管理器检查，Windows 使用 winget/Squirrel。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Tabby** | electron-updater + keygen.sh + GitHub Actions 夜间构建 | `tabby-core/src/services/updater.service.ts` |
| **cmux** | Sparkle 框架 | `Sources/Updater/SparkleController.swift` |

## PureTerm 现状

**Done（2026-06-29）。** `auto_update.rs` 提供 `AutoUpdateConfig`（`enabled` / `auto_check` / `channel` / `feed_url`，默认 `enabled = false`）和 `check_for_updates`，由 `AppConfig::load_auto_update` 从 `[update]` 读取，启动时通过 `p4_runtime::PureTermRuntime::auto_update_cfg` 装载；CLI 子命令 `pureterm update check`（`p4_cli.rs`）调用 `check_for_updates` 输出 `current_version` / `latest_available` / `message`，以 `PURETERM_LATEST_VERSION` 环境变量作为注入的 latest（Sparkle / GitHub Releases 实际抓取待后续）。

## 优先级

**P4 — 视觉/分发**

自动更新是分发基础设施，但非功能特性。当前可通过 `cargo install` 或 GitHub Releases 手动更新。

## 工作量

**M（1 周）**

macOS：`objc2` 集成 Sparkle。Linux/Windows：检查 GitHub Releases API。

## 实现方向

1. macOS：集成 Sparkle 框架（`SUUpdater`）
2. 跨平台：检查 GitHub Releases API `https://api.github.com/repos/CNB/PureTerm/releases/latest`
3. 启动时检查 + 每 24 小时检查
4. 通知：菜单栏显示"有新版本" → 点击下载
5. 配置：`update.auto_check = true`, `update.channel = "stable" | "nightly"`
