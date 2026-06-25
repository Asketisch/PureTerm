# Screen Capture（屏幕截图）

## 概述

内置截图功能——截取终端内容（当前可见区域或滚动缓冲区），导出为 PNG/SVG。

## 竞品参考

| 立品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | Built-in screenshot functionality | `sources/Screenshots/` |
| **Warp** | Computer Use 中包含截图能力 | `crates/computer_use/src/` |

## PureTerm 现状

**未实现。** PureTerm 的 MCP Server 有 `capture_pane` tool 返回终端文本，但不支持图片截图。

## 优先级

**P4 — 视觉/分发**

截图功能可通过系统工具（macOS `Cmd+Shift+4`）替代。

## 工作量

**M（1 周）**

需要：终端内容光栅化 → PNG 导出。

## 实现方向

1. 截取当前可见区域：从 GPU 纹理读取
2. 截取滚动缓冲区：从 Grid 数据重建 → 光栅化
3. 导出格式：PNG（`image` crate）、SVG（文本重建）
4. 快捷键：`Cmd+Shift+C` 截取当前 pane
5. MCP tool：`screenshot_pane()` 返回 base64 PNG
6. 与 Browser Screenshot（GAP-7.8）联动
