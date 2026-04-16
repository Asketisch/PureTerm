# Agent 使用指南 — 工作区总览

本文件是工作区级别的导航索引。各 crate 的详细 Agent 指南请查看对应的 AGENTS.md。

## Crate 导航

| Crate | 说明 | 详细指南 |
|-------|------|----------|
| [pureterm](pureterm/AGENTS.md) | 二进制入口点，事件循环、窗口管理、多窗格渲染、CLI | [pureterm/AGENTS.md](pureterm/AGENTS.md) |
| [pureterm-core](pureterm-core/AGENTS.md) | 配置、主题（28 种内置）、快捷键、i18n、AI、加密、触发器、C API | [pureterm-core/AGENTS.md](pureterm-core/AGENTS.md) |
| [pureterm-terminal](pureterm-terminal/AGENTS.md) | VTE 解析器、屏幕网格（reflow）、终端模拟、Tmux 集成 | [pureterm-terminal/AGENTS.md](pureterm-terminal/AGENTS.md) |
| [pureterm-session](pureterm-session/AGENTS.md) | Session trait + Local/SSH/Serial/Telnet + Shell 集成 | [pureterm-session/AGENTS.md](pureterm-session/AGENTS.md) |
| [pureterm-ui](pureterm-ui/AGENTS.md) | egui 组件：标签页、分割面板、终端面板、设置、Inspector | [pureterm-ui/AGENTS.md](pureterm-ui/AGENTS.md) |
| [pureterm-gui](pureterm-gui/AGENTS.md) | wgpu GPU 渲染、字形图集、着色器、背景图片、egui 集成 | [pureterm-gui/AGENTS.md](pureterm-gui/AGENTS.md) |

## 依赖关系

```
pureterm (二进制)
  → pureterm-core, pureterm-terminal, pureterm-session, pureterm-ui, pureterm-gui

pureterm-ui → pureterm-core, pureterm-terminal, pureterm-session
pureterm-gui → pureterm-core, pureterm-terminal
pureterm-session → pureterm-core
pureterm-terminal（独立）
pureterm-core（独立）
```

## Agent 类型选择

### Explore Agent
用于理解代码架构和跨 crate 关系：
- 探索 Session trait 在 Local/SSH/Serial/Telnet 中的实现
- 理解 VTE 解析器与 Terminal 的集成
- 调查 wgpu 渲染管线
- 在提出变更前寻找现有模式

### Plan Agent
用于涉及多个 crate 或需要架构决策的实现任务：
- 添加新的会话类型
- 修改终端模拟引擎
- 渲染管线的变更
- UI 层重构

### General-Purpose Agent
用于范围明确的有针对性修复：
- 修复 API 兼容性问题（如 russh 版本更新）
- 修复单个 crate 中的特定 bug
- 为现有功能添加测试
- 单个模块内的重构

## 核心数据流

```
用户按键 → TerminalPane::handle_key → encode_key (App Cursor/Alt/Ctrl/Shift) → Session::write
Session 输出 → broadcast channel → TerminalPane::drain_output → Terminal::write
Terminal::write → VteParser → VtePerform → Grid/Cell (wrapped flag for reflow)
渲染：Grid snapshot → build_vertices (bold→bright, inverse, hidden, bg image) → wgpu GPU
多窗格：每窗格 ViewportRect → 合并顶点 → draw_to_surface → 可选 egui 覆盖 → frame.present()
输入时自动滚回底部；Bell 事件通过 take_bell() 通知 UI
```

## 版本兼容性注意

- `russh 0.60` 和 `russh-keys` 有严格的版本耦合 — 使用 workspace.dependencies 中的版本
- `vte 0.15` 是当前稳定版
- `wgpu 29` — workspace 和 egui-wgpu 使用统一版本（旧版 wgpu 会导致运行时 ID 冲突 panic）
- `portable-pty 0.8` — 用 `take_writer()` 而不是 `try_clone_writer()`
- `egui 0.34` — `context.run()` 已废弃，应使用 `run_ui()`；`tessellate()` 需传入 `pixels_per_point`
- `image 0.25` — 用于背景图片解码（RGBA8）
