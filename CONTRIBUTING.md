# 贡献 PureTerm

## 开发环境

```bash
# 构建
cargo build

# 运行测试
cargo test --workspace

# 带调试日志运行
RUST_LOG=debug cargo run

# 检查格式
cargo fmt --all -- --check

# 运行 clippy
cargo clippy --workspace --all-targets -- -D warnings
```

## 架构

PureTerm 是一个 12 个 crate 的工作区:

- `pureterm` — 二进制入口(Iced 事件循环、UI、Vi 模式、状态栏、插件)
- `pureterm-alacritty-runtime` — 运行时封装(GPU 字体图集,精简版)
- `pureterm-settings` — 独立设置进程(Iced 设置界面,Cmd+, 打开)
- `pureterm-jsonrpc` — 传输无关的 JSON-RPC 2.0 类型(GUI IPC 与 daemon 共享)
- `pureterm-daemon` — 持久化 PTY 守护进程(puretermd,会话跨 GUI 重启存活)
- `pureterm-ssh` — 进程无关的 SSH PTY 启动(GUI 直连与 daemon 共用)
- `pureterm-codeindex` — Tree-sitter 代码切块 + SQLite 关键词搜索
- `pureterm-lsp` — JSON-RPC stdio LSP 客户端
- `pureterm-computer-use` — 桌面截图 + 输入能力
- `alacritty_terminal` — 终端内核、PTY、Grid(vendored 上游源码)
- `alacritty_config` — 配置反序列化抽象(vendored 上游源码)
- `alacritty_config_derive` — 配置 derive 宏(vendored 上游源码)

依赖关系与数据流详见 `crates/AGENTS.md`。

## Pull Request 流程

1. 从 `main` 创建特性分支
2. 以清晰的提交信息完成修改
3. 确保 `cargo fmt`、`cargo clippy`、`cargo test` 全部通过
4. 提交 PR 并在描述中说明变更内容

## 代码规范

- 遵循标准 Rust 格式(`cargo fmt`)
- 解决所有 clippy 警告
- 注释使用中文,与现有代码库风格一致
- UI 元素必须遵循圆角设计语言(见 CLAUDE.md)
