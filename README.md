# PureTerm

纯 Rust 编写的 GPU 加速终端模拟器。

## 功能特性

- **多标签终端** 支持嵌套分屏
- **多种会话类型**：本地 Shell、SSH、串口、Telnet
- **GPU 加速渲染** 基于 wgpu
- **跨平台**：macOS、Linux、Windows
- **主题支持** 自定义配色方案
- **可配置快捷键**
- **插件系统** 支持扩展

## 快速开始

```bash
cargo build --release
./target/release/pureterm
```

## 架构设计

PureTerm 由 6 个 crate 组成的工作区：

| Crate | 说明 |
|-------|------|
| `pureterm` | 二进制入口，事件循环 |
| `pureterm-core` | 配置、主题、快捷键、插件 |
| `pureterm-terminal` | VTE 解析器、屏幕网格 |
| `pureterm-session` | Session trait + Local/SSH/Serial/Telnet 实现 |
| `pureterm-ui` | egui 组件（标签页、分屏面板） |
| `pureterm-gui` | wgpu 渲染、字形图集、着色器 |

```
用户输入 → TerminalPane::handle_key → Session::write
会话输出 → TerminalPane::drain_output → Terminal::write → VteParser → Grid
渲染 → ScreenSnapshot → TerminalRenderer::render → wgpu GPU
```

## 技术栈

- **渲染**：wgpu + egui
- **终端解析**：vte
- **窗口/事件**：winit
- **本地 Shell**：portable-pty
- **SSH**：russh
- **异步运行时**：tokio

## 项目结构

```
pureterm/
├── crates/
│   ├── pureterm/           # 二进制入口
│   ├── pureterm-core/      # 配置、主题、快捷键
│   ├── pureterm-terminal/  # 终端模拟
│   ├── pureterm-session/   # 会话管理
│   ├── pureterm-ui/        # UI 组件
│   └── pureterm-gui/      # GPU 渲染
├── build.sh
├── Cargo.toml
└── README.md
```

## 开源协议

MIT
