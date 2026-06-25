# Browser 面板 + Agent-Browser 兼容

## 概述

在终端 pane 中嵌入 WebView（通过 `wry` crate），支持：
- **GAP-3.5**：`Cmd+Shift+L` 分出新 Web pane / `Cmd+L` 聚焦地址栏 / `Cmd+[ ]` 导航
- **GAP-7.8**：agent-browser 兼容子集（8 async ops + 2 sync ops），MCP 全 op 暴露

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | BrowserWindowPortal（173 KB），WKWebView + agent-browser API | `Sources/BrowserWindowPortal.swift` |
| **iTerm2** | 完整内置浏览器（29 子目录：Core, UI, Bookmarks, History, FindOnPage 等） | `sources/Browser/` |

## PureTerm 现状

**🚧 进行中（代码闭环，待 E2E）。** 

**GAP-3.5 状态**：代码闭环 + 自动 harness 全绿（2026-06-25）
- Phase A：`crates/pureterm/src/browser/` 自实现 wry 嵌入（WKWebView 作 content NSView 子视图）
- Phase C：per-view SOCKS5 proxy 绑定
- 用户交互层：`Cmd+Shift+L` 分出新 Web pane / `Cmd+L` 地址栏 overlay / `normalize_url_input`

**GAP-7.8 状态**：代码闭环（2026-06-24）
- 8 async ops + 2 sync ops：`navigate/click/type/extract/scroll/wait/get_url/get_title` + `screenshot`
- MCP 全 op 暴露：`mcp/server.rs` 现 9 个 browser tool
- CLI：`pureterm browser {…}` 11 子命令

**未做**：
- 后退/前进导航（`Cmd[`/`Cmd]` 与 pane 焦点切换冲突，有意 defer）
- Linux webkit2gtk 原生嵌入（现仅 macOS）
- 交互式 feed-panel 跨 origin 两阶段拦截（现 audit-only）
- 93→125 命令扩展（独立大项目）

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
