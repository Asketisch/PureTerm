# PureTerm - 基于 Alacritty 的终端模拟器

基于 Alacritty 上游源码构建的 GPU 加速终端模拟器。

## 快速开始

```bash
cargo build --release
./target/release/pureterm
```

配置文件：`~/.config/pureterm/alacritty.toml`（兼容回退 `~/.config/alacritty/alacritty.toml`）

## 架构

6 个 crate 的工作区：
- `pureterm` — 二进制入口（薄封装，基于 Iced 0.14 daemon）
- `pureterm-alacritty-runtime` — Alacritty 内核 + GPU 字体图集（精简版，2026-06-08 起仅含 `renderer::glyph_atlas`）
- `pureterm-settings` — Iced 0.14 设置 UI（独立进程，Cmd+, 打开）
- `alacritty_terminal` — 终端内核、PTY、Grid（vendored 上游源码）
- `alacritty_config` — 配置反序列化抽象（vendored 上游源码）
- `alacritty_config_derive` — 配置 derive 宏（vendored 上游源码）

## Agent 指南

- **项目导航**：[AGENTS.md](AGENTS.md)
- **工作区总览**：[crates/AGENTS.md](crates/AGENTS.md)
- **已移除的 legacy 功能**：[docs/removed-legacy-features.md](docs/removed-legacy-features.md)

## 维护规则

- vendored Alacritty 源码不做无关格式化或重构，便于同步上游
- 新功能应在主路径（`crates/pureterm/src/`）实现；`pureterm-alacritty-runtime` 已精简为只承载 `glyph_atlas`，不再扩展
- 修改 alacritty 源码时在修改处添加注释说明原因

## 已归档模块（2026-06-08）

`pureterm-alacritty-runtime` 的 Alacritty Display 渲染管线（8-pass wgpu + 独立 render thread + HarfBuzz shaper + 全套配置 + IPC + SSH 后端）已整体归档至 [`archive/pureterm-display-path/`](archive/pureterm-display-path/AGENTS.md)。详见 [`docs/removed-legacy-features.md`](docs/removed-legacy-features.md)。

- 触发原因：`docs/performance-issues-todo.md` Phase B (B1–B8) 误把 Display 路径当主路径 GPU 优化目标，改完用户感知为 0
- 主路径 GPU 优化已重规划为 P-B' 4 项，只针对 `crates/pureterm/src/terminal_shader.rs`：
  - P-B1' 持久 atlas texture + 局部上传
  - P-B5' 颜色 atlas Nearest 采样 + 双 pipeline
  - P-B7' Widget 边界内 damage rect
  - P-B8' 复用 scratch buffer

## Settings UI

基于 Iced 0.14 的图形化设置界面，作为独立进程运行：

- **快捷键**：`Cmd+,`（macOS）打开设置窗口
- **架构**：独立进程 `pureterm-settings`，通过配置文件通信（写文件 → ConfigMonitor 自动热重载）
- **主题**：Halloy 风格暖色调暗色（Ferra 配色方向）
- **页面**：General / Appearance / Colors / Terminal / SSH Profiles / Keyboard Shortcuts
- **配置 I/O**：使用 `toml_edit::DocumentMut` 格式保留读写，`NamedTempFile::persist()` 原子写入

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
- GAP-5.1 持久 PTY 守护进程(`puretermd` 后台进程)
- GAP-5.6 完整 reconnect/disconnect 状态机
- SSH agent forwarding(GAP-5.4)
- pureterm:// URL scheme(GAP-5.5)

### 已实现(2026-06-08 闭环)
- ✅ SSH Tab UI(GAP-5.7):`crates/pureterm/src/ssh/` + iced UI 串通;`⌘⇧S` 弹 SSH 对话框;异步 ssh spawn;PtyEventLoop 集成;`🔒 SSH: <name>` 标题;Tab 关闭时 `graceful_close` 发 `exit\r` + 500ms 等待 + SIGHUP 兜底
- ✅ SSH 主 UI 设置 UI(GAP-5.8):右侧栏 `SshSettingsPanelState`;Profile 列表 + 编辑 + 保存到 toml;`CredentialStore` 主密码解锁/设置端到端(AES-256-GCM + Argon2id,Zeroizing 包裹 master_key)
- ✅ 命令面板(GAP-5.9):`⌘⇧P` 触发;`nucleo-matcher` 模糊搜索;`Mode::Actions` ↔ `Mode::Profiles` 切换,SSH Profile 一跳到底

## graphify

本项目在 graphify-out/ 目录下有一个知识图谱，包含核心节点、社区结构和跨文件关系。

规则：
- 回答代码库相关问题时，先运行 `graphify query "<问题>"`（当 graphify-out/graph.json 存在时）。用 `graphify path "<A>" "<B>"` 查找两个概念之间的关系，用 `graphify explain "<概念>"` 深入了解某个概念。这些命令返回一个局部子图，通常比 GRAPH_REPORT.md 或原始 grep 输出小得多。
- 如果 graphify-out/wiki/index.md 存在，用它做宽泛导航，而不是直接浏览源码。
- 只有在架构概览或 query/path/explain 无法提供足够上下文时，才阅读 graphify-out/GRAPH_REPORT.md。
- 修改代码后，运行 `graphify update .` 保持图谱最新（仅 AST，无需 API 消耗）。
