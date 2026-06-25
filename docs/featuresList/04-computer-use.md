# Computer Use（AI 桌面操控）

## 概述

允许 AI agent 通过屏幕截图 + 鼠标点击/键盘输入来操控桌面应用，将终端 AI 能力扩展到整个操作系统。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Warp** | 屏幕截图 + 鼠标点击/键盘输入，Mac/Linux/Win 全平台支持 | `crates/computer_use/src/` |
| **iTerm2** | 无 | — |
| **Kaku** | 无 | — |

## PureTerm 现状

**未实现。** PureTerm 的 AI 集成仅限于终端 PTY 读写和浏览器操控（MCP browser tools），无法操控桌面应用。

## 优先级

**P1 — AI 差异化能力**

Computer Use 是 AI agent 能力的终极扩展，Warp 将其作为差异化功能。但实现复杂度高，且大部分终端用户场景不需要。

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
