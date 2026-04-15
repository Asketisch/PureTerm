# Ghostty 源码功能分析 - 总览文档

本文档将 Ghostty 终端模拟器的全部 Zig 源码按实际功能分为 **20 个功能区块**，
每个区块对应 PureTerm 中需要用 Rust 实现的目标模块。

---

## 源码规模统计

| 目录 | .zig 文件数 | 核心功能 |
|------|-----------|---------|
| `src/terminal/` | 108 | VTE 解析、屏幕网格、模式管理、OSC/DCS/APC、Kitty 协议、Tmux、搜索 |
| `src/renderer/` | ~34 | GPU 渲染管线（Metal/OpenGL/WebGL）、字体图集、着色器 |
| `src/input/` | 17 | 键盘编码、鼠标编码、绑定系统、粘贴、键映射 |
| `src/config/` | 19 | 配置加载/验证/序列化、主题、条件配置 |
| `src/apprt/` | 56 | 应用运行时抽象（GTK/嵌入式/无头）、GObject 类 |
| `src/font/` | 46 | 字体发现、加载、字形渲染、图集管理、精灵字体 |
| `src/termio/` | 9 | 终端 I/O 线程、PTY 管理、Shell 集成 |
| `src/os/` | 33 | 操作系统抽象（macOS/Linux/Windows/WASM） |
| `src/cli/` | 25 | 命令行界面 |
| `src/build/` | 35 | 构建系统逻辑 |
| `src/inspector/` | 13 | Dear ImGui 调试检查器 |
| `src/unicode/` | 8 | Unicode 属性表、字形断字 |
| `src/datastruct/` | 11 | 自定义数据结构 |
| `src/simd/` | 6 | SIMD 加速例程 |
| `src/lib/` | 9 | C ABI 库工具 |
| `src/crash/` | 4 | 崩溃报告（Sentry） |
| `src/benchmark/` | 12 | 性能基准 |
| `src/synthetic/` | 9 | 合成数据生成器 |
| `src/` (根) | 27 | 入口、Surface、App、PTY、全局状态等 |
| **总计** | **~480** | |

---

## 功能区块索引

| # | 功能区块 | 文档 | Ghostty 源码范围 | PureTerm 对应 crate |
|---|---------|------|-----------------|-------------------|
| 01 | VTE 解析引擎 | [01-vte-parser.md](01-vte-parser.md) | `terminal/Parser.zig`, `parse_table.zig`, `stream.zig`, `stream_terminal.zig`, `ansi.zig` | `pureterm-terminal` |
| 02 | 终端屏幕与网格 | [02-screen-grid.md](02-screen-grid.md) | `terminal/Screen.zig`, `page.zig`, `PageList.zig`, `Terminal.zig`, `point.zig`, `Tabstops.zig` | `pureterm-terminal` |
| 03 | SGR 与样式系统 | [03-sgr-style.md](03-sgr-style.md) | `terminal/sgr.zig`, `style.zig`, `color.zig`, `x11_color.zig`, `ref_counted_set.zig` | `pureterm-terminal` |
| 04 | 终端模式管理 | [04-modes.md](04-modes.md) | `terminal/modes.zig`, `charsets.zig`, `cursor.zig`, `focus.zig`, `mouse.zig` | `pureterm-terminal` |
| 05 | OSC/DCS/APC 序列处理 | [05-osc-dcs-apc.md](05-osc-dcs-apc.md) | `terminal/osc.zig`, `osc/parsers/*.zig`(14), `dcs.zig`, `apc.zig` | `pureterm-terminal` |
| 06 | Kitty 图形协议 | [06-kitty-graphics.md](06-kitty-graphics.md) | `terminal/kitty/graphics*.zig`(6) | `pureterm-terminal` |
| 07 | 输入系统 | [07-input-system.md](07-input-system.md) | `input/*.zig`(17) | 新 crate 或 `pureterm-core` |
| 08 | 渲染系统 | [08-renderer.md](08-renderer.md) | `renderer/*.zig`, `renderer/metal/*.zig`, `renderer/opengl/*.zig`(~34) | `pureterm-gui` |
| 09 | 字体系统 | [09-font-system.md](09-font-system.md) | `font/*.zig`(46) | `pureterm-gui` |
| 10 | 配置系统 | [10-config.md](10-config.md) | `config/*.zig`(19) | `pureterm-core` |
| 11 | 应用运行时与 Surface | [11-app-surface.md](11-app-surface.md) | `App.zig`, `Surface.zig`, `surface_mouse.zig`, `apprt/*.zig`(56) | `pureterm` + `pureterm-ui` |
| 12 | 终端 I/O 与 PTY | [12-termio-pty.md](12-termio-pty.md) | `termio/*.zig`(9), `pty.zig`, `Command.zig` | `pureterm-session` |
| 13 | 搜索系统 | [13-search.md](13-search.md) | `terminal/search/*.zig`(6), `StringMap.zig` | `pureterm-terminal` |
| 14 | 操作系统抽象 | [14-os-abstraction.md](14-os-abstraction.md) | `os/*.zig`(33) | 各 crate 的平台代码 |
| 15 | 数据结构与工具 | [15-datastruct-utils.md](15-datastruct-utils.md) | `datastruct/*.zig`(11), `fastmem.zig`, `math.zig`, `unicode/*.zig`(8), `simd/*.zig`(6) | 各 crate 共用 |
| 16 | Tmux 控制模式 | [16-tmux.md](16-tmux.md) | `terminal/tmux/*.zig`(4) | `pureterm-terminal` |
| 17 | Shell 集成 | [17-shell-integration.md](17-shell-integration.md) | `termio/shell_integration.zig`, `shell-integration/` | `pureterm-session` |
| 18 | 检查器与调试 | [18-inspector.md](18-inspector.md) | `inspector/*.zig`(13) | `pureterm-ui` |
| 19 | C API 与库导出 | [19-c-api.md](19-c-api.md) | `terminal/c/*.zig`(26), `lib/*.zig`(9), `lib_vt.zig` | 不需要 |
| 20 | 构建/CLI/崩溃/基准 | [20-build-cli-crash.md](20-build-cli-crash.md) | `build/*.zig`, `cli/*.zig`, `crash/*.zig`, `benchmark/*.zig`, `synthetic/*.zig` | `pureterm` (CLI) |

---

## 实现优先级建议

### P0 - 核心终端功能（必须先做）
1. **01 VTE 解析引擎** - 终端的基础，所有功能的前提
2. **02 终端屏幕与网格** - 数据模型
3. **03 SGR 与样式系统** - 文本渲染所需
4. **04 终端模式管理** - 模式切换逻辑
5. **05 OSC/DCS/APC 序列处理** - 完整终端兼容性
6. **12 终端 I/O 与 PTY** - 连接子进程

### P1 - 用户可见功能
7. **08 渲染系统** - GPU 渲染
8. **09 字体系统** - 字体加载和渲染
9. **07 输入系统** - 键盘/鼠标处理
10. **10 配置系统** - 用户配置
11. **13 搜索系统** - 搜索功能

### P2 - 高级功能
12. **06 Kitty 图形协议** - 图像显示
13. **11 应用运行时与 Surface** - 窗口管理
14. **14 操作系统抽象** - 跨平台
15. **15 数据结构与工具** - 性能优化
16. **17 Shell 集成** - Shell 集成

### P3 - 可选功能
17. **16 Tmux 控制模式** - Tmux 支持
18. **18 检查器与调试** - 调试工具
19. **20 构建/CLI** - 打包发布
20. **19 C API** - 不需要

---

## 架构对比

### Ghostty 架构（Zig）
```
main.zig → App.zig → Surface.zig
                         ├── termio/ (IO 线程)
                         │     ├── Termio.zig (状态管理)
                         │     ├── Thread.zig (写线程)
                         │     └── Exec.zig (PTY 后端)
                         ├── renderer/ (渲染线程)
                         │     ├── generic.zig (通用渲染逻辑)
                         │     ├── Metal.zig / OpenGL.zig (后端)
                         │     └── Thread.zig (渲染循环)
                         └── terminal/ (终端状态)
                               ├── Terminal.zig
                               ├── Screen.zig
                               ├── PageList.zig
                               └── Parser.zig
```

### PureTerm 架构（Rust）- 目标
```
main.rs → App → Surface
                  ├── pureterm-session (IO 线程)
                  │     ├── Session trait
                  │     └── PtySession (PTY 后端)
                  ├── pureterm-gui (渲染线程)
                  │     ├── Renderer (wgpu)
                  │     └── FontSystem
                  └── pureterm-terminal (终端状态)
                        ├── Terminal
                        ├── Screen / Grid
                        └── Parser (VTE)
```
