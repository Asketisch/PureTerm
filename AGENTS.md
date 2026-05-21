# Agent 使用指南 — 项目根导航

PureTerm 是基于 Alacritty 的终端模拟器。配置通过标准 `alacritty.toml` 进行。

## 层级导航

```
CLAUDE.md                                       <- 项目指令
`-- crates/AGENTS.md                            <- 工作区总览
    |-- pureterm/AGENTS.md                      <- 二进制入口
    |-- pureterm-alacritty-runtime/AGENTS.md    <- Alacritty 运行时
    |-- alacritty_terminal/AGENTS.md            <- 终端内核（vendored）
    |-- alacritty_config/AGENTS.md              <- 配置抽象（vendored）
    `-- alacritty_config_derive/AGENTS.md       <- 配置 derive 宏（vendored）
```

## 快速定位

| 关注点 | 指南 |
|--------|------|
| 整体架构 | [crates/AGENTS.md](crates/AGENTS.md) |
| 二进制入口 | [crates/pureterm/AGENTS.md](crates/pureterm/AGENTS.md) |
| Alacritty 运行时 | [crates/pureterm-alacritty-runtime/AGENTS.md](crates/pureterm-alacritty-runtime/AGENTS.md) |
| 终端内核 | [crates/alacritty_terminal/AGENTS.md](crates/alacritty_terminal/AGENTS.md) |
| 已移除的 legacy 功能 | [docs/removed-legacy-features.md](docs/removed-legacy-features.md) |

## 配置

- `alacritty.toml` — 标准 Alacritty 配置文件（`~/.config/alacritty/alacritty.toml`）
- `Cargo.toml` — workspace 定义
- `CLAUDE.md` — 项目级别的 Agent 指令
