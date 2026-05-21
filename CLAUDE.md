# PureTerm - 基于 Alacritty 的终端模拟器

基于 Alacritty 上游源码构建的 GPU 加速终端模拟器。

## 快速开始

```bash
cargo build --release
./target/release/pureterm
```

配置文件：标准 `alacritty.toml`（`~/.config/alacritty/alacritty.toml`）

## 架构

5 个 crate 的工作区：
- `pureterm` — 二进制入口（薄封装）
- `pureterm-alacritty-runtime` — Alacritty 运行时（GUI、事件循环、渲染）
- `alacritty_terminal` — 终端内核、PTY、Grid（vendored 上游源码）
- `alacritty_config` — 配置反序列化抽象（vendored 上游源码）
- `alacritty_config_derive` — 配置 derive 宏（vendored 上游源码）

## Agent 指南

- **项目导航**：[AGENTS.md](AGENTS.md)
- **工作区总览**：[crates/AGENTS.md](crates/AGENTS.md)
- **已移除的 legacy 功能**：[docs/removed-legacy-features.md](docs/removed-legacy-features.md)

## 维护规则

- vendored Alacritty 源码不做无关格式化或重构，便于同步上游
- 新功能应在 `pureterm-alacritty-runtime` 中以最小 patch 方式集成
- 修改 alacritty 源码时在修改处添加注释说明原因
