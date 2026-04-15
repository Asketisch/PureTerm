# 19 - C API 与库导出

## 功能概述

libghostty 和 libghostty-vt 的 C API 导出，使终端引擎可以被 C/ObjC/Swift/WASM 调用。

## Ghostty 源码文件

### 终端 C API (26)
`terminal/c/` 目录下的所有文件，导出 `ghostty_*` 前缀的 C 函数。

### 库工具 (9)
`lib/` 目录下的 C ABI 兼容类型生成器。

### 入口 (3)
| 文件 | 功能 |
|------|------|
| `lib_vt.zig` | ghostty-vt 模块公共 API |
| `main_c.zig` | libghostty C 入口 |
| `main_wasm.zig` | WASM 模块入口 |

## C API 功能清单

| 类别 | 函数前缀 | 功能 |
|------|---------|------|
| 初始化 | `ghostty_init` | 全局初始化 |
| 终端 | `ghostty_terminal_*` | 创建/销毁/写入/调整终端 |
| 键盘 | `ghostty_key_event_*`, `ghostty_key_encode_*` | 键事件和编码 |
| 鼠标 | `ghostty_mouse_event_*`, `ghostty_mouse_encode_*` | 鼠标事件和编码 |
| 粘贴 | `ghostty_paste_*` | 粘贴安全检查和编码 |
| 焦点 | `ghostty_focus_*` | 焦点事件编码 |
| SGR | `ghostty_sgr_*` | SGR 属性解析 |
| OSC | `ghostty_osc_*` | OSC 序列解析 |
| 格式化 | `ghostty_formatter_*` | 终端内容格式化 |
| 渲染 | `ghostty_render_*` | 渲染状态查询 |
| Kitty | `ghostty_kitty_*` | Kitty 图形查询 |
| 网格 | `ghostty_grid_*` | 网格引用和单元格查询 |
| 构建 | `ghostty_build_info_*` | 构建信息 |
| 配置 | `ghostty_config_*` | 配置管理 |

## 纯 Rust 实现要点

**PureTerm 不需要 C API**。但如果将来需要嵌入其他应用，可以用 `cbindgen` 或 `uniffi` 生成 FFI 绑定。属于不需要实现的部分。
