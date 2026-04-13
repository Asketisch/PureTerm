# PureTerm 功能完善 - 并行开发计划

## Context

PureTerm 已实现核心架构（6 个 crate, ~4700 行 Rust），可运行本地 Shell 并 GPU 渲染。但 PLAN.md 规划的大量功能尚未实现。本计划按耦合度分析，将无依赖的功能分组为并行批次，使用 Subagent + worktree 并行开发。

## 已实现功能

| 模块 | 状态 |
|------|------|
| Workspace 结构 (6 crate) | 完成 |
| ConfigService (TOML) | 完成 |
| ThemeService (8 配色) | 完成 |
| HotkeyService (13 快捷键) | 完成 |
| Profile 数据模型 | 完成 (无 Provider 注册) |
| Plugin 骨架 | 完成 (空壳) |
| VTE Parser (CSI/SGR/OSC) | 完成 (ESC/hook/put/unhook 空) |
| Grid + Cell + Cursor + Mode | 完成 |
| Terminal 主结构 | 完成 |
| Session trait + 4 实现 | 完成 (SSH 无 known_hosts/Agent; Telnet 无 IAC) |
| Middleware 框架 | 完成 (无具体实现) |
| App + TabManager + SplitContainer + TerminalPane | 完成 |
| GPU Renderer (wgpu + fontdue + WGSL) | 完成 |
| 键盘输入编码 | 完成 |

## 依赖冲突关键文件

以下文件被多个功能争用，不能并行修改同一文件：
- `crates/pureterm-terminal/src/parser/perform.rs` — ESC/Sixel/OSC 8/Kitty/Marks
- `crates/pureterm/src/main.rs` — egui/托盘/全局热键/多窗口/Quake
- `crates/pureterm-ui/src/app.rs` — 设置/菜单/标题栏/命令面板
- `crates/pureterm-session/src/ssh/client.rs` — known_hosts/Agent/密码
- `crates/pureterm-session/src/ssh/mod.rs` — SFTP/端口转发/X11

---

## 并行执行批次

### Batch 0: 基础扩展 (2 个并行 Agent)

**A0-A: ESC Dispatch 序列** — worktree
- 文件: `crates/pureterm-terminal/src/parser/perform.rs`
- 实现: ESC 7 (save cursor), ESC 8 (restore cursor), ESC c (reset), ESC ( / ESC ) (字符集), ESC # (DEC alignment)
- 依赖: 无

**A0-B: Profile Provider 注册** — worktree
- 新文件: `crates/pureterm-session/src/local_provider.rs`, `ssh_provider.rs`, `serial_provider.rs`, `telnet_provider.rs`
- 修改: `crates/pureterm-session/src/lib.rs`, `crates/pureterm/src/main.rs` (注册)
- 实现: 为每种 SessionType 实现 `ProfileProvider` trait
- 依赖: 无

---

### Batch 1: Session 增强 (6 个并行 Agent)

全部是 `pureterm-session` 内的不同文件，互不冲突：

**A1-A: SSH Host Key Verification** — worktree
- 新文件: `crates/pureterm-session/src/ssh/known_hosts.rs`
- 修改: `crates/pureterm-session/src/ssh/client.rs` (check_server_key)
- 实现: 解析 `~/.ssh/known_hosts`, TOFU 策略

**A1-B: SSH Agent + Interactive Password Auth** — worktree
- 新文件: `crates/pureterm-session/src/ssh/agent.rs`
- 修改: `crates/pureterm-session/src/ssh/client.rs` (start() auth 分支)
- 注意: 与 A1-A 都改 client.rs，需协调不同函数区域，或串行执行

**A1-C: Telnet IAC 协议协商** — worktree (完全独立)
- 新文件: `crates/pureterm-session/src/telnet/iac.rs`
- 修改: `crates/pureterm-session/src/telnet/mod.rs`
- 实现: IAC DO/DONT/WILL/WONT, NAWS, terminal type

**A1-D: 具体中间件实现** — worktree (完全独立)
- 新文件: `crates/pureterm-session/src/login_script.rs`
- 实现: LoginScriptMiddleware, ZmodemMiddleware

**A1-E: Session Log** — worktree (完全独立)
- 新文件: `crates/pureterm-session/src/session_log.rs`
- 实现: 带时间戳的终端输出日志

**A1-F: Sync Input** — worktree (完全独立)
- 新文件: `crates/pureterm-session/src/sync_input.rs`
- 实现: 向多会话广播输入

> A1-A 和 A1-B 都改 client.rs，建议串行或仔细分工 (A1-A 改 check_server_key, A1-B 改 start() auth match)。A1-C/D/E/F 完全独立可并行。

---

### Batch 2: Core 数据扩展 (5 个并行 Agent)

全部是 `pureterm-core` 内的新文件，互不冲突：

**A2-A: Triggers 系统** — worktree
- 新文件: `crates/pureterm-core/src/trigger.rs`
- 修改: `crates/pureterm-core/src/lib.rs`
- 实现: 正则表达式 -> 动作的触发器规则

**A2-B: Multi-clipboard** — worktree
- 新文件: `crates/pureterm-core/src/clipboard.rs`
- 实现: 命名剪贴板缓冲区

**A2-C: Dir Frequency** — worktree
- 新文件: `crates/pureterm-core/src/dirfreq.rs`
- 实现: 目录访问频率统计

**A2-D: Remote Control Protocol** — worktree
- 新文件: `crates/pureterm-core/src/remote.rs`
- 实现: JSON IPC 协议 (Unix socket/Named pipe)

**A2-E: Config Encryption (VaultService)** — worktree
- 新文件: `crates/pureterm-core/src/vault.rs`
- 修改: `crates/pureterm-core/src/config.rs` (加密字段支持)
- 实现: AES-256-GCM 加密敏感配置
- 需添加 crate 依赖: `aes-gcm`

> 全部新增文件，只改 `lib.rs` 的 mod 声明，完全可并行。

---

### Batch 3: Terminal 数据扩展 (串行)

与 Batch 1/2 并行执行，但内部串行 (都改 perform.rs):

**A3-A: OSC 8 超链接**
- 修改: `perform.rs` (osc_dispatch OSC 8), `cell.rs` (加 hyperlink 字段)
- 新文件: `crates/pureterm-terminal/src/hyperlink.rs`

**A3-B: Marks 系统** (A3-A 完成后)
- 新文件: `crates/pureterm-terminal/src/mark.rs`
- 修改: `grid.rs` (行标记), `perform.rs` (OSC 133)

---

### Batch 4: SSH 高级功能 (与 Batch 1-3 并行)

**A4-A: SFTP** — worktree (新文件)
- 新文件: `crates/pureterm-session/src/ssh/sftp.rs`
- 修改: `crates/pureterm-session/src/ssh/mod.rs`

**A4-B: Port Forwarding** — (A4-A 后, 共享 ssh/mod.rs)
- 新文件: `crates/pureterm-session/src/ssh/forward.rs`

**A4-C: X11 Forwarding** — (A4-B 后)
- 新文件: `crates/pureterm-session/src/ssh/x11.rs`

**A4-D: SSH Multiplexer** — worktree (新文件, 独立)
- 新文件: `crates/pureterm-session/src/ssh/multiplexer.rs`

---

### Batch 5: egui 集成 (关键路径, 串行)

这是 UI 功能的前置条件，必须先完成：

**A5: egui Widget 集成**
- 修改: `pureterm-gui/src/renderer.rs`, `pureterm/src/main.rs`
- 用 egui-wgpu 替代裸 wgpu 渲染循环
- 创建 egui `TerminalWidget`
- 用 egui 实现 tab bar、split divider 等基础 UI

---

### Batch 6: UI 功能 (A5 后并行)

**A6-A: Settings UI** — worktree
- 替换 `pureterm-ui/src/settings/mod.rs` 占位
- 新文件: `appearance.rs`, `terminal.rs`, `profiles.rs`, `hotkeys.rs`

**A6-B: Context Menu** — worktree
- 新文件: `crates/pureterm-ui/src/context_menu.rs`

**A6-C: Command Palette** — worktree
- 新文件: `crates/pureterm-ui/src/command_palette.rs`

**A6-D: Quick Bar** — worktree (新文件, 完全独立)
- 新文件: `crates/pureterm-ui/src/quick_bar.rs`

**A6-E: Auto-Complete** — worktree (新文件, 完全独立)
- 新文件: `crates/pureterm-ui/src/autocomplete.rs`

**A6-F: AI Chat** — worktree (新文件, 完全独立)
- 新文件: `crates/pureterm-ui/src/ai_chat.rs`, `crates/pureterm-core/src/ai.rs`

**A6-G: Focus Mode** — worktree
- 新文件: `crates/pureterm-ui/src/focus_mode.rs`

**A6-H: Lock Screen** — worktree
- 新文件: `crates/pureterm-ui/src/lock_screen.rs`

**A6-I: Explorer Panel** — worktree (依赖 A4-A SFTP)
- 新文件: `crates/pureterm-ui/src/explorer.rs`

> A6-A~H 可并行，都是新文件。合并时统一在 app.rs 注册。

---

### Batch 7: 平台集成 (与 Batch 6 并行)

**A7-A: Notification Trait + 实现** — worktree (完全独立)
- 新文件: `crates/pureterm-core/src/platform.rs`, `crates/pureterm/src/notification.rs`

**A7-B: Auto Updater** — worktree (完全独立)
- 新文件: `crates/pureterm-core/src/updater.rs`, `crates/pureterm/src/update.rs`

**A7-C: Clipboard Integration** — worktree
- 新文件: `crates/pureterm/src/clipboard.rs`
- 修改: `pureterm-ui/src/terminal/pane.rs`

**A7-D: Tab Recovery** — worktree
- 修改: `pureterm-ui/src/tabs/manager.rs`

> A7-A/B 完全独立。A7-C/D 与 Batch 6 中的 pane.rs/app.rs 有潜在冲突，需协调。

---

### Batch 8: 平台高级 (串行, 都改 main.rs)

**A8-A: System Tray** → **A8-B: Global Hotkeys** → **A8-C: Quake Mode**
**A8-D: Single Instance** → **A8-E: Multiple Windows**

---

### Batch 9: 渲染增强 (A5 后)

**A9-A: Window Transparency/Blur** — worktree
**A9-B: Background Images** — worktree
**A9-C: Kitty Graphics Protocol** (依赖 Batch 3 A3-A 和 hook/put/unhook 框架)

---

### Batch 10: 终端高级 (串行, 优化阶段)

**A10-A: Instant Replay**
**A10-B: Multi-thread Architecture**
**A10-C: SIMD Optimization**
**A10-D: Memory Compression**
**A10-E: Online Search**

---

## 最大并行度汇总

| 批次 | 最大并行 Agent 数 | 主要 crate |
|------|------------------|-----------|
| Batch 0 | 2 | terminal + session |
| Batch 1 | 5~6 | session |
| Batch 2 | 5 | core |
| Batch 3 | 1 (串行) | terminal |
| Batch 4 | 2 (A4-D 独立 + A4-A 串行链) | session/ssh |
| Batch 5 | 1 (串行, 关键路径) | gui + binary |
| Batch 6 | 8~9 | ui |
| Batch 7 | 4 | core + binary |
| Batch 8 | 1 (串行) | binary |
| Batch 9 | 2 | gui |
| Batch 10 | 1 (串行) | terminal |

**Batch 0-4 可同时启动, 理论峰值并行度 15+ Agent**

---

## 验证方案

每批次完成后:
1. `cargo build --workspace` — 编译通过
2. `cargo clippy --workspace` — 无警告
3. 手动运行 `./target/debug/pureterm` — 本地 Shell 正常
4. 新模块添加 `#[cfg(test)]` 单元测试
5. 最终: 完整集成测试 (SSH 连接、串口、分屏、标签页)

## 建议执行顺序

第一批并行启动 (无耦合, 最大收益):
1. **A0-A** ESC dispatch (terminal)
2. **A0-B** Profile Provider (session)
3. **A1-C** Telnet IAC (session/telnet)
4. **A1-D** Middleware 实现 (session)
5. **A1-E** Session Log (session)
6. **A1-F** Sync Input (session)
7. **A2-A** Triggers (core)
8. **A2-B** Multi-clipboard (core)
9. **A2-C** Dir Frequency (core)
10. **A2-D** Remote Control (core)
11. **A2-E** Vault (core)
12. **A4-D** SSH Multiplexer (session/ssh, 新文件独立)

共 12 个 Agent 并行, 覆盖 3 个 crate, 零文件冲突。
