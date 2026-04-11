# Agent 使用指南 — 工作区总览

本文件是工作区级别的导航索引。各 crate 的详细 Agent 指南请查看对应的 AGENTS.md。

## Crate 导航

| Crate | 说明 | 详细指南 |
|-------|------|----------|
| [pureterm](pureterm/AGENTS.md) | 二进制入口点，事件循环和窗口管理 | [pureterm/AGENTS.md](pureterm/AGENTS.md) |
| [pureterm-core](pureterm-core/AGENTS.md) | 配置、主题、快捷键、配置文件、插件 | [pureterm-core/AGENTS.md](pureterm-core/AGENTS.md) |
| [pureterm-terminal](pureterm-terminal/AGENTS.md) | VTE 解析器、屏幕网格、终端模拟 | [pureterm-terminal/AGENTS.md](pureterm-terminal/AGENTS.md) |
| [pureterm-session](pureterm-session/AGENTS.md) | Session trait + Local/SSH/Serial/Telnet | [pureterm-session/AGENTS.md](pureterm-session/AGENTS.md) |
| [pureterm-ui](pureterm-ui/AGENTS.md) | egui 组件：标签页、分割面板、终端面板 | [pureterm-ui/AGENTS.md](pureterm-ui/AGENTS.md) |
| [pureterm-gui](pureterm-gui/AGENTS.md) | wgpu GPU 渲染、字形图集、着色器 | [pureterm-gui/AGENTS.md](pureterm-gui/AGENTS.md) |

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
用户按键 → TerminalPane::handle_key → Session::write
Session 输出 → broadcast channel → TerminalPane::drain_output → Terminal::write
Terminal::write → VteParser → VtePerform → Grid/Cell
渲染 → ScreenSnapshot → TerminalRenderer::render → wgpu GPU
```

## 版本兼容性注意

- `russh` 和 `russh-keys` 有严格的版本耦合 — 使用 workspace.dependencies 中的版本
- `vte 0.15` 是当前稳定版
- `wgpu 24` API 变更 — 优先使用 `Option::ok()` 而不是 `.map_err()` 选择适配器
- `portable-pty 0.8` — 用 `take_writer()` 而不是 `try_clone_writer()`
