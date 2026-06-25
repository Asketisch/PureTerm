# Auto Update（自动更新）

## 概述

内置自动更新机制，检测新版本并提示用户安装。macOS 使用 Sparkle 框架，Linux 使用系统包管理器检查，Windows 使用 winget/Squirrel。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Tabby** | electron-updater + keygen.sh + GitHub Actions 夜间构建 | `tabby-core/src/services/updater.service.ts` |
| **cmux** | Sparkle 框架 | `Sources/Updater/SparkleController.swift` |

## PureTerm 现状

**未实现。**

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
