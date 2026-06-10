# PureTerm 路线图与待办清单

> 单一来源的 PureTerm 功能补齐路线图。按 **能力类别 × 优先级二级** 排序。
> 本文件 2026-06-10 由旧 `docs/CMUX-GAP-TODO.md` + 旧 `docs/TODOLIST.md` 合并而成,前两者已删除,git history 可追溯。
> 已归档特性(触发器系统 / OSC 133 / HarfBuzz / 字体回退 / 背景图像 / 8 pass 渲染管线)不在本文件追踪,改在 [`README.md` §已归档特性](../README.md#已归档特性2026-06-08-起) 中说明。

## 0. 元信息

| 字段 | 值 |
|---|---|
| 文档版本 | v1.0(2026-06-10 合并版) |
| 维护者 | @pureterm-core |
| 对标项目 | [cmux-main](https://github.com/manaflow-ai/cmux)(v0.64.x)、Ghostty、iTerm2、Alacritty |
| 主线 | branch `iced-cover` |
| 关联文档 | [README.md](../README.md) / [CLAUDE.md](../CLAUDE.md) / [removed-legacy-features.md](removed-legacy-features.md) |
| 更新约定 | 完成 P0/P1 → 同步 §1 仪表板 + §11 看板;季度末 → 同步 P0 焦点 |

**状态图例**

| 图例 | 含义 |
|---|---|
| ✅ | 已实现并验证 |
| 🚧 | 部分实现,关键子能力缺失 |
| ❌ | 未实现 |
| ⛔ | 明确不做 |

**优先级图例**

| 等级 | 定义 | 决策点 |
|---|---|---|
| **P0** 关键 | 缺失导致核心场景不可用;CLAUDE.md 显式列为"待实现" | 本季度闭环 |
| **P1** 高 | 影响日常使用舒适度;主流终端用户期待 | 下季度排期 |
| **P2** 中 | 锦上添花;cmux 独占但非主流 | 评估后做 |
| **P3** 低 | 平台战略 / 商业 / 特定用户群 | 未来评估 |

**能力类别**

| ID | 类别 |
|---|---|
| A | 核心架构(抽象层 / 分屏 / IPC / 守护进程) |
| B | UI/UX(侧边栏 / 命令面板 / 状态栏 / 快捷键 / 搜索) |
| C | 终端交互(复制模式 / hover / 搜索 / 协议) |
| D | SSH(持久会话 / 代理 / URL / 状态机) |
| E | 集成与可编程(Socket API / CLI / 自定义命令 / 事件流) |
| F | AI Agent(hooks / adapter / hibernation / 审批) |
| G | 平台(自动更新 / 移动端 / 云) |

---

## 1. 总览仪表板

### 1.1 数字摘要(不计已归档项)

| 类别 | ✅ | 🚧 | ❌ | ⛔ | 小计 |
|---|---|---|---|---|---|
| A. 核心架构 | 0 | 2 | 3 | 0 | 5 |
| B. UI/UX | 1 | 0 | 6 | 0 | 7 |
| C. 终端交互 | 0 | 0 | 7 | 0 | 7 |
| D. SSH | 3 | 0 | 6 | 0 | 9 |
| E. 集成与可编程 | 0 | 0 | 8 | 0 | 8 |
| F. AI Agent | 0 | 0 | 8 | 0 | 8 |
| G. 平台 | 1 | 0 | 1 | 3 | 5 |
| **总计** | **5** | **2** | **39** | **3** | **49** |

整体进度:✅ 5/49 ≈ 10%(含 🚧 折半计为 12%)

### 1.2 本季度焦点(P0 优先)

1. **GAP-2.2** 嵌套分屏补齐(等分 / 交换 / 缩放 / 焦点切换)
2. **GAP-2.3** Socket API v2 协议设计(method 分类骨架)
3. **GAP-5.1** 持久 PTY 守护进程 `puretermd`(关 GUI 不丢会话)
4. **GAP-5.6** SSH reconnect/disconnect 状态机
5. **GAP-3.7** 快捷键 6 套模板 + live reload(tmux/Vim/iTerm/agent-triage/vscode)

---

## 2. 已实现(✅)

详细说明见 [`README.md` §已实现特性](../README.md#已实现特性)。本节仅列闭环条目作完成参考:

| GAP | 标题 | 类别 | 闭环日期 | 实现位置 |
|---|---|---|---|---|
| GAP-3.3 | 命令面板 ⌘⇧P | B | 2026-06-08 | `crates/pureterm/src/iced_app.rs`(nucleo-matcher + Actions/Profiles 双模式) |
| GAP-5.7 | SSH Tab UI 串通 | D | 2026-06-08 | `crates/pureterm/src/ssh/` + iced UI 串通(`⌘⇧S`、异步 spawn、graceful_close) |
| GAP-5.8 | SSH 主 UI 设置 UI | D | 2026-06-08 | 右侧栏 `SshSettingsPanelState` + `CredentialStore` 主密码端到端 |
| GAP-5.9 | SSH 命令面板集成 | D | 2026-06-08 | `Mode::Profiles` 一跳到底 |
| GAP-8.5 | 跨平台优势(反向能力) | G | — | macOS / Linux / Windows 三平台持续维护 |

---

## 3. 类别 A:核心架构

### A.P0

#### GAP-2.2 嵌套水平+垂直分屏(bonsplit) {#gap-2-2}
- **状态**: 🚧
- **优先级**: P0
- **PureTerm 现状**: `crates/pureterm/src/split_tree.rs`(23.5 KB)实现 `SplitNode { Leaf | Split { direction, ratio, first, second } }`,水平/垂直递归嵌套 + 拖拽,`MIN_RATIO=0.1 / MAX_RATIO=0.9`
- **目标**:
  - Equalize Splits(`Cmd+Shift+E` 一键等分)
  - Swap Panes(拖拽交换)
  - Zoom Pane(`Cmd+Shift+Enter` 临时最大化)
  - Focus Next/Prev Pane(`Cmd+Alt+方向键`)
  - Move Pane to New Tab/Window
- **依赖**: GAP-2.1(Workspace 抽象)— 可并行
- **风险点**: 焦点切换算法在深嵌套层级的性能
- **DoD**: 嵌套 3 层分屏,`Cmd+Shift+E` 等分,`Cmd+Alt+方向键` 焦点移动到正确 Pane
- **对标**: cmux `vendor/bonsplit`

#### GAP-2.3 Socket API 协议 + 端口模型 {#gap-2-3}
- **状态**: 🚧
- **优先级**: P0
- **PureTerm 现状**: `pureterm msg` 子命令已实现 Unix socket,仅 3 个 method(`CreateWindow / Config / GetConfig`),`IoListener::spawn` 暴露
- **目标**: Socket API v2 协议层,method 命名空间:`window.* / workspace.* / pane.* / surface.* / notification.* / feed.* / system.* / debug.*`,首版 30+ method;JSON-RPC 2.0 风格;`tokio::net::UnixListener` + `SocketRouter` HashMap
- **依赖**: GAP-2.1(抽象层) — 可并行启动协议骨架
- **风险点**: 协议稳定性的向后兼容设计
- **DoD**: 至少 30 个 method 在 `pureterm-cli` 中可调用,与 GUI 行为一致
- **对标**: cmux `Packages/SocketControl/`(283+ v2 method)

#### GAP-2.5 多 UI 路径统一治理 {#gap-2-5}
- **状态**: 🚧(实际已基本完成,仅角色定位需明确)
- **优先级**: P0
- **PureTerm 现状**: 默认二进制 `pureterm/src/main.rs` 走 Iced 0.14;`pureterm-alacritty-runtime` 已 2026-06-08 精简为只承载 `glyph_atlas`,不再扩展
- **目标**: 在 README / AGENTS.md 中明确"主路径 = Iced,alacritty-runtime = 字体图集库"。决策点:是否归档整个 `pureterm-alacritty-runtime` crate,将 `glyph_atlas` 直接合入 `pureterm`
- **依赖**: 无
- **风险点**: 归档剩余模块会破坏 vendored Alacritty 引用链
- **DoD**: AGENTS.md 与 CLAUDE.md 描述一致,新贡献者无歧义
- **对标**: cmux 单一 AppKit/SwiftUI 路径,无此问题

### A.P1

#### GAP-2.1 Surface/Pane/Workspace/Window 抽象层 {#gap-2-1}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: iced 路径有 `Tab { id, title, panes, split_tree, active_pane }`,仅 Pane + Tab 两层
- **目标**: 引入 4 层抽象 — Surface(单 PTY 实例) / Pane(容器) / Workspace(命名工作区) / Window(顶级窗口);持久化 `workspaces.json` 到 `dirs::config_dir() / pureterm / workspaces.json`
- **依赖**: 无
- **风险点**: 现有事件循环重写风险高,需渐进式迁移
- **DoD**: 用户可创建 2 个 Workspace,各含 1 个 Tab,每个 Tab 1 个 Pane;Workspace 切换不丢 Surface 状态
- **对标**: cmux `Sources/TerminalController.swift`(21540 行)

#### GAP-2.4 远程守护进程 `puretermd` 等价物 {#gap-2-4}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: 无独立守护进程;SSH PTY 由 `crates/pureterm/src/ssh/pty.rs` 在主进程内管理,会话绑死在窗口
- **目标**: 独立 `puretermd` Rust binary(`tokio::main()`),持久 PTY 会话(本地 + SSH),Session Manager 集中管理;接受 Socket API v2 调用;启动方式 `launchd`(macOS) / `systemd --user`(Linux) / 服务(Windows)
- **依赖**: GAP-2.3(Socket API)
- **风险点**: 启动顺序、崩溃恢复、版本兼容
- **DoD**: 启动 `puretermd`,GUI 关闭后再启动,之前的 session 可恢复
- **对标**: cmux `daemon/remote/cmuxd-remote`

### A.P3

#### GAP-2.6 iOS 伴侣 + Cloud VM {#gap-2-6}
- **状态**: ❌ → 见 §10 不做(平台战略外)

---

## 4. 类别 B:UI/UX

### B.P0

#### GAP-3.7 完整快捷键自定义 + 模板 {#gap-3-7}
- **状态**: 🚧
- **优先级**: P0
- **PureTerm 现状**: `bindings.rs::enum Action`(50+ 变体)支持完整自定义 TOML 配置;**无预设模板**,有 6 处 `unimplemented!()`(`input/mod.rs` L1268-1309)
- **目标**:
  - 6 套预设模板 — `resources/presets/{tmux,vim,iterm,agent-triage,vscode,custom}.toml`
  - 单键 + chord 完整支持(扩展 `Binding { key, mods, action, mode }` 加 `chord: Option<BindingKey>`)
  - live reload(`Cmd+Shift+R` 或 `config_watcher.rs` 扩展 `KeymapChanged`)
  - CLI 子命令:`pureterm shortcuts list/preset/set/reset`
- **依赖**: 无
- **风险点**: chord 解析复杂度
- **DoD**: `pureterm shortcuts preset vim` 切换后,`j` `k` `h` `l` 在 normal mode 可移动光标
- **对标**: cmux `Sources/CmuxConfig.swift::shortcuts.bindings`

### B.P1

#### GAP-3.1 双侧边栏 {#gap-3-1}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: 无主 UI 侧边栏;`pureterm-settings/src/widgets/sidebar.rs` 是设置 UI 的内部导航,不通用
- **目标**: 可隐藏双侧边栏 — 左侧 Workspace + Git 分支(`git2` crate 30s 轮询);右侧多 mode(files / find / sessions / feed);宽度可拖拽(默认 240px,范围 120-480)
- **依赖**: GAP-2.1(Workspace 抽象)
- **风险点**: 拖拽宽度的 layout reflow 性能
- **DoD**: `Cmd+B` 显示左侧栏,显示工作目录 git 状态 + Workspace 列表
- **对标**: cmux `Sources/Sidebar/`

#### GAP-3.4 状态栏 {#gap-3-4}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: 只有标签栏,无状态栏
- **目标**:
  - 顶部 title bar shortcut hint pills(当前 mode 快捷键提示)
  - 侧边栏 footer update pill
  - 菜单栏 extra badge(macOS `NSStatusItem` via `objc2`)
  - 底部状态栏(`Container::new().height(Length::Fixed(24.0))`):命令 / Git 分支 / CPU 内存 / SSH 状态
- **依赖**: 无
- **风险点**: 高度计算(挤压终端空间)
- **DoD**: 底部状态栏显示命令提示 + Git 分支;macOS 显示菜单栏图标
- **对标**: cmux `Sources/StatusBar/` + `Sources/TitleBar/`

#### GAP-3.5 内置 Browser 面板(WebView) {#gap-3-5}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: 无 WebView 集成
- **目标**: Pane 中可嵌入 WebView — `⌘⇧L` split 打开 / `⌘L` 聚焦地址栏 / `⌘[ ]` 导航;支持 SOCKS5/HTTP CONNECT proxy(绑定 GAP-5.2);跨平台用 `wry` crate(`tao` 生态)
- **依赖**: GAP-2.2(分屏)
- **风险点**: 跨平台差异、内存占用
- **DoD**: Pane 中打开 `https://example.com`,可渲染和滚动
- **对标**: cmux `Sources/BrowserWindowPortal.swift`(173 KB)

#### GAP-3.8 搜索体系 {#gap-3-8}
- **状态**: 🚧
- **优先级**: P1
- **PureTerm 现状**: `Action::SearchForward / SearchBackward` 实现基础正则搜索,**无目录搜索、无全局搜索**
- **目标**:
  - `⌘F` 当前 surface 内搜索(已有,补 iced 浮层 UI)
  - `⌘⇧F` 在工作目录内(`ripgrep` 子进程 `tokio::process::Command`)
  - 全局搜索(menubar):遍历所有 active surface
  - 正则 / case sensitive / whole word / 结果高亮跳转
- **依赖**: 无
- **风险点**: 大文件/大目录搜索性能
- **DoD**: `⌘F` 高亮匹配;`⌘⇧F` 工作目录搜文本,结果显示在侧边栏
- **对标**: cmux `Sources/Search/` + iTerm2 ⌘F/⌘⇧F

### B.P2

#### GAP-3.2 可编程 Sidebars(DSL 解释器) {#gap-3-2}
- **状态**: ❌
- **优先级**: P2
- **PureTerm 现状**: 无任何"用户可编程 UI"机制
- **目标**: 用 `rhai` crate(中等投入)或 `wasmtime`(灵活,投入大);sidebar 文件声明 触发器 + 渲染回调,DSL 注册到事件总线;沙箱(资源限制 + 权限隔离)
- **依赖**: GAP-3.1(侧边栏框架) + GAP-6.7(事件流)
- **风险点**: DSL 沙箱安全
- **DoD**: 写一个 `~/.config/pureterm/sidebars/show-git.rhai`,实时显示当前 git 状态
- **对标**: cmux `Sources/Sidebar/Interpreter/`(Swift 解释器,vibe-coded)

### B.P3

#### GAP-3.6 macOS Glass 效果 {#gap-3-6}
- **状态**: ❌
- **优先级**: P3
- **PureTerm 现状**: `objc2-app-kit` 启用 `NSVisualEffectView` 依赖,**无调用代码**
- **目标**: macOS 26 用 `NSGlassEffectView`,macOS 13-15 回退 `NSVisualEffectView`;窗口 `setOpaque(false)` + 主背景 alpha=0
- **依赖**: 无
- **风险点**: 仅 macOS
- **DoD**: macOS 26 启用 Glass,窗口显示半透明玻璃效果
- **对标**: cmux `Sources/GlassEffectView.swift`

---

## 5. 类别 C:终端交互

### C.P1

#### GAP-4.1 Vim 复制模式 {#gap-4-1}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: Alacritty 有 `Vi` 模式 + `Action::Vi(ViAction)`,但**不是完整 copy mode**(不能从光标开始选区)
- **目标**: 独立 Copy Mode — `Enter / Alt+Enter` 触发,`jkhl` 移动,`v` visual,`V` line,`Ctrl+v` block,`y` 复制,`/` 搜索,`Esc` 退出;复用 Alacritty `vi_mode` 模块(已 vendored);新增 `CopyMode { mode, anchor, cursor }` 状态机
- **依赖**: 无
- **风险点**: 与现有 Vi 模式共存
- **DoD**: `Enter` 进入 copy mode,`jk` 移动,`v` 选词,`y` 复制,`Esc` 退出
- **对标**: cmux `Sources/CopyMode/` + iTerm2 `iTermCopyModeHandler.m`(24KB)

#### GAP-4.2 智能选择 hover preview {#gap-4-2}
- **状态**: 🚧
- **优先级**: P1
- **PureTerm 现状**: `alacritty_terminal/src/smart_selection.rs` 9 规则智能选择已实现(URL / SSH URL / 邮箱 / 引号 / 路径 / C++ namespace / dotted include / Git SHA / IPv4),**无 hover preview**
- **目标**: 鼠标悬停 URL/路径 显示 tooltip — 200ms 延迟,iced `Tooltip` widget;URL preview(`reqwest` 抓 HTML title + favicon),路径 preview(`std::fs::metadata` + `ignore` crate 列目录);右键选择后弹出动作菜单(打开 / 复制 / 编辑器打开)
- **依赖**: GAP-3.1(侧边栏渲染基础)
- **风险点**: hover 性能(高频触发)
- **DoD**: 鼠标悬停 URL 1 秒,显示 tooltip 含页面标题
- **对标**: cmux `Sources/SemanticHistory.swift` + iTerm2 `iTermSemanticHistoryController.m`(45KB)

#### GAP-4.3 Tail-Find 自动滚动搜索 {#gap-4-3}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: 基础搜索已有,无 tail-find
- **目标**: 搜索匹配高亮 + 自动跟随;手动滚到底 → 取消跟随;`Ctrl+Shift+F` 进入 / `F3` 下一个;Alacritty `search` 模块加 `SearchState::follow_tail: bool` 字段
- **依赖**: 无
- **风险点**: 输出速率高时 CPU 占用
- **DoD**: `grep error` tail-find,持续输出含 "error" 时光标跟随;手动滚到底 `F3` 恢复
- **对标**: cmux `Sources/Search/TailFind.swift` + iTerm2 `TailFindController.swift`

### C.P2

#### GAP-1.2 DECSCL 设备控制 {#gap-1-2}
- **状态**: ❌
- **优先级**: P2
- **PureTerm 现状**: Alacritty 上游未实现
- **目标**: 实现 DECSCL(Set Conformance Level)CSI 序列处理
- **DoD**: VT100/VT200/VT300/VT400 conformance level 切换正常
- **对标**: Ghostty `src/terminal/modes.zig`

#### GAP-1.3 标题栈(CSI 22/23t) {#gap-1-3}
- **状态**: ❌
- **优先级**: P2
- **目标**: 实现 CSI 22t(push title)/ 23t(pop title)
- **DoD**: `echo -e "\e[22;0t" && echo -e "\e[23;0t"` 标题栈正常 push/pop
- **对标**: Ghostty `src/terminal/Terminal.zig` 22t handler

#### GAP-1.4 BiDi 双向文本 {#gap-1-4}
- **状态**: ❌
- **优先级**: P2
- **PureTerm 现状**: 已知差距;archive 中曾有 BiDi 支持(`archive/pureterm-terminal/src/screen/`)
- **目标**: 阿拉伯文/希伯来文 RTL 渲染;`unicode-bidi` crate;BiDi 重排序与 HarfBuzz 整形(如未来回归)联动
- **依赖**: 无强依赖,但与字体回退一同设计更好
- **风险点**: 与 Vi mode 列计算冲突
- **DoD**: 输入 RTL 字符串显示正确
- **对标**: Ghostty `src/unicode/`

### C.P3

#### GAP-4.6 Touch Bar / VoiceOver {#gap-4-6}
- **状态**: ❌
- **优先级**: P3
- **PureTerm 现状**: VoiceOver 不完整,无 Touch Bar
- **目标**: VoiceOver 所有 UI 元素可读(iced `accessibility` + `widget::accessible()`);Touch Bar(macOS `NSTouchBar` via `objc2`,Apple 已弱化优先级)
- **依赖**: 无
- **风险点**: VoiceOver 测试难
- **DoD**: VoiceOver 开启后,Tab 切换、命令面板弹出都有语音提示
- **对标**: Ghostty `Surface.zig` 完全 VoiceOver,iTerm2 `Accessibility/`

---

## 6. 类别 D:SSH

### D.P0

#### GAP-5.1 持久 PTY 守护进程 {#gap-5-1}
- **状态**: ❌
- **优先级**: P0
- **PureTerm 现状**: SSH PTY 由 `crates/pureterm/src/ssh/pty.rs` 在主进程内管理,**关 GUI 会话即死**
- **目标**: 引入 `puretermd-ssh` 子进程(可与 GAP-2.4 合并),接收 SSH 连接 + 分配 slot 目录(`~/.local/share/pureterm/daemon/<slot>/`);持久 PTY 写入 slot;Socket API v2 `ssh.attach/detach/input/output`;考虑迁移到 `russh` crate(替换当前 `ssh` 子进程方案)
- **依赖**: GAP-2.4(daemon)、GAP-2.3(Socket API)
- **风险点**: SSH 私钥管理、reconnect 状态、`russh` 与 ControlMaster 共存策略
- **DoD**: `pureterm ssh connect user@host`,关 GUI 重启,session 仍可 attach
- **对标**: cmux `daemon/remote/cmuxd-remote`

#### GAP-5.6 完整 reconnect/disconnect 状态机 {#gap-5-6}
- **状态**: ❌
- **优先级**: P0
- **PureTerm 现状**: `SshPty` 单次 connect/disconnect,无重连
- **目标**: 显式状态机 `enum SshState { Disconnected, Connecting, Authenticating, Connected(SessionId), Reconnecting { attempt, next_retry } }`;tokio interval + 指数退避(最大 60s);Socket API event `SshStateChanged { profile, state }`;UI 显示当前状态
- **依赖**: GAP-5.1
- **风险点**: 状态转换的边界情况(用户手动 disconnect 取消重连)
- **DoD**: SSH 中拔网线 5s,网络恢复后自动重连,UI 显示重连过程
- **对标**: cmux 完整事件流状态机

### D.P1

#### GAP-5.2 SOCKS5/HTTP CONNECT Browser Tunnel {#gap-5-2}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: 无任何 SSH 隧道能力
- **目标**: SSH 创建本地 SOCKS5/HTTP CONNECT 代理;绑定 GAP-3.5 Browser 面板;CLI `pureterm ssh tunnel user@host --local-port 1080 --socks5`;`tokio` 写 `Socks5Server`,转发到 `russh::Channel`
- **依赖**: GAP-5.1
- **风险点**: 多路复用、TCP 性能
- **DoD**: `curl --socks5 localhost:1080 example.com` 经 SSH 代理成功
- **对标**: cmux `Sources/SSH/Tunnel.swift`

#### GAP-5.4 SSH agent forwarding {#gap-5-4}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: 无 agent forwarding
- **目标**: 自动检测 `$SSH_AUTH_SOCK`;`-o ForwardAgent=yes`;`russh::Agent` 协议;转发到 SSH channel
- **依赖**: GAP-5.1
- **风险点**: 平台差异(macOS Keychain agent vs Linux gpg-agent)
- **DoD**: `ssh-add` 后远程 `git push` 不需输密码
- **对标**: cmux 标准 ForwardAgent

#### GAP-5.5 `pureterm://` URL scheme {#gap-5-5}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: 无 URL scheme
- **目标**: 注册 `pureterm://` 到 OS:
  - `pureterm://ssh/user@host:port?profile=production`
  - `pureterm://workspace/<name>`
  - `pureterm://open?path=/some/file`
  - macOS `Info.plist` + `LSSetDefaultHandlerForURLScheme`(`objc2`)
  - Linux `.desktop` 文件 + `MimeType=x-scheme-handler/pureterm`
  - Windows 注册表
- **依赖**: 无
- **风险点**: 跨平台实现差异
- **DoD**: 浏览器点 `pureterm://ssh/test@127.0.0.1:22` 唤起 PureTerm 并打开 SSH Tab
- **对标**: cmux `cmux://`

### D.P2

#### GAP-5.3 smallest-screen-wins PTY resize {#gap-5-3}
- **状态**: ❌
- **优先级**: P2
- **PureTerm 现状**: 每个 Surface 独立 PTY,无共享
- **目标**: 多 Pane attach 同一 SSH session 时,PTY 大小 = min(行列);`SshSessionManager` 维护 `attached_surfaces: HashMap<SurfaceId, (rows, cols)>`
- **依赖**: GAP-5.1
- **风险点**: Pane 关闭时 detach
- **DoD**: 2 个 Pane 共享 SSH session,PTY 大小 = 较小者
- **对标**: cmux 多 surface 共享 SSH PTY

---

## 7. 类别 E:集成与可编程

### E.P0

#### GAP-6.1 Socket API v2 完整协议 {#gap-6-1}
- **状态**: ❌
- **优先级**: P0
- **PureTerm 现状**: 3 个 method(`pureterm msg`)
- **目标**: 完成 GAP-2.3 设计的完整 method,首版 30+;每个命名空间至少 3-5 个 method;错误码 `-32601/-32602/-32603`
- **依赖**: GAP-2.3
- **风险点**: 同 GAP-2.3
- **DoD**: 30+ method 在 `pureterm-cli` 中可调用
- **对标**: cmux `Packages/SocketControl/`

#### GAP-6.2 CLI 协议 {#gap-6-2}
- **状态**: ❌
- **优先级**: P0
- **PureTerm 现状**: 基础 `pureterm { msg, migrate, ... }`
- **目标**: 9 大组 — `pureterm { window, workspace, pane, surface, ssh, feed, system, debug, events }`,每组 5-10 子命令;全局 flag `--socket / --password / --json / --id-format`;环境变量 `PURETERM_SOCKET_PATH / PURETERM_WORKSPACE_ID`;`clap` 4.x derive 风格
- **依赖**: GAP-6.1
- **风险点**: CLI 维护成本
- **DoD**: `pureterm --help` 列出 9 大组,每组 5+ 子命令
- **对标**: cmux `CLI/cmux.swift`(32119 行,283+ 命令)

### E.P1

#### GAP-6.4 自定义命令注册 {#gap-6-4}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: 无
- **目标**: `alacritty.toml` `[[commands]]` 段:
  ```toml
  [[commands]]
  id = "deploy.staging"
  title = "Deploy to Staging"
  key = "Cmd+Shift+D"
  command = "ssh staging 'cd app && ./deploy.sh'"
  ```
  注册到 action 解析器 + 命令面板可搜
- **依赖**: GAP-3.3(已 ✅)
- **风险点**: 命令安全(避免任意命令执行)
- **DoD**: 注册的 `deploy.staging` 在命令面板可搜到并触发
- **对标**: cmux `Sources/CmuxConfig.swift::commands`

#### GAP-6.7 事件流 + 持久 JSONL 日志 {#gap-6-7}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: 无
- **目标**: Socket API `events` 命名空间;事件分类(window / workspace / surface / pane / notification / feed / agent / sidebar / browser / app / config);JSONL 持久化 `~/.local/share/pureterm/events.jsonl`,16 MiB rotation;4096 in-memory replay buffer;`tokio::sync::broadcast` channel;CLI `pureterm events --type=feed`
- **依赖**: GAP-6.1
- **风险点**: 高频事件性能
- **DoD**: `pureterm events --type=feed` 实时显示审批请求
- **对标**: cmux `Sources/EventStream.swift` + `cmux events`

### E.P2

#### GAP-6.3 URL scheme / AppleScript 等价桥 {#gap-6-3}
- **状态**: ❌
- **优先级**: P2
- **PureTerm 现状**: 无 URL scheme(见 GAP-5.5)
- **目标**: macOS AppleScript 桥(`osascript -e 'tell application "PureTerm" to ...'` via `objc2` + `NSAppleEventManager`);Linux D-Bus(`zbus` crate);Windows COM
- **依赖**: GAP-5.5
- **风险点**: 平台差异大
- **DoD**: macOS `osascript -e 'tell application "PureTerm" to create ssh tab'` 工作
- **对标**: cmux `Sources/AppleScriptBridge.swift`

#### GAP-6.6 tmux 兼容子集 {#gap-6-6}
- **状态**: ❌
- **优先级**: P2
- **PureTerm 现状**: 无
- **目标**: `pureterm tmux ...` 子集 — `capture-pane / resize-pane / pipe-pane / wait-for / swap-pane / break-pane / join-pane`;解析 tmux 参数映射到 Socket API
- **依赖**: GAP-6.1
- **风险点**: 100% 兼容不现实,做子集
- **DoD**: `pureterm tmux capture-pane -t %0 -p` 输出最近 50 行
- **对标**: tmux compat / Ghostty `terminal/tmux/*.zig`

#### GAP-6.8 可编程 Sidebar 解释器(IPC 侧) {#gap-6-8}
- **状态**: ❌
- **优先级**: P2
- **PureTerm 现状**: 无
- **目标**: 同 GAP-3.2,本侧重 IPC — 解释器订阅事件流,sidebar 视图 = 事件流聚合 + 用户渲染回调
- **依赖**: GAP-3.2、GAP-6.7
- **DoD**: sidebar 文件订阅 `feed.*` 事件,实时显示待审批数

### E.P3

#### GAP-6.5 Dock 集成 {#gap-6-5}
- **状态**: ❌
- **优先级**: P3
- **PureTerm 现状**: 无
- **目标**: macOS Dock 角标(Feed 项数 / 待审批);`NSApplication.dockTile().setBadgeLabel()`;Dock 右键菜单显示最近 Workspace
- **依赖**: GAP-7.4(Feed 审批)
- **风险点**: 仅 macOS
- **DoD**: macOS Dock 显示 "3" 角标
- **对标**: cmux `Sources/DockController.swift`

---

## 8. 类别 F:AI Agent

### F.P0

#### GAP-7.1 Agent Hooks 通用框架 {#gap-7-1}
- **状态**: ❌
- **优先级**: P0
- **PureTerm 现状**: 无
- **目标**: JSONL over stdin/stdout 协议;hook 事件 `PreToolUse / PostToolUse / Notification / Stop / PermissionRequest / AskUserQuestion / ExitPlanMode`;`tokio::process::Command` 子进程;与 Claude Code hook 协议对齐(https://docs.claude.com/en/docs/claude-code/hooks);TOML 配置 `[[agent.hook]] command = "..." event = "PreToolUse"`
- **依赖**: 无
- **风险点**: 进程崩溃、超时
- **DoD**: 配置 hook 拦截 `Bash` tool,自动拒绝 `rm -rf` 命令
- **对标**: cmux `Sources/AgentHook/`

#### GAP-7.2 16+ agent adapter(Claude Code 优先) {#gap-7-2}
- **状态**: ❌
- **优先级**: P0
- **PureTerm 现状**: 无
- **目标**:
  - 优先级:Claude Code → Codex → OpenCode → Grok → Pi → 其他
  - 每个 adapter:识别进程 + 注入 hook + 显示状态
  - `pureterm agents` 列出已注册,`pureterm agents install claude-code` 安装 hook
  - Claude Code:写 `.claude/hooks.json`
  - Codex:`~/.codex/config.toml::notify`
- **依赖**: GAP-7.1
- **风险点**: agent 协议变化快
- **DoD**: 安装 Claude Code hook,PreToolUse 事件被 PureTerm 拦截
- **对标**: cmux `Sources/AgentHook/Adapters/`

#### GAP-7.4 Feed 审批面板 {#gap-7-4}
- **状态**: ❌
- **优先级**: P0
- **PureTerm 现状**: 无
- **目标**: 右侧栏 Feed mode;Permission / ExitPlanMode / AskUserQuestion;Once / Always(此会话) / All tools(此 agent) / Bypass(临时) / Deny;120s 软等待(超时默认 deny);审计日志 `~/.local/share/pureterm/feed.jsonl`;agent → hook → 事件总线 → Feed
- **依赖**: GAP-7.1、GAP-3.1
- **风险点**: 同步阻塞 hook 的超时
- **DoD**: Claude Code 请求 `Bash` 权限,Feed 弹 4 选项,选 Once 后命令执行
- **对标**: cmux `Sources/Feed/RightSidebar.swift`

### F.P1

#### GAP-7.3 Agent Hibernation {#gap-7-3}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: 无
- **目标**: 监控 idle 时间;超阈值弹确认(`maxLiveTerminals=12, idleSeconds=5, 60s confirmation`);`SIGSTOP + 保存 session id`;`--resume <id>` 重启;`nix` crate(Unix);定期 `top -l 1 -pid <pid>` 或 `/proc/<pid>/stat`
- **依赖**: GAP-7.2
- **风险点**: SIGSTOP 不可移植到 Windows
- **DoD**: 5 个 agent,3 个 idle 5 分钟后被 hibernate,resume 后 session 恢复
- **对标**: cmux `Sources/AgentHibernation.swift`

#### GAP-7.5 Agent Session Restore {#gap-7-5}
- **状态**: ❌
- **优先级**: P1
- **PureTerm 现状**: 无
- **目标**: 持久化 session id 到 `~/.local/share/pureterm/agents/<agent>-sessions.json`;启动时 `--resume <id>`;支持 `claude --resume` / `codex --resume`
- **依赖**: GAP-7.2、GAP-2.4
- **风险点**: agent resume 协议可能变化
- **DoD**: 关 PureTerm,重启后 Claude Code 自动恢复最近 session
- **对标**: cmux `Sources/Agent/SessionRestore.swift`

### F.P2

#### GAP-7.6 Vault agent registry {#gap-7-6}
- **状态**: ❌
- **优先级**: P2
- **PureTerm 现状**: 无
- **目标**: `alacritty.toml::[vault.agents]` 声明式注册:
  ```toml
  [[vault.agents]]
  name = "claude-code"
  detect = "ps aux | grep claude"
  session_id_source = "transcript"
  resume_command = "claude --resume $SESSION_ID"
  ```
  启动扫描注册表识别运行 agent
- **依赖**: GAP-7.5
- **风险点**: detect 启发式
- **DoD**: 配置 Claude Code,启动自动识别并 attach 到现有 session
- **对标**: cmux `Sources/CmuxConfig.swift::vault.agents[]`

#### GAP-7.7 Custom resume bindings {#gap-7-7}
- **状态**: ❌
- **优先级**: P2
- **PureTerm 现状**: 无
- **目标**: 自定义命令模板 + 签名审批;`pureterm surface resume set --kind tmux --checkpoint work --shell "tmux attach -t work"`;`handlebars` crate + HMAC-SHA256;通过 GAP-7.4 Feed 审批流
- **依赖**: GAP-7.6
- **风险点**: 安全性(共享密钥)
- **DoD**: 配置 `--kind tmux --checkpoint work`,session 关闭时执行 `tmux attach`
- **对标**: cmux `Sources/ResumeBinding.swift`

#### GAP-7.8 agent-browser 兼容 {#gap-7-8}
- **状态**: ❌
- **优先级**: P2
- **PureTerm 现状**: 无 Browser 面板(见 GAP-3.5)
- **目标**: GAP-3.5 实现后,实现 agent-browser 兼容子集 — `click / type / screenshot / navigate / extract`;参考 `https://agent-browser.dev/protocol`
- **依赖**: GAP-3.5
- **风险点**: 兼容性深度
- **DoD**: agent 发 `click #submit`,Browser 面板点击对应 DOM
- **对标**: cmux `Sources/Browser/`(93/125 命令兼容)

---

## 9. 类别 G:平台

### G.P3

#### GAP-8.1 Sparkle auto-update {#gap-8-1}
- **状态**: ❌
- **优先级**: P3
- **PureTerm 现状**: 无自动更新
- **目标**: macOS 嵌入 Sparkle 框架(`objc2`);Linux `apt/pacman` 仓库 + 检查;Windows `winget` + Squirrel;跨平台抽象 `updater::check_and_notify()`
- **依赖**: 无
- **风险点**: macOS 依赖打包
- **DoD**: macOS 检查新版本,菜单栏通知
- **对标**: cmux `Sources/Updater/SparkleController.swift`

---

## 10. ⛔ 明确拒绝

| GAP | 标题 | 拒绝理由 |
|---|---|---|
| GAP-8.2 | 商业 tier | 偏离开源定位 |
| GAP-8.3 | iOS 伴侣 app | PureTerm 主线不进入移动平台战略;跨平台桌面是优势 |
| GAP-8.4 | Cloud VM(Next.js + AWS RDS 后端) | 非开源范围,不进入云组件 |

---

## 11. 进度看板(滚动更新)

| GAP | 标题 | 类别 | 优先级 | 状态 | 闭环日期 | 备注 |
|---|---|---|---|---|---|---|
| GAP-1.2 | DECSCL 设备控制 | C | P2 | ❌ | - | - |
| GAP-1.3 | 标题栈(CSI 22/23t) | C | P2 | ❌ | - | - |
| GAP-1.4 | BiDi 双向文本 | C | P2 | ❌ | - | - |
| GAP-2.1 | Surface/Pane/Workspace/Window 抽象层 | A | P1 | ❌ | - | - |
| GAP-2.2 | 嵌套水平+垂直分屏 | A | P0 | 🚧 | - | split_tree 已有,缺 action 入口 |
| GAP-2.3 | Socket API 协议 + 端口模型 | A | P0 | 🚧 | - | 3/30+ method |
| GAP-2.4 | 远程守护进程 puretermd | A | P1 | ❌ | - | - |
| GAP-2.5 | 多 UI 路径统一治理 | A | P0 | 🚧 | - | iced 已是主路径 |
| GAP-2.6 | iOS 伴侣 + Cloud VM | A | P3 | ⛔ | - | 见 §10 |
| GAP-3.1 | 双侧边栏 | B | P1 | ❌ | - | - |
| GAP-3.2 | 可编程 Sidebars(DSL) | B | P2 | ❌ | - | - |
| GAP-3.3 | 命令面板 ⌘⇧P | B | P0 | ✅ | 2026-06-08 | nucleo-matcher + Actions/Profiles |
| GAP-3.4 | 状态栏 | B | P1 | ❌ | - | - |
| GAP-3.5 | 内置 Browser 面板 | B | P1 | ❌ | - | wry crate |
| GAP-3.6 | macOS Glass 效果 | B | P3 | ❌ | - | - |
| GAP-3.7 | 完整快捷键自定义 + 模板 | B | P0 | 🚧 | - | 50+ Action,缺模板 |
| GAP-3.8 | 搜索体系 | B | P1 | 🚧 | - | 基础搜索已有,缺目录/全局 |
| GAP-4.1 | Vim 复制模式 | C | P1 | ❌ | - | - |
| GAP-4.2 | 智能选择 hover preview | C | P1 | 🚧 | - | 9 规则已有,缺 hover |
| GAP-4.3 | Tail-Find 自动滚动搜索 | C | P1 | ❌ | - | - |
| GAP-4.6 | Touch Bar / VoiceOver | C | P3 | ❌ | - | - |
| GAP-5.1 | 持久 PTY 守护进程 | D | P0 | ❌ | - | - |
| GAP-5.2 | SOCKS5/HTTP CONNECT Tunnel | D | P1 | ❌ | - | - |
| GAP-5.3 | smallest-screen-wins resize | D | P2 | ❌ | - | - |
| GAP-5.4 | SSH agent forwarding | D | P1 | ❌ | - | - |
| GAP-5.5 | pureterm:// URL scheme | D | P1 | ❌ | - | - |
| GAP-5.6 | reconnect/disconnect 状态机 | D | P0 | ❌ | - | - |
| GAP-5.7 | SSH Tab UI 串通 | D | P0 | ✅ | 2026-06-08 | ⌘⇧S + graceful_close |
| GAP-5.8 | SSH 主 UI 设置 UI | D | P0 | ✅ | 2026-06-08 | 右侧栏 + CredentialStore |
| GAP-5.9 | SSH 命令面板集成 | D | P0 | ✅ | 2026-06-08 | Profiles mode 一跳到底 |
| GAP-6.1 | Socket API v2 协议 | E | P0 | ❌ | - | - |
| GAP-6.2 | CLI 协议 | E | P0 | ❌ | - | - |
| GAP-6.3 | URL scheme / AppleScript 桥 | E | P2 | ❌ | - | - |
| GAP-6.4 | 自定义命令注册 | E | P1 | ❌ | - | - |
| GAP-6.5 | Dock 集成 | E | P3 | ❌ | - | - |
| GAP-6.6 | tmux 兼容子集 | E | P2 | ❌ | - | - |
| GAP-6.7 | 事件流 + JSONL 日志 | E | P1 | ❌ | - | - |
| GAP-6.8 | 可编程 Sidebar 解释器 | E | P2 | ❌ | - | - |
| GAP-7.1 | Agent Hooks 通用框架 | F | P0 | ❌ | - | - |
| GAP-7.2 | 16+ agent adapter | F | P0 | ❌ | - | Claude Code 优先 |
| GAP-7.3 | Agent Hibernation | F | P1 | ❌ | - | - |
| GAP-7.4 | Feed 审批面板 | F | P0 | ❌ | - | - |
| GAP-7.5 | Agent Session Restore | F | P1 | ❌ | - | - |
| GAP-7.6 | Vault agent registry | F | P2 | ❌ | - | - |
| GAP-7.7 | Custom resume bindings | F | P2 | ❌ | - | - |
| GAP-7.8 | agent-browser 兼容 | F | P2 | ❌ | - | - |
| GAP-8.1 | Sparkle auto-update | G | P3 | ❌ | - | macOS 优先 |
| GAP-8.2 | 商业 tier | G | P3 | ⛔ | - | 见 §10 |
| GAP-8.3 | iOS 伴侣 | G | P3 | ⛔ | - | 见 §10 |
| GAP-8.4 | Cloud VM | G | P3 | ⛔ | - | 见 §10 |
| GAP-8.5 | 跨平台优势 | G | N/A | ✅ | - | 已是优势 |

> **不在本表追踪的归档项**(详见 [`README.md` §已归档特性](../README.md#已归档特性2026-06-08-起)):
> 触发器系统(原 GAP-4.4)、OSC 133 Shell 集成扩展(原 GAP-4.5)、8 pass wgpu 管线、HarfBuzz 整形、字体回退链、背景图像渲染、5 个实验 crate。

---

## 12. 维护说明

**何时更新本文件**:
- 完成一个 P0/P1 → 同步 §1.1 数字摘要 + §11 进度看板 + §2 已实现条目
- 季度末 → 同步 §1.2 本季度焦点
- cmux/Ghostty/iTerm2 有重大发布 → 整体复盘并增补 GAP
- 新能力归档 → 移到 [`README.md` §已归档特性](../README.md#已归档特性2026-06-08-起),从本文件移除

**不写实现细节**:
- 本文件是 roadmap,不写代码
- 子任务在 GitHub Issue 或 `docs/superpowers/specs+plans/` 中维护
- 关联 PR 链接在 §11 备注列填入

**与 README 的边界**:
- 本文件:未来要做什么 + 当前在做什么
- README:当前已经做了什么 + 历史归档了什么

---

## 13. 参考资源

### 13.1 cmux 关键目录

| 路径 | 用途 |
|---|---|
| `cmux/Sources/TerminalController.swift`(21540 行) | v2 method 调度器 |
| `cmux/Packages/SocketControl/` | 283+ v2 socket method |
| `cmux/CLI/cmux.swift`(32119 行) | CLI 283+ 命令 |
| `cmux/daemon/remote/cmuxd-remote/` | Go 持久 PTY daemon |
| `cmux/Sources/AgentHook/` | Agent hook 框架 |
| `cmux/Sources/Feed/` | Feed 审批面板 |
| `cmux/Sources/Sidebar/` | 双侧边栏 |
| `cmux/vendor/bonsplit` | 嵌套分屏库 |

### 13.2 PureTerm 内部参考

| 路径 | 用途 |
|---|---|
| `crates/pureterm/src/iced_app.rs` | iced UI 主循环(96 KB) |
| `crates/pureterm/src/split_tree.rs` | 分屏树(23.5 KB) |
| `crates/pureterm/src/terminal_shader.rs` | 终端着色器(30.9 KB) |
| `crates/pureterm/src/config_watcher.rs` | 配置热重载 |
| `crates/pureterm/src/ssh/` | SSH 后端(7 个文件) |
| `crates/pureterm-settings/src/pages/` | Settings UI 6 页面 |
| `crates/alacritty_terminal/src/smart_selection.rs` | 智能选择 9 规则 |
| `archive/` | 归档历史(见 `removed-legacy-features.md`) |

### 13.3 协议与规范

| 协议 | 链接 |
|---|---|
| JSON-RPC 2.0 | https://www.jsonrpc.org/specification |
| Claude Code hooks | https://docs.claude.com/en/docs/claude-code/hooks |
| agent-browser | https://agent-browser.dev/protocol |
| XTerm modifyOtherKeys | https://invisible-island.net/xterm/modified-keys.html |
| Tmux control mode | https://github.com/tmux/tmux/wiki/Control-Mode |
| Kitty image protocol | https://sw.kovidgoyal.net/kitty/graphics-protocol/ |
