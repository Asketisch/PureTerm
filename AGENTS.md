# Agent 使用指南 — 项目根导航

本文件是项目根目录的导航入口。工作区级别的详细指南请查看 [crates/AGENTS.md](crates/AGENTS.md)。

## 快速定位

| 关注点 | 指南 |
|--------|------|
| 整体架构和数据流 | [crates/AGENTS.md](crates/AGENTS.md) |
| 主入口和事件循环 | [crates/pureterm/AGENTS.md](crates/pureterm/AGENTS.md) |
| 配置、主题、快捷键 | [crates/pureterm-core/AGENTS.md](crates/pureterm-core/AGENTS.md) |
| VTE 解析、屏幕网格 | [crates/pureterm-terminal/AGENTS.md](crates/pureterm-terminal/AGENTS.md) |
| 会话管理（SSH/Serial/Telnet） | [crates/pureterm-session/AGENTS.md](crates/pureterm-session/AGENTS.md) |
| UI 组件层 | [crates/pureterm-ui/AGENTS.md](crates/pureterm-ui/AGENTS.md) |
| GPU 渲染 | [crates/pureterm-gui/AGENTS.md](crates/pureterm-gui/AGENTS.md) |

## 配置文件

- `Cargo.toml` — workspace 定义和依赖版本
- `CLAUDE.md` — 项目级别的 Agent 指令（本文件的上级入口）
