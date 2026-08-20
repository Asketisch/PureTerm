---
gap: GAP-3.5
title: 浏览器面板
category: UI/外壳
plan_type: feature
priority: P1
effort: L
status: done
closed: 2026-06-25
aliases: [GAP-7.8]
e2e: manual-e2e/gap-3.5-browser.md
---

# Browser 面板 + Agent-Browser 兼容

## 概述

在终端 pane 中嵌入 WebView（通过 `wry` crate），支持：
- **GAP-3.5**：`Cmd+Shift+L` 分出新 Web pane / `Cmd+L` 聚焦地址栏 / `Cmd+[ ]` 导航
- **GAP-7.8**：agent-browser 兼容子集（8 async ops + 2 sync ops），MCP 全 op 暴露

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | BrowserWindowPortal（173 KB），WKWebView + agent-browser API | `Sources/BrowserWindowPortal.swift` |
| **参考实现 A** | 完整内置浏览器（29 子目录：Core, UI, Bookmarks, History, FindOnPage 等） | `sources/Browser/` |

## PureTerm 现状

**Done（2026-06-25）。** 代码闭环 + 自动 harness 全绿；GUI E2E 见 `manual-e2e/gap-3.5-browser.md`。
- Phase A：`crates/pureterm/src/browser/` 自实现 wry 嵌入（WKWebView 作 content NSView 子视图）
- Phase C：per-view SOCKS5 proxy 绑定
- 用户交互层：`Cmd+Shift+L` 分出新 Web pane / `Cmd+L` 地址栏 overlay / `normalize_url_input`

**GAP-7.8 状态**：代码闭环（2026-06-24）
- 8 async ops + `screenshot`（Rust 侧处理，macOS `takeSnapshot`）：`navigate/click/type/extract/scroll/wait/get_url/get_title` + `screenshot`
- MCP 全 op 暴露：`mcp/server.rs` 现 9 个 browser tool
- CLI：`pureterm browser {…}` 11 子命令

**未做**：
- 后退/前进导航（`Cmd[`/`Cmd]` 与 pane 焦点切换冲突，有意 defer）
- Linux webkit2gtk 原生嵌入（现仅 macOS）
- 多窗口父视图精确匹配（`active_ns_view` 现取 key window 的 content NSView，detached 窗里的 web pane 可能错挂到关键窗；随 GAP-25 Multi-Window 落地，需给 NSWindow 打 identifier 标签精确路由）
- 交互式 feed-panel 跨 origin 两阶段拦截（现 audit-only）
- 93→125 命令扩展（独立大项目）

**GAP-7.8 Review（2026-07-01）✅**：执行核心稳健，修 3 处 —— ① `bind_browser_tunnel` 改用纯函数 `bind_tunnel_verdict(status)` 仅放行 `connected` 隧道（stopped/failed 等仍报 `local_addr` 但无监听 → 此前死代理重建 + `navigate` 假成功）；② `browser navigate`/`pane.split_web` 接共享核心 `browser::url::loadable_url()`（裸 `localhost:8080` 此前加载失败；agent 路径严格——非 URL 报错，地址栏仍搜索回退）；③ 7 处 `reply_tx.lock().unwrap()` 统一为毒锁安全的 `ipc::take_reply_sender`。+ screenshot/doc 订正 + 计数订正。新增 7 单测，26 browser + 12 normalize + 55 ipc + 41 cli 全绿 0 回归，dist 重打包。

## 优先级

**P1 — AI 差异化能力**（agent-browser 部分）
**已实现**（Browser 面板基础部分）

## 工作量

**收尾 S（2-3 天）**：E2E 验证
**后续 L（1.5-2 周）**：93→125 命令扩展

## 实现方向

1. E2E：dist `.app` 重打包 → 用户真实页面 GUI 验证
2. 后续：feed-panel 跨 origin 两阶段拦截
3. 后续：93→125 命令扩展（参考 cmux `Sources/Browser/`）
