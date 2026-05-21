# pureterm-alacritty-runtime — Alacritty 单窗口运行时

第一阶段替换默认终端路径的运行时 crate。它将 vendored Alacritty 应用模块库化，暴露给 `pureterm` 二进制入口调用。

## 文件结构

```
src/
`-- lib.rs    # 运行时入口，复用 Alacritty Processor/WindowContext/Display
build.rs      # 生成 OpenGL bindings 与 VERSION 环境变量
```

## 关键依赖

- `crates/alacritty_terminal` — Alacritty 终端内核、PTY loop、Grid、Selection、Damage。
- `crates/alacritty_config` — Alacritty 配置反序列化。
- `src/alacritty/*` — 通过 `#[path]` 引入显示层、事件层、renderer 和窗口上下文。

## 公开 API

- `run_from_env()` — 使用当前进程参数启动 Alacritty 单窗口运行时。

## 迁移约束

- 默认终端体验应走 Alacritty `Term + Display` 路径，不再进入旧 `ScreenSnapshot -> pureterm-gui::TerminalRenderer` 热路径。
- 旧 PureTerm UI 保留 legacy 对照入口，后续按 `docs/alacritty-runtime-sacrificed-capabilities.md` 分阶段恢复能力。
- 修改 vendored Alacritty 代码时优先保持最小 patch，便于后续同步上游。
