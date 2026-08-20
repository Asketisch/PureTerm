---
gap: GAP-48
title: URL 协议
category: 平台
plan_type: platform
priority: P2
effort: M
status: done
closed: 2026-06-29
---

# URL Scheme（URL 方案）

## 概述

注册 `pureterm://` URL scheme 到操作系统，允许从浏览器或其他应用唤起 PureTerm 并执行操作。如 `pureterm://ssh/user@host:22` 打开 SSH 连接。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | `cmux://` URL scheme | — |

## PureTerm 现状

**未实现。**

## 优先级

**P2 — 终端增强**

URL Scheme 是应用间集成的标准方式，对自动化工作流有实用价值。

## 工作量

**M（1 周）**

需要：OS 注册 URL scheme + 事件处理。

## 实现方向

1. macOS：`Info.plist` 添加 `CFBundleURLTypes` + `LSSetDefaultHandlerForURLScheme`
2. Linux：`.desktop` 文件添加 `MimeType=x-scheme-handler/pureterm`
3. Windows：注册表 `HKEY_CLASSES_ROOT\pureterm`
4. URL 解析：`pureterm://ssh/user@host:22` → 解析为 SSH profile
5. 支持的操作：`ssh`, `open`, `command`
