# PureTerm - 纯 Rust 终端模拟器

纯 Rust 编写的 GPU 加速终端模拟器。

## 快速开始

```bash
cargo build --release
./target/release/pureterm
```

## 架构

6 个 crate 的工作区：`pureterm` (二进制入口) + `pureterm-core` + `pureterm-terminal` + `pureterm-session` + `pureterm-ui` + `pureterm-gui`

## Agent 指南（分级索引）

- **工作区总览**：[crates/AGENTS.md](crates/AGENTS.md) — 依赖关系、数据流、Agent 类型选择
- **各 Crate 详细指南**：
  - [pureterm/AGENTS.md](crates/pureterm/AGENTS.md) — 二进制入口，事件循环
  - [pureterm-core/AGENTS.md](crates/pureterm-core/AGENTS.md) — 配置、主题、快捷键、插件
  - [pureterm-terminal/AGENTS.md](crates/pureterm-terminal/AGENTS.md) — VTE 解析、屏幕网格
  - [pureterm-session/AGENTS.md](crates/pureterm-session/AGENTS.md) — Session trait + 4 种实现
  - [pureterm-ui/AGENTS.md](crates/pureterm-ui/AGENTS.md) — egui 组件层
  - [pureterm-gui/AGENTS.md](crates/pureterm-gui/AGENTS.md) — wgpu GPU 渲染
