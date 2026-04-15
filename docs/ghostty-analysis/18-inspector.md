# 18 - 检查器与调试

## 功能概述

基于 Dear ImGui 的终端调试检查器：查看页面内存、终端状态、渲染信息、键盘事件历史、VT 事件流。

## Ghostty 源码文件

| 文件 | 功能 |
|------|------|
| `inspector/Inspector.zig` | 检查器主结构 |
| `inspector/widgets.zig` | 通用 widget 工具 |
| `inspector/units.zig` | 单位转换 |
| `inspector/widgets/page.zig` | 页面内存查看 |
| `inspector/widgets/pagelist.zig` | PageList 概览 |
| `inspector/widgets/key.zig` | 键盘事件历史 |
| `inspector/widgets/renderer.zig` | 渲染器信息 |
| `inspector/widgets/screen.zig` | 屏幕/网格调试 |
| `inspector/widgets/style.zig` | 样式检查 |
| `inspector/widgets/surface.zig` | Surface 级检查器 |
| `inspector/widgets/terminal.zig` | 终端状态 |
| `inspector/widgets/termio.zig` | VT 事件流 |

## 纯 Rust 实现要点

**必须实现**。使用 `egui` 的 `debug` 功能代替 Dear ImGui。可以用 `egui_inspect` 或自定义调试面板。
