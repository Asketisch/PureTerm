# Agent 使用指南 — 工作区总览

本文件是工作区级别的导航索引。

## Crate 导航

| Crate | 说明 | 详细指南 |
|-------|------|----------|
| [pureterm](pureterm/AGENTS.md) | 二进制入口点，调用 Alacritty 运行时 | [pureterm/AGENTS.md](pureterm/AGENTS.md) |
| [pureterm-alacritty-runtime](pureterm-alacritty-runtime/AGENTS.md) | Alacritty 运行时，暴露 `run_from_env()` | [pureterm-alacritty-runtime/AGENTS.md](pureterm-alacritty-runtime/AGENTS.md) |
| [alacritty_terminal](alacritty_terminal/AGENTS.md) | Alacritty 终端内核、PTY loop、Grid（vendored 上游源码） | [alacritty_terminal/AGENTS.md](alacritty_terminal/AGENTS.md) |
| [alacritty_config](alacritty_config/AGENTS.md) | Alacritty 配置反序列化抽象（vendored 上游源码） | [alacritty_config/AGENTS.md](alacritty_config/AGENTS.md) |
| [alacritty_config_derive](alacritty_config_derive/AGENTS.md) | Alacritty 配置 derive 宏（vendored 上游源码） | [alacritty_config_derive/AGENTS.md](alacritty_config_derive/AGENTS.md) |

## 依赖关系

```
pureterm (二进制)
  → pureterm-alacritty-runtime

pureterm-alacritty-runtime → alacritty_terminal, alacritty_config, alacritty_config_derive
```

## 已移除的功能

所有 PureTerm legacy 功能已移至 `archive/` 目录，详见 [docs/removed-legacy-features.md](../docs/removed-legacy-features.md)。
