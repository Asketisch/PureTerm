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

## SSH 会话后端

已实现 SSH 会话后端支持：

### 功能
- **ControlMaster socket 复用** — 连接池管理，多标签共享同一 SSH 连接
- **~/.ssh/config 集成** — 直接使用 SSH Host 别名连接
- **PureTerm SSH Profile** — 可在 alacritty.toml 中配置 SSH Profile
- **加密凭证存储** — AES-256-GCM + Argon2id 加密，跨平台存储
- **SshPty EventedPty 实现** — 集成到 Alacritty 事件循环

### SSH Profile 配置 (alacritty.toml)

```toml
[[ssh.profiles]]
name = "生产服务器"
host = "192.168.1.100"
user = "admin"
port = 22

[[ssh.profiles]]
name = "跳板机"
host = "bastion"
key_path = "~/.ssh/bastion_key"
```

### SSH Action 键盘绑定

```toml
[[keyboard.bindings]]
key = "F5"
mods = "Control|Shift"
action = { Ssh = "OpenSettings" }

[[keyboard.bindings]]
key = "F6"
mods = "Control|Shift"
action = { Ssh = "ListProfiles" }
```

可用的 SSH Action：
- `Ssh = "Connect"` — 打开 SSH 连接对话框
- `Ssh = "OpenSettings"` — 打开 SSH 设置界面
- `Ssh = "ListProfiles"` — 列出所有 SSH Profile

### 待实现
- SSH Tab UI（实际标签页创建）
- SSH 设置 UI 界面
- 命令面板集成
