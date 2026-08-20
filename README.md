# PureTerm

> 基于 Alacritty 内核 + Iced 0.14 daemon 的 GPU 加速终端模拟器,纯 Rust 编写,跨平台(macOS / Linux / Windows)。
> 主分支:`iced-cover`。

---

## 目录

- [核心特性](#核心特性)
- [快速开始](#快速开始)
- [架构](#架构)
- [已实现特性](#已实现特性)
- [使用示例](#使用示例)
- [配置](#配置)
- [快捷键速查](#快捷键速查)
- [路线图](#路线图)
- [贡献与协议](#贡献与协议)

---

## 核心特性

- **GPU 加速渲染** — Iced 0.14 + 自定义 wgpu shader,跨平台 Metal / Vulkan / DX12 后端
- **多标签嵌套分屏** — 水平/垂直递归分屏,拖拽调整比例
- **会话持久化 + 跨重启恢复** — `puretermd` 守护进程持有 PTY master fd,关 GUI/崩溃不丢会话;重开自动恢复上次的 tabs/分屏布局/窗口尺寸(含 SSH,重连到存活 child 无需重新认证)(2026-06-17 GAP-5.1 ✅ + GAP-5.6 daemon 化 🚧)
- **SSH 一等公民** — 完整后端 + Profile + 加密凭证 + 主 UI 设置 + 命令面板一跳到底(2026-06-08 GAP-5.7/5.8/5.9 闭环)
- **独立设置 UI 进程** — Iced 0.14 daemon,`Cmd+,` 启动,6 个分类页面
- **命令面板** — `⌘⇧P` 触发,`nucleo-matcher` 模糊搜索,Actions / Profiles 双模式
- **快捷键自定义** — TOML 配置驱动 + 6 套预设模板(vim/tmux/macos/vscode/agent-triage) + CLI `pureterm shortcuts` + 热重载(2026-06-12 GAP-3.7 闭环)
- **Socket API v2** — JSON-RPC 2.0 over Unix socket,31 个 method 跨 8 命名空间;结构化 CLI `pureterm <group> <sub>`(GAP-6.2)+ 原始 `pureterm msg` 逃生舱,含写操作(tab/pane/ssh/config)(2026-06-13 GAP-6.1/6.2)
- **状态栏** — 底部 24px,显示 CWD / Git 分支 / SSH / 进程名 / 光标位置(2026-06-12 GAP-3.4)
- **Vi Copy Mode** — `Ctrl+Shift+Space` 进入,hjkl 导航,v/V/Ctrl+v 选区,y 复制,/ 搜索(2026-06-12 GAP-4.1)
- **配置热重载** — 改 `alacritty.toml` 自动应用,无需重启
- **跨平台** — macOS / Linux / Windows 三平台持续维护

---

## 快速开始

```bash
git clone <repo> && cd PureTerm
cargo build --release
./target/release/pureterm
```

**配置文件路径**:
- 默认:`~/.config/pureterm/alacritty.toml`
- 回退:`~/.config/alacritty/alacritty.toml`(完全兼容标准 Alacritty TOML schema)

**关键快捷键**:
- 打开设置:`Cmd+,`(macOS)
- 命令面板:`⌘⇧P`
- SSH 连接:`⌘⇧S`
- Vi 复制模式:`Ctrl+Shift+Space`

**CLI 命令**:
```bash
pureterm system version         # 程序化控制(需要运行实例,GAP-6.2 命令组)
pureterm tab new                # 创建标签;pureterm --help 列出 8 个命令组
pureterm shortcuts list         # 查看快捷键
pureterm shortcuts preset vim   # 切换预设
```

完整快捷键见 [快捷键速查](#快捷键速查)。

---

## 架构

PureTerm 由 6 个 crate 组成的工作区:

| Crate | 用途 |
|---|---|
| `pureterm` | 主二进制入口,Iced 0.14 daemon — 主路径分屏 / Tab / SSH / 命令面板均在此 |
| `pureterm-alacritty-runtime` | 已精简,仅承载 GPU 字体图集 `glyph_atlas`(2026-06-08 起从 75+ 源文件减到 3 个) |
| `pureterm-settings` | 独立设置 UI 进程(Iced 0.14 daemon),`Cmd+,` 启动 |
| `alacritty_terminal` | vendored 上游 — 终端内核 / PTY / Grid / Selection |
| `alacritty_config` | vendored 上游 — 配置反序列化抽象 |
| `alacritty_config_derive` | vendored 上游 — 配置 derive 宏 |

**维护规则**:
- vendored Alacritty 源码不做无关格式化或重构,便于上游同步
- 新功能在主路径 `crates/pureterm/src/` 实现,`pureterm-alacritty-runtime` 不再扩展
- 修改 vendored alacritty 源码时,在修改处加注释说明原因

依赖关系:`pureterm` → `pureterm-alacritty-runtime` → 3 个 vendored alacritty crate。详见 [`crates/AGENTS.md`](crates/AGENTS.md)。

---

## 已实现特性

### 渲染
- Iced 0.14 + 自定义 wgpu shader(`crates/pureterm/src/terminal_shader.rs`,30.9 KB)
- 文字模糊处理(commit `b1320de`,改善低分屏阅读体验)
- 字体图集 LRU + dirty region(`pureterm-alacritty-runtime/src/renderer/glyph_atlas.rs`)
- 跨平台 wgpu 后端:Metal / Vulkan / DX12 自动选择

### 标签与分屏
- 多标签 + 嵌套水平/垂直分屏(`crates/pureterm/src/split_tree.rs`,23.5 KB)
- 拖拽调整分屏比例(`MIN_RATIO=0.1`,`MAX_RATIO=0.9`)
- **分屏操作补齐(2026-06-10 GAP-2.2)** — 键盘 + 右键菜单 + 命令面板三入口:
  - 等分所有分屏 `⌘=`(递归 `equalize_all`)
  - 交换 Pane `⌘⇧X`(翻转 first/second,ratio 不跳)
  - 临时最大化 `⌘⇧Enter`(`Tab.zoom_state` 保存原树,再按恢复)
  - 焦点 Next/Prev Pane `⌘]` / `⌘[` (DFS 先序循环;与 `⌘⌥+方向键` 几何相邻并存)
  - 移动 Pane 到新 Tab `⌘⇧T` / 新窗口 `⌘⇧N`
  - 命令面板搜 `split.*` 7 个 builtin
- 标签关闭策略:本地 Tab 直接关;SSH Tab 走 `graceful_close`(发送 `exit\r` → 等 500ms → SIGHUP 兜底);关 tab/pane 同步 `session.kill` 结束其 daemon 会话(语义见下「会话持久化与跨重启恢复」)
- macOS 原生菜单 + Dock reopen 集成

### SSH 一等公民(2026-06-08 GAP-5.7/5.8/5.9 闭环)
- **SSH 后端** 在主路径 `crates/pureterm/src/ssh/`(7 个文件):
  - ControlMaster socket 复用,多标签共享同一 SSH 连接
  - `~/.ssh/config` 集成,直接用 Host 别名
  - SSH Profile 可在 `alacritty.toml` 中声明
- **加密凭证存储**:AES-256-GCM + Argon2id,`Zeroizing` 包裹 master_key
- **主 UI SSH 设置面板**:右侧栏 `SshSettingsPanelState`,Profile 列表 + 实时编辑 + 保存到 toml
- **SSH Tab UI**:`⌘⇧S` 弹对话框 → 异步 spawn → `PtyEventLoop` 集成 → 标题显示 `🔒 SSH: <name>`
- **命令面板集成**:`⌘⇧P` 触发,`nucleo-matcher` 模糊搜索,`Mode::Actions` ↔ `Mode::Profiles` 切换,选 Profile 一跳到底连接
- **断线重连**(2026-06-14 GAP-5.6 进程内子集):SSH 连接 >5s 后掉线 → 指数退避(3→60s × 8)自动重连,保留滚动历史;状态栏显示 `SSH` / `SSH ↻ n` / `SSH ✗` / `SSH ⏻`;`pureterm ssh disconnect` 手动断开不触发重连

### 会话持久化与跨重启恢复(2026-06-17 GAP-5.1 ✅ + GAP-5.6 daemon 化 🚧)
- **持久守护进程 `puretermd`**:单例(flock)+ `setsid` 脱离 GUI 进程组,PTY master fd 由 pump 线程持有 → GUI 关闭/崩溃后子 shell 不死;每会话 1 MiB `RingBuffer` 原始字节,新 attach 回放为 `SCROLLBACK` 帧重建 grid;多 GUI 客户端可共享同一会话(多 attach 扇出)
- **跨重启恢复**:GUI 重启时读 `gui-sessions.json` → `session.list` 校验存活会话 → `attach_existing` 重连(本地 + SSH)→ scrollback 回放重建可见区;恢复 tabs 顺序/标题/分屏树(SplitNode 方向 + 比例)/窗口尺寸/active tab;孤儿会话(daemon 有 / 快照无)作额外 tab 接回;SSH 重连到存活 `ssh -tt` child 无需重新认证
- **持久化策略**:布局变更去抖(`time::every(250ms)` + ≥500ms 空闲)`NamedTempFile::persist` 原子写 + 关窗 flush;daemon 不可用 / 无存活会话时回落单新 tab(零回归)
- **状态文件**:`~/Library/Application Support/pureterm/daemon/gui-sessions.json`(owner-only 0700)
- **关闭语义**(刻意区分):`⌘W` 关闭 tab / pane → 结束其 daemon 会话(`session.kill`),下次**不再**恢复;退出 app(`⌘Q` / 关窗 / 杀进程)→ **保留**所有存活会话,下次恢复。即「关 tab = 不要这个会话;退出 = 暂时离开」

### Settings UI(独立进程)
- 独立二进制 `pureterm-settings`,通过 `Cmd+,` 启动
- 主题:Halloy 风格暖色调暗色(Ferra 配色方向)
- 6 个分类页面:**General / Appearance / Colors / Terminal / SSH Profiles / Keyboard Shortcuts**
- 配置 I/O:`toml_edit::DocumentMut` 格式保留读写,`NamedTempFile::persist()` 原子写入
- 与主进程通信:写文件 → 主进程 `ConfigMonitor` 自动热重载

### 配置与热重载
- 配置文件:`~/.config/pureterm/alacritty.toml`(回退 `~/.config/alacritty/alacritty.toml`)
- 标准 Alacritty TOML schema 完整兼容
- 热重载:`crates/pureterm/src/config_watcher.rs` 监听文件改动,自动应用

### 终端交互
- 完整 VTE 解析:CSI / ESC / OSC / DCS / APC / PM
- 智能选择 9 规则:URL / SSH URL / 邮箱 / 引号字符串 / 文件路径 / C++ namespace / dotted include / Git SHA / IPv4(`crates/alacritty_terminal/src/smart_selection.rs`)
- **Vi Copy Mode**(2026-06-12 GAP-4.1):`Ctrl+Shift+Space` 进入,hjkl 导航,v/V/Ctrl+v 选区,y 复制,//? 搜索,n/N 遍历,Escape 退出;GPU 渲染青色 vi cursor + 搜索匹配 amber 高亮;状态栏 -- COPY -- / -- VISUAL -- 指示(`crates/pureterm/src/vi_mode_handler.rs`)
- 键盘绑定:31+ Action 枚举,TOML 可自定义 + 6 套预设模板 + CLI 管理(`crates/pureterm/src/keymap.rs`)
- IME 输入法候选

### Socket API v2 远程接口(2026-06-13,GAP-6.1)
- **JSON-RPC 2.0** over Unix domain socket,程序化控制 PureTerm(查询 + 写操作)
- **31 个 method,8 命名空间**:
  - `system.*`(4):ping / version / info / methods
  - `window.*`(4):list / create / close / focus
  - `tab.*`(4):list / new / close / switch
  - `pane.*`(6):list / split / close / focus / swap / zoom
  - `surface.*`(4):list / send_text / resize / snapshot
  - `config.*`(4):get / list / reload / set
  - `ssh.*`(3):list_profiles / connect / disconnect
  - `debug.*`(2):ping / dump_state
- **写操作副作用机制**:`IpcSideEffect` 枚举 — 写 method(tab.new / pane.split / ssh.connect / config.reload 等)通过副作用回写 iced 主循环,真实驱动 UI 状态变更
- **CLI 客户端**:结构化命令组 `pureterm <group> <subcommand> [flags]`(2026-06-13 GAP-6.2),原始逃生舱 `pureterm msg <method> [JSON_PARAMS]`
- **自动发现**:`$PURETERM_SOCKET` 环境变量 + 文件系统扫描;CLI 全局 flag `--socket <PATH>` 覆盖 / `--json` 紧凑输出
- **架构**:`crates/pureterm/src/ipc/`(protocol / methods / listener / client) — tokio 异步监听 → mpsc channel → iced Subscription bridge → `dispatch` 返回 `(响应, Option<IpcSideEffect>)`
- **测试**:45 个单元测试全过

### 自定义命令(2026-06-15 GAP-6.4)
- 在 `pureterm.toml` 声明 `[[commands]]`,把常用 shell 片段(deploy / make test / ssh 跳板)做成「绑快捷键 + ⌘⇧P 可搜 + 一键触发」的命令
- **执行模型**:触发时把 `command + 回车` 写进**当前 pane 的 PTY**,在 pane 的 shell/CWD 执行、输出可见(类似主流终端 snippet / tmux send-keys);不引入新进程管理,快捷键与命令面板共用同一 `dispatch_action`
- 配置示例:
  ```toml
  [[commands]]
  id = "deploy.staging"
  title = "Deploy to Staging"
  key = "Cmd+Shift+D"          # 可选;不写则仅面板可搜
  command = "ssh staging 'cd app && ./deploy.sh'"

  [[commands]]
  id = "make.test"
  title = "Run Tests"
  command = "make test"
  ```
- **键修饰符**:`+` 和 `|` 都接受(`Cmd+Shift+D` 或 `Command|Shift|D`);也可拆成 `key = "D"` + `mods = "Command|Shift"`。shift + 字母会自动注册 macOS 双形态(小写+shift / 大写无 shift),非 shift 字母归一为小写
- **热重载**:存盘即生效(`apply_config_reload` 重新加载 + 重新注册绑定)
- 实现:`crates/pureterm/src/keymap.rs`(`CustomCommand` + `Action::CustomCommand` + `split_mods_and_key` + `register_custom_commands`)+ `app_config.rs`(`load_custom_commands`)+ `iced_app.rs`(dispatch + 面板);13 单测

### Agent Hooks 通用框架(2026-06-18 GAP-7.1 ✅)
- PureTerm 充当 Claude Code / 其它 agent 的 hook 中转:agent 把 hook 事件 JSON 经 stdin 喂给 `pureterm hook <event>`,PureTerm 按 `[[agent.hook]]` 策略评估,输出 Claude Code hooks 协议决策(stdout JSON / 退出码)。是整个 AI-Agent 类(F)的地基
- **Standalone**:不依赖 daemon / GUI,直接读 `pureterm.toml` + 本地评估 + 起子进程;每次工具调用零 IPC 往返
- **双规则模型**:
  - 内置 `deny_pattern`(正则,无需写脚本)—— 主体优先级:`tool_input.command`(Bash)→ `tool_input.file_path`(Write/Edit)→ `UserPromptSubmit.prompt` → `Notification.message`
  - 外部 `command`(通用机制)—— 经 `sh -c` 执行,stdin 喂事件 JSON,stdout JSON / exit code = 决策;带超时(默认 60s,超时 kill 且不阻断工具)
- 配置示例:
  ```toml
  [[agent.hook]]
  event = "PreToolUse"
  matcher = "Bash"               # 正则匹配 tool_name(仅 Pre/PostToolUse)
  deny_pattern = "\\brm\\s+-rf"  # 命中 → deny
  deny_reason = "rm -rf blocked by policy"

  [[agent.hook]]
  event = "PostToolUse"
  matcher = "Write|Edit"
  command = "/abs/path/lint.sh"  # 外部命令,经 sh -c 执行
  timeout = 30
  ```
- **Claude Code 接入**:推荐用下方 GAP-7.2 的自动安装器(`pureterm agents install claude-code`);手动登记则把下面这段写进 `~/.claude/settings.json`:
  ```json
  {"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"/abs/pureterm hook PreToolUse"}]}]}}
  ```
  之后 Claude Code 调 Bash 前会调用 PureTerm;`rm -rf` 命中即按协议返回 `permissionDecision:"deny"`,`ls` 等放行
- 事件对齐官方协议:`PreToolUse` / `PostToolUse` / `Notification` / `UserPromptSubmit` / `Stop` / `SubagentStop` / `PreCompact` / `SessionStart` / `SessionEnd`;决策输出:PreToolUse 走 `hookSpecificOutput.permissionDecision`,其它阻断型事件走 `{"decision":"block"}`
- 实现:`crates/pureterm/src/agent_hooks.rs`(框架:`HookEvent` / `HookRule` / `HookInput` / `Decision` + `evaluate` 引擎 + `run_external_command` 子进程+超时 + `run_hook_shim` CLI 入口)+ `app_config.rs`(`load_agent_hooks`)+ `cli.rs`(`Command::Hook`);171 单测含真实子进程/超时验证。**后续**:GAP-7.4 Feed 审批面板、GAP-6.7 事件转发

### Agent 适配器自动安装(2026-06-18 GAP-7.2 ✅)
- 在 GAP-7.1 的 `pureterm hook` shim 之上,一键把它注册进外部 agent 的配置 —— 免去手编 `~/.claude/settings.json`。框架化(`AgentAdapter` trait)使 Codex / OpenCode / Grok / Pi 成为机械式后续,v1 实现 Claude Code
- **CLI**(本地无 IPC):
  ```bash
  pureterm agents install claude-code              # 只注册 pureterm.toml 里已配 [[agent.hook]] 规则的事件
  pureterm agents install claude-code --all-events # 注册全 9 个 Claude Code 事件
  pureterm agents install claude-code --dry-run    # 预览将写入的 JSON,不落盘
  pureterm agents list                             # 列出已知 adapter + installed 状态
  pureterm agents uninstall claude-code            # 移除 PureTerm 写入的钩子
  ```
- **安全写配置**:对 `~/.claude/settings.json` 走**加锁读-改-写**(fs2 排他锁 + `NamedTempFile::persist` 原子写,镜像 `config_io::mutate_config`),完整保留 `enabledPlugins` / `env` / `permissions` 等既有内容;自有条目靠命令路径哨兵(`<pureterm bin> hook `)识别 → 安装幂等(去重)、卸载精准(只删自己的组,清空事件数组与空 hooks 键)
- **默认事件策略**:仅注册 `pureterm.toml` 中已配 `[[agent.hook]]` 规则的事件(Claude Code 不会在未配策略的事件上无谓 spawn pureterm);零规则时警告退出不写;`--all-events` 注册全量(之后新增规则无需重装,PureTerm `evaluate()` 内部按 matcher 过滤)
- 实现:`crates/pureterm/src/agent_adapters.rs`(`AgentAdapter` trait + provided 加锁 RMW 引擎 + `ClaudeCodeAdapter` + `registry` + `dispatch` + `pureterm_bin` 解析)+ `cli.rs`(`Command::Agents` / `AgentsSub`)+ `main.rs`(`mod agent_adapters`)+ `Cargo.toml`(`fs2`/`tempfile`);10 单测(保留外部内容 / 幂等 / 同事件多 matcher / 精准卸载 / 非 object root 拒写 等),整套 181 全绿

### Agent 自动发现 + 可观测(2026-06-22 GAP-7.6 🚧)
- 在 GAP-7.2 之上,让 agent 体验"开箱即用闭环"+"一眼可见":
  - **SessionStart 自动注册**(打通 GAP-7.5 冷恢复):`pureterm agents install claude-code` 后,只要 `pureterm.toml` 配了 `[[vault.agents]]`,SessionStart 钩子自动注册 —— agent session id 持久化 + 关 PureTerm 重开自动 `claude --resume`。修复了"装了适配器但冷恢复静默失效"的隐蔽坑(此前需 `--all-events` 或手配规则)
  - **进程扫描**:`pureterm agents detect` 扫描机器上正在运行的 agent(按 `[[vault.agents]]` 的 `detect` 子串匹配,排除 grep/rg/ps/shell),交叉 `agent_sessions` 给出可 resume 的 session id —— 发现 PureTerm 之外启动的 agent
  - **registry 检视**:`pureterm agents registry` 打印解析后的 `[[vault.agents]]`,确认配置生效
  - **状态栏 agent 指示**:`⚡ kind`(活跃)/`❄ kind`(休眠)/`⏳ N`(N 条 Feed 审批待决),无需打开面板即知 agent 状态
  - **Feed 审计查阅**:`pureterm feed list [--limit N]` 读 `feed.jsonl` 审批历史
- **DoD 语义重定义**:原"attach 到现有 session"在 daemon 自持 PTY 模型下不可落地 → 重定义为"识别 + 提示 resume"(外部 agent 扫描识别,提示在新 pane resume,非抢占 TTY)
- **配置**(`detect` 是进程名/cmdline **子串**,非 shell 命令 —— 避免注入):
  ```toml
  [[vault.agents]]
  kind = "claude-code"
  detect = "claude-code"                              # 可选,子串;缺省 = kind 小写
  resume_command = ["claude", "--resume", "{session_id}"]
  never_hibernate = false                             # 可选,豁免 GAP-7.3 休眠
  ```
- 实现:`agent_detect.rs` + `feed_log.rs`(新建)+ `agent_adapters.rs::derive_entries`(SessionStart 特例)+ `iced_app.rs`(`agent_indicator` 纯函数 + view_bottom_bar + `last_session_list` 缓存,零新 IPC)+ `cli.rs`(Feed/Detect/Registry);254 单测全绿。**待 dist 重打包 + GUI E2E**;GUI "检测到外部 session,是否 resume" 提示面板留后续

### 状态栏
- 底部 24px 状态栏(2026-06-12 GAP-3.4 闭环;2026-06-22 GAP-7.6 加 agent 指示)
- 显示:工作目录(OSC 7) · Git 分支(30s 轮询) · SSH 连接指示 · 前台进程名 · **agent 指示 `⚡ kind` / `❄ kind` / `⏳ N`**(2026-06-22 GAP-7.6) · 光标位置(row:col)

### 跨平台
- macOS / Linux / Windows
- 持续 CI 验证三平台
- 统一 `./build.sh` 跨平台打包(.app + .dmg / .tar.gz / .zip)— 见 [构建与打包](#构建与打包)

---

## 构建与打包

所有跨平台 / 跨目标工作统一通过 `./build.sh` 完成。
它取代了过去只能打包 macOS 的旧脚本,现在同一脚本可以产出
macOS `.app` + `.dmg`、Linux `.tar.gz`、Windows `.zip`。

### 快速命令

```bash
./build.sh --help          # 全部选项
./build.sh info            # 检测到的主机、已安装 rustup 目标、可用工具
./build.sh release         # 仅构建 release 二进制
./build.sh app             # 为主机 OS + 主机 triple 打包
./build.sh app-all         # 为主机 OS 的所有已安装 triple 打包
./build.sh matrix          # 跨平台矩阵打包 (跳过未安装的目标)
./build.sh test            # 运行测试 (nextest → cargo test)
./build.sh clean           # 清除 target/ 与 dist/
```

### 按平台产出的工件

| 主机 OS | 模式 | 产物 (位于 dist/) |
|---|---|---|
| macOS | `app` | `PureTerm.app/` · `PureTerm-<v>-arm64.app.zip` · `PureTerm-<v>-arm64.dmg` |
| macOS | `app --universal` | `PureTerm.app/` (含 universal2 二进制) · `PureTerm-<v>-universal.app.zip` · `PureTerm-<v>-universal.dmg` |
| Linux | `app` | `pureterm-<v>-<arch>-<env>/` (staged tree) · `pureterm-<v>-<arch>-<env>.tar.gz` |
| Windows | `app` | `pureterm-<v>-<arch>/` · `pureterm-<v>-<arch>.zip` |

### macOS universal2 二进制 (`--universal`)

```bash
./build.sh --universal app
# 或
./build.sh app --bundle all --universal
```

- 同时构建 `aarch64-apple-darwin` + `x86_64-apple-darwin` 两个目标。
- 用 `lipo -create -arch arm64 ... -arch x86_64 ...` 合并成单个 universal2 Mach-O,
  在 Apple Silicon 和 Intel Mac 上都原生运行 — **不会弹出"即将结束对 Intel 芯片 App 的支持"** 的 Rosetta 警告。
- 缺一不可:必须同时安装两个 rustup 目标 (`rustup target add aarch64-apple-darwin x86_64-apple-darwin`),且主机必须是 macOS — 跨主机 lipo 没有意义。
- 工件名带 `-universal` 后缀以与单架构产物区分;`.dmg` 同时也是 universal 的。

### 目标选择

```bash
# 单一目标
./build.sh --target aarch64-apple-darwin app
./build.sh --target x86_64-pc-windows-msvc app

# 多个目标 (逗号分隔)
./build.sh --targets aarch64-apple-darwin,x86_64-apple-darwin app

# 按架构筛选 (会展开为所有匹配的 triple)
./build.sh --arch x86_64 app-all
./build.sh --arch aarch64 matrix
```

### 工具可用性 (`./build.sh info`)

打包脚本在每个目标工具缺失时会优雅降级 — 不会因为缺 `hdiutil`
就拒绝构建。常用工具检测表:

| 工具 | 用途 | 缺失时的行为 |
|---|---|---|
| `sccache` | 编译缓存 | 静默降级,直接调用 rustc |
| `cargo-nextest` | 并行测试 | 回退到 `cargo test` |
| `mold` / `lld` | Linux 链接加速 | rust 自动用系统 ld |
| `hdiutil` | macOS .dmg | 跳过 .dmg,仍产出 .app.zip |
| `codesign` + `CODESIGN_IDENTITY` | macOS 代码签名 | 跳过签名 |
| `xcrun notarytool` + `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` | macOS 公证 | 跳过公证 |
| `zstd` | Linux .tar.zst | 跳过 zst 格式 |
| `zip` / `powershell` | Windows .zip | 二者皆无时报错 |
| `signtool` + `WINDOWS_CERT_FILE` / `WINDOWS_CERT_PASSWORD` | Windows 代码签名 | 跳过签名 |

### v1 未实现的打包格式

| 格式 | 平台 | 状态 |
|---|---|---|
| `.deb` | Linux | 待 `dpkg-deb` 支持 |
| `.rpm` | Linux | 待 `rpmbuild` 支持 |
| `.AppImage` | Linux | 待 `appimagetool` 支持 |
| `.msi` (wixl) | Windows | v1 仅便携 .zip |
| NSIS installer | Windows | v1 不提供 |

如果你的分发渠道需要以上任一格式,在仓库开 ticket。

### 跨平台构建矩阵 (v0.2 引入)

`./build.sh matrix` 会遍历以下 8 个目标,跳过主机上没有的 rustup 目标
与无法交叉编译的目标(例如在 macOS 上构建 Windows .exe 缺 sysroot):

| 主机 | x86_64 | aarch64 |
|---|---|---|
| macOS | `x86_64-apple-darwin` | `aarch64-apple-darwin` |
| Linux (gnu) | `x86_64-unknown-linux-gnu` | `aarch64-unknown-linux-gnu` |
| Linux (musl, 完全静态) | `x86_64-unknown-linux-musl` | `aarch64-unknown-linux-musl` |
| Windows (MSVC) | `x86_64-pc-windows-msvc` | `aarch64-pc-windows-msvc` |

`--auto-install-targets` 允许脚本在目标缺失时自动 `rustup target add`。

---

## 使用示例

### SSH Profile 配置(alacritty.toml)

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

可用 SSH Action:
- `Ssh = "Connect"` — 打开 SSH 连接对话框
- `Ssh = "OpenSettings"` — 打开 SSH 设置界面
- `Ssh = "ListProfiles"` — 列出所有 SSH Profile

### 命令面板使用

按 `⌘⇧P` 打开命令面板:
- 输入 `ssh` → 切到 Profiles 模式,选 Profile 直接连接
- 输入 `set` → 跳到 Settings UI 打开 action
- 输入任意 action ID 模糊搜索 → 直接触发

### 设置界面

按 `Cmd+,` 启动独立设置进程:
- 修改任意配置项 → 写入 `alacritty.toml`(原子写入,保留格式)
- 主进程 `ConfigMonitor` 检测到改动 → 自动热重载,无需重启

### 快捷键自定义(GAP-3.7)

**CLI 管理:**

```bash
# 查看当前所有快捷键
pureterm shortcuts list

# 切换预设模板(vim/tmux/macos/vscode/agent-triage)
pureterm shortcuts preset vim

# 恢复默认
pureterm shortcuts reset
```

**TOML 手动编辑** (`~/.config/pureterm/alacritty.toml`):

```toml
[[keyboard.bindings]]
key = "d"
mods = "Command"
action = "SplitVertical"

[[keyboard.bindings]]
key = "z"
mods = "Command"
action = "ToggleZoom"
```

保存后自动热重载,无需重启。自定义绑定覆盖同名默认绑定。

### Vi 复制模式(GAP-4.1)

按 `Ctrl+Shift+Space` 进入 Vi Copy Mode(状态栏显示 `-- COPY --`):

| 按键 | 动作 |
|---|---|
| `h` `j` `k` `l` | 左/下/上/右移动 |
| `w` `b` `e` | 按词前进/后退/词尾 |
| `0` `$` | 行首/行尾 |
| `g` `G` | 屏幕顶部/底部 |
| `Ctrl+u` `Ctrl+d` | 半页上滚/下滚 |
| `v` | 进入 Visual 选区(字符) |
| `V` | 进入 Visual Line 选区(整行) |
| `Ctrl+v` | 进入 Visual Block 选区(矩形) |
| `y` | 复制选中内容到剪贴板并退出 |
| `yy` | 复制当前行 |
| `/` `?` | 向前/向后搜索(弹出搜索栏) |
| `n` `N` | 下一个/上一个匹配 |
| `Escape` | 退出搜索 → 退出 visual → 退出 vi mode |

GPU 渲染:青色 vi 光标 + 搜索匹配 amber 高亮。

### Socket API 远程接口(GAP-6.1 + GAP-6.2)

PureTerm 运行时自动创建 Unix domain socket,可通过 CLI 程序化控制(31 个 method,含写操作)。GAP-6.2 提供 8 个结构化命令组(类型化参数,无需手写 JSON);`msg` 是原始 JSON-RPC 逃生舱:

```bash
# 结构化命令组(GAP-6.2,推荐)— pureterm --help 列出全部
pureterm system version                       # 查看版本
pureterm system ping                          # 心跳
pureterm tab list                             # 列出标签
pureterm tab new                              # 创建新标签
pureterm tab switch --index 1                 # 切换到标签 1
pureterm pane split --direction h             # 水平分屏(v|h)
pureterm pane zoom                            # 切换 pane 缩放
pureterm pane focus --direction next
pureterm config reload                        # 热重载配置
pureterm ssh list-profiles                    # 列出 SSH Profile(unix)
pureterm system methods                       # 列出所有可用 method

# 全局 flag
pureterm --json tab list                      # 紧凑 JSON 输出
pureterm --socket /tmp/x.sock system ping     # 显式指定 socket

# 原始 JSON-RPC 逃生舱(GAP-6.1,任意 method)
pureterm msg system.version
pureterm msg tab.switch '{"index":1}'
pureterm msg pane.split '{"direction":"horizontal"}'

# 错误处理(越界索引 → -32602 参数错误)
pureterm tab switch --index 99
# → Error (-32602): Index 99 out of range (0..2)

# 错误处理(未知 method)
pureterm msg foo.bar
# → Error (-32601): Method not found: foo.bar
```

完整命令组运行 `pureterm --help`(列出 8 组 + msg + shortcuts),某组的子命令运行 `pureterm <group> --help`,全部 method 列表运行 `pureterm system methods`。环境变量 `$PURETERM_SOCKET` 自动注册,CLI 客户端会自动发现 socket;也可用 `--socket <PATH>` 覆盖。

### 测试 GAP-6.2 CLI

```bash
# 1. 构建
cargo build --release

# 2. 无需 GUI 的纯 CLI 校验(help / 错误处理)
./target/release/pureterm --help               # 列出 8 命令组 + msg + shortcuts
./target/release/pureterm tab --help           # 列出 tab 子命令
./target/release/pureterm system --help
./target/release/pureterm bogus                # → clap 报错 exit 2(未知子命令)

# 3. 端到端(需要运行实例):开两个终端
#    终端 A:启动 GUI
./target/release/pureterm &
#    终端 B:发命令,观察终端 A 的窗口实时变化
./target/release/pureterm system ping          # → {"status":"ok"}
./target/release/pureterm system info          # → 版本/OS/tab/pane 计数
./target/release/pureterm system methods       # → 31 个 method 列表
./target/release/pureterm tab new              # → 终端 A 出现新标签
./target/release/pureterm tab list             # → JSON 标签列表
./target/release/pureterm tab switch --index 1 # → 终端 A 切到标签 1
./target/release/pureterm pane split --direction h   # → 水平分屏
./target/release/pureterm pane zoom            # → 切换 pane 缩放
./target/release/pureterm pane focus --direction next
./target/release/pureterm config reload        # → 热重载配置
./target/release/pureterm --json tab list      # → 紧凑 JSON(对比 pretty)
./target/release/pureterm ssh list-profiles    # → 已配置的 SSH Profile(unix)

# 4. 单元测试(leaf→method 映射 / 全局 flag / 裸参数守卫 / Msg 逃生舱)
cargo test -p pureterm cli::

# 5. 全量回归(确保 send_rpc 改签名未破坏既有功能)
cargo test -p pureterm
```

写操作(tab.new / pane.split / config.reload 等)经 `IpcSideEffect` 回写 iced 主循环,所以**终端 A 的 GUI 会实时变化** —— 这是验证写路径成功的直观方式。`msg` 是原始 JSON-RPC 逃生舱,与命令组走同一 socket,可用于任意 method。

---

## 配置

完整 TOML schema 详见 [Alacritty 官方文档](https://github.com/alacritty/alacritty/blob/master/extra/man/alacritty.5.scd)(PureTerm 完全兼容)。

PureTerm 扩展段:
- `[ssh.profiles]` — SSH Profile 列表
- SSH Action 在 `[keyboard.bindings]` 中可用

加载顺序:
1. `~/.config/pureterm/alacritty.toml`(PureTerm 专属,优先)
2. `~/.config/alacritty/alacritty.toml`(回退到 Alacritty 标准位置)

---

## 快捷键速查

| 动作 | macOS | 说明 |
|---|---|---|
| 新建标签 | `⌘T` | 当前工作目录 |
| 关闭标签 | `⌘W` | SSH Tab 走 graceful_close(exit + SIGHUP 兜底) |
| 切换上一/下一标签 | `⌘⇧[` / `⌘⇧]` | |
| 跳到第 N 标签 | `⌘1` … `⌘9` | |
| 水平分屏 | `⌘D` | |
| 垂直分屏 | `⌘⇧D` | |
| 等分所有分屏 | `⌘=` | 递归等分(GAP-2.2) |
| 交换 Pane | `⌘⇧X` | first/second 互换 |
| 临时最大化 Pane | `⌘⇧Enter` | 再按恢复 |
| 焦点 Next/Prev Pane | `⌘]` / `⌘[` | DFS 先序循环 |
| 移动 Pane 到新 Tab | `⌘⇧T` | |
| 移动 Pane 到新窗口 | `⌘⇧N` | |
| 关闭当前 Pane | `⌘W` | |
| 命令面板 | `⌘⇧P` | nucleo-matcher 模糊搜索 |
| SSH 连接 | `⌘⇧S` | 弹 SSH Profile 选择 |
| SSH 设置 | `Ctrl+Shift+F5`(默认) | 可在 `[keyboard.bindings]` 自定义 |
| 设置 | `⌘,` | 启动独立 Settings UI 进程 |
| Vi Copy Mode | `Ctrl+Shift+Space` | 进入 vi 模式,hjkl 移动,v 选区,y 复制,/ 搜索 |
| 搜索向前 | `⌘F` | |
| 搜索向后 | `⌘⇧F` | |

完整键盘映射可在 Settings UI → Keyboard Shortcuts 页查看与编辑。

---

## 路线图

**已实现**(2026 Q2):
- ✅ **GAP-2.2** 嵌套分屏补齐(等分 `Cmd+Shift+E` / 交换 / 缩放 / 焦点切换)
- ✅ **GAP-2.3** Socket API v2 协议骨架(JSON-RPC 2.0 + 基础 method + CLI `pureterm msg`)
- ✅ **GAP-2.5** 多 UI 路径统一治理(alacritty-runtime 精简为 glyph_atlas)
- ✅ **GAP-3.4** 状态栏(底部 24px:CWD · Git 分支 · SSH · 进程名 · 光标位置)
- ✅ **GAP-3.7** 快捷键 6 套预设模板(tmux / Vim / macos / agent-triage / vscode / custom)
- ✅ **GAP-4.1** Vi Copy Mode(ViSubMode 状态机 + 18 ViMotion + visual/block/yank + 搜索 + GPU 高亮)
- ✅ **GAP-5.7/5.8/5.9** SSH 一等公民(Tab UI + 设置面板 + 命令面板集成)
- ✅ **GAP-5.1** 持久 PTY 守护进程 `puretermd`(关 GUI 不丢会话:PTY master fd 由 pump 线程持有,1 MiB scrollback 回放,多 attach 扇出)
- 🚧 **GAP-5.6 daemon 化跨重启恢复**(2026-06-17 代码闭环,待 GUI E2E):GUI 重启后自动重连 daemon 存活会话 + 重建 tabs/分屏布局/窗口尺寸;`gui-sessions.json` 变更去抖原子写 + 关窗 flush;孤儿会话作额外 tab 接回;SSH 重连到存活 `ssh -tt` child 无需重新认证
- 🚧 **GAP-5.6** SSH reconnect/disconnect 状态机(进程内子集:网络中断自动重连 + 指数退避 + 状态栏 + `ssh.disconnect`;daemon 化跨重启恢复已实现,见上)
- ✅ **GAP-6.1** Socket API v2 完整协议(31 method / 8 命名空间 + IpcSideEffect 写操作副作用)
- ✅ **GAP-7.1/7.2** Agent Hooks 通用框架 + Claude Code 适配器自动安装(`pureterm hook` shim + `pureterm agents install`,加锁 RMW 写 `~/.claude/settings.json`)
- 🚧 **GAP-7.3/7.4/7.5/7.6** Agent 体验闭环(2026-06-22 代码闭环,254 单测绿,待 GUI E2E):Hibernation(idle 双条件 SIGSTOP)+ Feed 审批面板(延迟回复 + 5 按钮 + 作用域记忆 + feed.jsonl 审计)+ Session Restore(`claude --resume` 冷恢复)+ Vault registry(`pureterm agents detect` 进程扫描 + SessionStart 自动注册打通冷恢复 + 状态栏 `⚡/❄/⏳` 指示 + `pureterm feed list`)

**本季度焦点**(2026 Q2-Q3):
- **GAP-6.2** CLI 协议(9 大组子命令,基于 GAP-6.1 Socket API 扩展)

**重点未实现领域**(详见路线图):
- AI Agent 收尾(GAP-7.3~7.6 的 GUI E2E + dist 重打包;GAP-7.7 custom resume bindings / GAP-7.8 agent-browser 兼容)
- Browser 面板(WebView + SOCKS5 隧道)
- 双侧边栏 + Workspace 抽象层

**完整路线图**:见 [`docs/todo.md`](docs/todo.md)（待办）、[`docs/progressing.md`](docs/progressing.md)（进行中）、[`docs/done.md`](docs/done.md)（已完成）与 [`docs/reviewed.md`](docs/reviewed.md)（已 Review）。

---

## 贡献与协议

**协议**:MIT(沿用 Alacritty 上游协议)。

**Agent 指引**:
- 项目根导航:[`AGENTS.md`](AGENTS.md)
- 项目级 Agent 指令:[`CLAUDE.md`](CLAUDE.md)
- 工作区总览:[`crates/AGENTS.md`](crates/AGENTS.md)
- 各 crate 详细说明:`crates/<crate>/AGENTS.md`

**知识图谱**:`graphify-out/` 目录提供代码库知识图谱,用 `graphify query "<问题>"` 检索(详见 [`CLAUDE.md`](CLAUDE.md) graphify 段)。

**维护规则**:
- vendored Alacritty 不做无关改动,便于上游 rebase
- 新功能在 `crates/pureterm/src/` 实现,不再扩展 `pureterm-alacritty-runtime`
- 修改 vendored 源码时,在修改处加注释说明原因
