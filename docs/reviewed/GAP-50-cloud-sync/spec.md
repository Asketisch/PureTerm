---
gap: GAP-50
title: Cloud Sync
category: 集成
plan_type: integration
priority: P4
effort: M
status: reviewed
closed: 2026-06-29
reviewed: 2026-07-01
---

# Cloud Sync（云端同步）

## 概述

将设置、配置、SSH profiles、快捷键绑定等同步到云端，跨设备保持一致。支持 GitHub Gist 或自建服务器。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Warp** | Settings/preferences 云端同步 | `app/src/settings/cloud_preferences_syncer.rs` |

## PureTerm 现状

**Done（2026-06-29，本地 bundle MVP）。** `cloud_sync.rs` 提供 `CloudSyncConfig`（`enabled` / `gist_id` / `token_env`，默认 `token_env = "PURETERM_GIST_TOKEN"`）和 `SyncBundle`（`config_toml` + `exported_at`），`sync_push` 把 `alacritty.toml` 打包到 `~/.config/pureterm/sync/latest.json`，`sync_pull` 从 bundle 还原。Gist 上行/下行（`token_env` + `gist_id`）按 spec 留待后续。

## 优先级

**P4 — 视觉/分发**

Cloud Sync 是便利性功能，可通过 dotfiles 管理（git repo）替代。

## 工作量

**M（1 周）**

核心是配置序列化 + GitHub Gist API。

## 实现方向

1. 同步后端：GitHub Gist（用户授权 token）
2. 同步内容：`alacritty.toml` + SSH profiles + 快捷键绑定
3. 冲突解决：last-write-wins 或手动选择
4. 配置：`cloud_sync.enabled = true`, `cloud_sync.gist_id = "..."`
5. CLI：`pureterm sync push` / `pureterm sync pull`
