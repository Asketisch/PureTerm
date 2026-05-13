# Agent 使用指南 — 项目根导航

本文件是项目根目录的导航入口。工作区级别的详细指南请查看 [crates/AGENTS.md](crates/AGENTS.md)。

## 层级导航

```
CLAUDE.md                    <- 项目指令（你正在阅读的入口）
`-- crates/AGENTS.md         <- 工作区总览（依赖图、数据流、Agent 选择）
    |-- pureterm/AGENTS.md   <- 二进制入口（CLI、事件循环、多窗格）
    |   `-- src/AGENTS.md    <- 源码详解（main、app、event_handler 等）
    |       `-- platform/AGENTS.md <- 平台特定功能（macOS）
    |-- pureterm-core/AGENTS.md <- 配置、主题、快捷键、AI、加密
    |   `-- src/AGENTS.md    <- 模块详解（22 个模块）
    |       `-- config/AGENTS.md <- 配置系统（10 个文件）
    |-- pureterm-terminal/AGENTS.md <- VTE 解析、屏幕网格
    |   `-- src/AGENTS.md    <- 模块详解（12 个模块）
    |       |-- parser/AGENTS.md  <- VTE 执行引擎
    |       |-- screen/AGENTS.md  <- 字符网格、样式、选择
    |       |-- input/AGENTS.md   <- 按键编码、鼠标、粘贴
    |       `-- tmux/AGENTS.md    <- Tmux 控制模式
    |-- pureterm-session/AGENTS.md <- Session trait + 5 种实现
    |   `-- src/AGENTS.md    <- 模块详解（19 个模块）
    |       |-- ssh/AGENTS.md     <- SSH 子系统
    |       |-- serial/AGENTS.md  <- 串口会话
    |       `-- telnet/AGENTS.md  <- Telnet 会话
    |-- pureterm-ui/AGENTS.md <- Surface、标签页、分割面板、命令面板
    |   `-- src/AGENTS.md    <- 模块详解（24 个模块）
    |       |-- settings/AGENTS.md  <- 设置面板
    |       |-- split/AGENTS.md     <- 分割面板布局
    |       |-- tabs/AGENTS.md      <- 标签页管理
    |       `-- terminal/AGENTS.md  <- 终端面板
    `-- pureterm-gui/AGENTS.md <- wgpu 多 pass 渲染、字体、ShaderToy
        `-- src/AGENTS.md    <- 模块详解（14 个模块）
            `-- render_pass/AGENTS.md <- 8 pass 渲染架构
```

## 快速定位

| 关注点 | 指南 |
|--------|------|
| 整体架构和数据流 | [crates/AGENTS.md](crates/AGENTS.md) |
| 主入口和事件循环 | [crates/pureterm/AGENTS.md](crates/pureterm/AGENTS.md) |
| 配置、主题、快捷键 | [crates/pureterm-core/AGENTS.md](crates/pureterm-core/AGENTS.md) |
| VTE 解析、屏幕网格 | [crates/pureterm-terminal/AGENTS.md](crates/pureterm-terminal/AGENTS.md) |
| 会话管理（SSH/Serial/Telnet/Tmux） | [crates/pureterm-session/AGENTS.md](crates/pureterm-session/AGENTS.md) |
| UI 组件层 | [crates/pureterm-ui/AGENTS.md](crates/pureterm-ui/AGENTS.md) |
| GPU 渲染 | [crates/pureterm-gui/AGENTS.md](crates/pureterm-gui/AGENTS.md) |

## 配置文件

- `Cargo.toml` — workspace 定义和依赖版本
- `CLAUDE.md` — 项目级别的 Agent 指令（本文件的上级入口）
