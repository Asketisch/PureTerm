# PureTerm

纯 Rust 编写的 GPU 加速终端模拟器。

## 功能特性

- **多标签终端** 支持嵌套分屏，Ghostty 风格 GPU 渲染标签栏（28pt 高度，DPI 感知，等分布局，左侧悬浮关闭按钮，"+" 新建按钮）
- **多种会话类型**：本地 Shell、SSH、串口、Telnet
- **GPU 加速渲染** 基于 wgpu，8 pass 管线（背景色→装饰→文字→光标→图片→后处理→调试覆盖）
- **跨平台**：macOS、Linux、Windows
- **主题支持** 自定义配色方案（28 种内置主题）
- **可配置快捷键**
- **插件系统** 支持扩展

## 快速开始

```bash
cargo build --release
./target/release/pureterm
```

## 架构设计

PureTerm 由 7 个 crate 组成的工作区：

| Crate | 说明 |
|-------|------|
| `pureterm` | 二进制入口，默认路由到 Alacritty runtime，保留 legacy UI |
| `pureterm-alacritty-runtime` | Alacritty 单窗口终端运行时 |
| `pureterm-core` | 配置、主题、快捷键、插件 |
| `pureterm-terminal` | VTE 解析器、屏幕网格（reflow + VecDeque） |
| `pureterm-session` | Session trait + Local/SSH/Serial/Telnet 实现 |
| `pureterm-ui` | Surface、标签页、分屏面板、终端面板 |
| `pureterm-gui` | wgpu 多 pass 渲染、字形图集、Sprite 光栅化 |

```
默认：用户输入/PTY 输出 → Alacritty Processor/Term → Display damage → OpenGL renderer
Legacy：TerminalPane::handle_key → Session::write → VteParser/Grid → ScreenSnapshot → wgpu
```

## 技术栈

- **渲染**：默认 Alacritty OpenGL display；legacy 路径为 wgpu 29（8 pass GPU 管线）+ egui 0.34
- **终端解析**：完整 VTE（CSI/ESC/OSC/DCS/APC/PM）
- **窗口/事件**：winit
- **字体**：fontdue + rustybuzz（HarfBuzz）+ fontdb（5 步回退链）
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
