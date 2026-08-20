---
gap: GAP-04
title: 计算机操作
category: AI 智能体
plan_type: feature
priority: P2
effort: XL
status: done
closed: 2026-06-29
---

# Computer Use（AI 桌面操控）

## 概述

允许 AI agent 通过屏幕截图 + 鼠标点击/键盘输入来操控桌面应用，将终端 AI 能力扩展到整个操作系统。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | 屏幕截图 + 鼠标点击/键盘输入，Mac/Linux/Win 全平台支持 | `crates/computer_use/src/` |
| **参考实现 B** | 无 | — |
| **参考实现 C** | 无 | — |

## PureTerm 现状

**未实现。** PureTerm 的 AI 集成仅限于终端 PTY 读写和浏览器操控（MCP browser tools），无法操控桌面应用。

## 优先级

**P2 — AI 差异化能力**

Computer Use 是 AI agent 能力的终极扩展，业界将其作为核心能力。但实现复杂度高，且大部分终端用户场景不需要。

## 工作量

**XL（2-3 周）**

需要：屏幕截图 API、OCR/视觉理解、鼠标/键盘事件注入、跨平台抽象。macOS 可用 `CoreGraphics`，Linux 用 `Xlib`/`xdotool`，Windows 用 `winapi`。

## 实现方向

1. 新增 `crates/pureterm-computer-use/` crate
2. 截图：macOS `CGWindowListCreateImage`，Linux `XGetImage`，Windows `BitBlt`
3. 鼠标/键盘：macOS `CGEventPost`，Linux `XTestFakeMotionEvent`，Windows `SendInput`
4. 暴露为 MCP tools：`screenshot()`, `click(x,y)`, `type_text(text)`, `key_press(key)`
5. AI agent 通过截图理解屏幕内容，决定下一步操作
6. 安全：敏感操作需用户确认（类似 Feed Approval）

## 评审（2026-07-01）✅

按本仓 review 范式审计 GAP-04 实现（macOS 截图 + 输入 stub + 4 个 MCP 工具）。核心实现干净（输入注入按设计返回 Err、仅 macOS 截图），修 1 条 spec 明确要求的安全缺口 + 3 个真实小 bug，Review flip ✅。

### 发现与修复

- **[HIGH 安全] 截图缺审批门**（实现方向第 6 条要求「敏感操作需用户确认」）：`desktop_screenshot` 原立即把**全屏未脱敏 PNG**（密码/2FA/私信）交给 agent。**复用 `feed_client::request_approval`**（既有 Feed 面板 deferred-reply），在 handler 内 capture 前发起审批；抽纯函数 `computer_use::approval_allows_screenshot(&FeedOutcome) -> bool`，NoGui/Timeout/Error 一律 **fail-closed** 拒绝。click/type/key 三个 stub 加 SECURITY 指引注释（实现输入注入后须同样过门）。
- **[MED] 临时文件泄漏**：screencapture 退出非 0 或 `fs::read` 失败时 `?` 早退跳过 `remove_file` → PNG 残留 `$TMPDIR`。抽 `read_and_cleanup(path)` 纯函数，**所有路径无条件清理**（success/empty/missing 全覆盖）。
- **[LOW-MED] 临时路径可预测 + 共享回退**：`temp_png_path()` 原 PID-only（可预测，symlink race）；非 UTF8 回退 `unwrap_or("/tmp/pureterm-screenshot.png")` 连 PID 都丢 → 固定共享路径、多实例碰撞。改：非 UTF8 直接报错（不再静默回退共享路径）+ 加 `PID-nanos-counter` 后缀（std-only，无需依赖）。
- **[LOW] `rpc_method` 元数据误导**：4 个 desktop 工具原标 `computer_use.*` Socket 方法，但 handler 是进程内直调、根本无对应 GUI 派发器。改 sentinel `"(in-process)"`（`rpc_method` 仅 CLI 展示/测试用，从不参与派发）+ roster 测试锁契约。
- **[LOW doc] 优先级**：正文 P1 → P2（与 frontmatter + done.md 对齐）。

### 设计边界（非 bug，已接受）

- 输入注入（click/type/key）为 stub 返回 Err —— MVP 故意。
- 仅 macOS 截图（`screencapture`）—— MVP 故意，Linux/Windows 待跨平台抽象。
- 无图像缩放/脱敏/大小封顶 —— 审批门是 MVP 缓解；真脱敏需引入 `image` crate，列 follow-up。
- 临时路径在真正多用户加固主机上的 symlink 残余风险记为单用户局限（如需 mkstemp 级保护加 `tempfile` 依赖）。

### Follow-up（非阻塞）

- 图像脱敏/区域裁剪/大小封顶（复用面：`browser_screenshot` 同样未封顶）。
- 并发 `desktop_screenshot` 调用：Feed 面板单实例，并发会排队/失败。
- 输入注入落地时，click/type/key 接 `request_approval`（注释已指引）。
- 多窗口/无头场景下 `session_id: "computer-use"` 的 Always 记忆粒度（当前按工具记忆）。

### 验证

`cargo test -p pureterm-computer-use`（5 绿）+ `cargo test -p pureterm`（**792 绿 0 回归**，含新 `approval_allows_screenshot`×3 / `data_url_round_trips` / `desktop_tools_use_in_process_sentinel` / `read_and_cleanup`×3 / `temp_png_paths_are_distinct`）。GUI E2E（审批面板弹窗 → Deny 报错 / Allow 出图）待人工非阻塞。
