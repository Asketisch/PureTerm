<div align="center">

[![CI](https://github.com/Asketisch/PureTerm/actions/workflows/ci.yml/badge.svg)](https://github.com/Asketisch/PureTerm/actions/workflows/ci.yml)
[![Release](https://github.com/Asketisch/PureTerm/actions/workflows/release.yml/badge.svg)](https://github.com/Asketisch/PureTerm/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey)

**简体中文** · [English](README_EN.md)

</div>

# PureTerm

> **关掉窗口,会话还在。**
> 基于 Alacritty 内核 + Iced 0.14 的 GPU 加速终端,纯 Rust 编写。PTY 由独立守护进程持有 —— 关 GUI、杀进程、甚至崩溃,编译、部署、SSH 照常运行;重开自动恢复全部标签、分屏、布局与滚动历史,**不需要 tmux**。
> 在此之上:内置加密凭证库的 SSH 客户端、覆盖整个 UI 的 JSON-RPC 控制面,以及为 Claude Code 等 AI Agent 准备的 hooks 策略、审批与冷恢复 —— 全部本地执行,MIT 开源。

---

## 目录

- [为什么是 PureTerm](#为什么是-pureterm)
- [核心特性](#核心特性)
- [安装预编译二进制](#安装预编译二进制)
- [从源码构建](#从源码构建)
- [特性详解](#特性详解)
  - [会话不丢:关 Tab ≠ 退出应用](#会话不丢关-tab--退出应用)
  - [SSH 一等公民](#ssh-一等公民)
  - [AI Agent 原生](#ai-agent-原生)
  - [可编程控制面:Socket API + CLI](#可编程控制面socket-api--cli)
  - [设置与配置:Alacritty 兼容 + 热重载](#设置与配置alacritty-兼容--热重载)
  - [终端基本功](#终端基本功)
- [快捷键速查](#快捷键速查)
- [架构](#架构)
- [构建与打包](#构建与打包)
- [路线图](#路线图)
- [贡献](#贡献)

---

## 为什么是 PureTerm

三个终端用户的老问题,PureTerm 给出新答案:

**1.「终端一关,任务全丢」**

跑了一半的构建、还挂着的 SSH,随窗口一起消失。tmux 能救,但每个会话都要先 `tmux new`,断了要 `attach`,多客户端要懂 prefix —— 有真实的习惯成本。PureTerm 把这件事下沉到 PTY 层:独立守护进程 `puretermd` 持有 PTY master fd,GUI 只是它的一个客户端。关 GUI、杀进程、甚至崩溃,会话照常跑;重开自动接回 —— **没有 tmux,也没有新习惯要学**。

**2.「SSH 工具链是拼出来的」**

Host 散在 `~/.ssh/config`,密码在脑子里或明文里,断了线整个流程重来。PureTerm 把 SSH 做成一等公民:Profile + AES-256-GCM / Argon2id 加密凭证库 + ControlMaster 多标签共享连接 + 断线指数退避自动重连(滚动历史不丢),`⌘⇧S` 一跳到底。

**3.「2026 年,终端要为 AI Agent 服务」**

Claude Code 们每天在你的终端里执行成百上千条命令 —— 谁来放行、拦截、审计?多数终端把这个问题留给你手动拼脚本。PureTerm 原生回答:hooks 策略中间件(正则 deny `rm -rf`、外接审批脚本)、`pureterm agents install claude-code` 一键接入、状态栏实时显示 agent 状态、关机重开自动 `claude --resume`,每条决策落在本地审计日志。面向 AI 的终端不少,但把 agent 策略**放在本地、开源可审**的不多 —— PureTerm 不上传任何东西。

### 与常见终端 / 多路复用器对比

| 能力 | **PureTerm** | Alacritty | iTerm2 | Kitty | WezTerm | tmux |
|---|---|---|---|---|---|---|
| GPU 渲染 | ✅ wgpu(Metal / Vulkan / DX12) | ✅ | ✅ Metal | ✅ | ✅ | ➖ |
| 标签 + 嵌套分屏 | ✅ | ❌ | ✅ | ✅ | ✅ | ✅ |
| GUI 关闭 / 崩溃后**会话存活** | ✅ 守护进程持有 PTY,零习惯成本 | ❌ | ⚠️ 进程随 GUI 消失 | ❌ | ❌ | ✅(需把工作包进 tmux) |
| 重开恢复标签 / 分屏 / 布局 / 滚动历史 | ✅ 本地 + SSH | ❌ | ⚠️ 仅窗口布局 | ⚠️ 需预定义 session,进程不存活 | ❌(社区插件) | ✅ |
| SSH:加密凭证 + ControlMaster + 断线自动重连 | ✅ 内置 | ❌ | ⚠️ 钥匙串存密码 | ❌ | ❌ | ❌ |
| AI Agent:hooks 策略 / 一键适配 / 冷恢复 / 审计 | ✅ 原生 | ❌ | ❌ | ❌ | ❌ | ❌ |
| 程序化控制(JSON-RPC + CLI) | ✅ 31 method / 8 命名空间 | ❌ | ⚠️ Python API | ⚠️ remote control | ⚠️ Lua 插件 API | ✅ |
| GUI 设置界面(格式保留写回配置) | ✅ 独立进程 | ❌ | ✅ | ❌ | ❌ | ❌ |
| 兼容 Alacritty TOML 配置 | ✅ | ✅(本尊) | ➖ | ➖ | ➖ | ➖ |
| macOS / Linux / Windows | ✅ | ✅ | macOS 专属 | macOS / Linux | ✅ | ✅ |

> Alacritty 是纯内核哲学(无标签、无分屏、无设置界面);PureTerm 以它为内核,补齐了完整 GUI 层 —— 你现有的 `alacritty.toml` 可以原样迁移。

---

## 核心特性

- 🔒 **会话不丢** — `puretermd` 守护进程持有 PTY master fd:GUI 关闭 / 崩溃不中断任务;重开自动恢复标签顺序、分屏树、窗口尺寸与滚动历史,SSH 重连免再认证
- 🛡 **SSH 一等公民** — ControlMaster 多标签共享连接、`~/.ssh/config` 直接可用、AES-256-GCM + Argon2id 加密凭证、断线指数退避自动重连(保留滚动历史)
- 🤖 **AI Agent 原生** — hooks 策略中间件(拦截 `rm -rf` 等)、`pureterm agents install claude-code` 一键接入、状态栏 agent 指示、`claude --resume` 冷恢复、本地审计日志
- ⌨️ **可编程控制面** — JSON-RPC 2.0 over Unix domain socket,31 个 method;`pureterm tab new` / `pane split` 等结构化 CLI,脚本与自动化友好
- 🎨 **GPU 渲染 + Alacritty 兼容** — wgpu 自定义 shader(Metal / Vulkan / DX12);标准 `alacritty.toml` 直接用,独立设置进程改完即热重载
- 🧩 **更多** — 命令面板(`⌘⇧P` 模糊搜索)、分屏全家桶(等分 / 交换 / 最大化 / 跨 Tab 移动)、Vi 复制模式、智能选择 9 规则、6 套快捷键预设、自定义命令 snippet、状态栏(CWD / Git / SSH / agent / 光标)

---

## 安装预编译二进制

推送 `v*` tag 后,CI 自动构建并在 [GitHub Releases](../../releases) 发布全平台产物,每个版本提供:

| 平台 | 产物 | 安装方式 |
|---|---|---|
| macOS Apple Silicon | `PureTerm-<v>-arm64.dmg`(或 `.app.zip`) | 打开 .dmg,拖入「应用程序」 |
| macOS Intel | `PureTerm-<v>-x64.dmg`(或 `.app.zip`) | 同上 |
| Linux x86_64 | `pureterm-<v>-x64-gnu.tar.gz` | 解压,目录树含 `bin/` + `share/`,可整体放入 `/usr/local` |
| Linux aarch64 | `pureterm-<v>-arm64-gnu.tar.gz` | 同上 |
| Windows x86_64 | `pureterm-<v>-x64.zip` | 解压后将 `bin` 目录加入 `PATH` |

产物包内含三个可执行文件:`pureterm`(主程序)、`pureterm-settings`(设置 UI)、`puretermd`(会话守护进程),三者需在同一目录。所有产物另附 `SHA256SUMS` 校验和。

注意:Linux 产物在 Ubuntu 22.04(x86_64)/ 24.04(aarch64)上链接,依赖较新的 glibc;更老的发行版请走源码构建。

## 从源码构建

```bash
git clone https://github.com/Asketisch/PureTerm && cd PureTerm
cargo build --release
./target/release/pureterm
```

配置文件:`~/.config/pureterm/alacritty.toml`(回退 `~/.config/alacritty/alacritty.toml`)。
跨平台 / 跨目标打包见 [构建与打包](#构建与打包)。

---

## 特性详解

### 会话不丢:关 Tab ≠ 退出应用

`puretermd` 是单例守护进程(flock + setsid 脱离 GUI 进程组),每个会话的 PTY master fd 由其 pump 线程持有,滚动缓冲为每会话 1 MiB 环形队列。GUI 只是 daemon 的一个客户端 —— 支持多客户端同时 attach 到同一会话。

恢复范围:GUI 重启后,tab 顺序、标题、分屏树(方向 + 比例)、窗口尺寸、active tab、可见滚动区(由环形缓冲回放重建);孤儿会话(daemon 有、快照无)作为额外 tab 接回。SSH 会话重连到存活的 `ssh -tt` child,**无需重新认证**。

关闭语义是刻意区分的:

| 操作 | 会话结局 | 下次启动 |
|---|---|---|
| `⌘W` 关闭 tab / pane | exit → SIGHUP,结束该 daemon 会话 | **不**恢复 |
| 退出 app(`⌘Q` / 关窗 / 杀进程) | 全部会话保留、继续运行 | 自动恢复全部布局与滚动历史 |

即:「关 tab = 不要这个会话;退出 = 暂时离开」。布局快照以去抖 + 原子写落盘;daemon 不可用时回落为普通单 tab 启动,零回归。

### SSH 一等公民

- **Profile** 在 `alacritty.toml` 声明,`⌘⇧S` 或命令面板输入 `ssh` 一跳连接;`~/.ssh/config` 的 Host 别名直接可用
- **ControlMaster** socket 复用,多标签共享同一条 SSH 连接
- **加密凭证库**:AES-256-GCM + Argon2id,master key 以 `Zeroizing` 包裹,退出即擦除
- **断线自动重连**:网络中断后指数退避(3→60s × 8),保留滚动历史;状态栏显示 `SSH` / `SSH ↻ n` / `SSH ✗`;`pureterm ssh disconnect` 主动断开不触发重连

```toml
[[ssh.profiles]]
name = "生产服务器"
host = "192.168.1.100"
user = "admin"

[[ssh.profiles]]
name = "跳板机"
host = "bastion"
key_path = "~/.ssh/bastion_key"
```

### AI Agent 原生

PureTerm 充当 Claude Code 等 agent 的 **hooks 策略中间件**:agent 在每次工具调用前把事件交给 `pureterm hook <event>`,PureTerm 按配置评估并按 Claude Code hooks 协议返回放行 / 拦截决策。Standalone 设计 —— 零 IPC 往返,每次工具调用只读本地配置。

```toml
[[agent.hook]]
event = "PreToolUse"
matcher = "Bash"                # 正则匹配工具名(仅 Pre/PostToolUse)
deny_pattern = "\\brm\\s+-rf"   # 命中 → deny
deny_reason = "rm -rf blocked by policy"

[[agent.hook]]
event = "PostToolUse"
matcher = "Write|Edit"
command = "/abs/path/lint.sh"   # 外部脚本:stdin 收事件 JSON,stdout/exit code = 决策
timeout = 30
```

```bash
pureterm agents install claude-code        # 一键注册进 ~/.claude/settings.json(加锁原子写、幂等,保留既有内容)
pureterm agents install claude-code --dry-run
pureterm agents detect                     # 扫描本机正在运行的 agent 会话
pureterm feed list                         # 审批 / 决策审计日志(feed.jsonl)
```

- 事件对齐官方协议:`PreToolUse` / `PostToolUse` / `UserPromptSubmit` / `Notification` / `Stop` / `SubagentStop` / `PreCompact` / `SessionStart` / `SessionEnd`
- **冷恢复**:配置 `[[vault.agents]]` 后,agent session id 持久化;重开 PureTerm 自动提示 / 执行 `claude --resume`,还可用进程扫描发现 PureTerm 之外启动的 agent
- **状态栏 agent 指示**:`⚡ kind`(活跃)/ `❄ kind`(休眠)/ `⏳ N`(N 条审批待决)
- 全部本地执行:策略本地评估、审计落本地文件、凭证本地加密 —— 无云依赖,MIT 可审

### 可编程控制面:Socket API + CLI

PureTerm 运行时在 Unix domain socket 上暴露 **JSON-RPC 2.0** 接口:8 个命名空间(`system` / `window` / `tab` / `pane` / `surface` / `config` / `ssh` / `debug`)、31 个 method,查询与写操作俱全 —— 写操作直接驱动 GUI(`tab.new` 真的开新标签)。CLI 自动发现 socket:

```bash
pureterm tab new                   # 新标签
pureterm tab switch --index 1
pureterm pane split --direction h  # 水平分屏
pureterm pane zoom
pureterm ssh list-profiles
pureterm config reload             # 热重载配置
pureterm system methods            # 列出全部 method

pureterm --json tab list           # 紧凑 JSON 输出
pureterm --socket /tmp/x.sock system ping                   # 显式指定 socket
pureterm msg pane.split '{"direction":"horizontal"}'        # 原始 JSON-RPC 逃生舱(任意 method)
```

### 设置与配置:Alacritty 兼容 + 热重载

- 配置文件 `~/.config/pureterm/alacritty.toml`,**完全兼容标准 Alacritty TOML schema** —— 现有配置原样迁移,回退路径 `~/.config/alacritty/alacritty.toml` 也支持
- `⌘,` 启动**独立设置进程** `pureterm-settings`(6 个分类页:General / Appearance / Colors / Terminal / SSH Profiles / Keyboard Shortcuts);修改经 `toml_edit` 格式保留 + 原子写回,主进程监听文件变化自动热重载
- 快捷键:31+ 个 Action 可在 TOML 自定义,6 套预设模板(vim / tmux / macos / vscode / agent-triage),CLI 管理(`pureterm shortcuts preset vim` / `reset`),存盘即热重载
- **自定义命令**:把常用 shell 片段做成「绑快捷键 + 面板可搜 + 一键触发」,在当前 pane 执行、输出可见:

  ```toml
  [[commands]]
  id = "deploy.staging"
  title = "Deploy to Staging"
  key = "Cmd+Shift+D"          # 可选;不写则仅命令面板可搜
  command = "ssh staging 'cd app && ./deploy.sh'"
  ```

### 终端基本功

- **渲染**:Iced 0.14 + 自定义 wgpu shader;Metal / Vulkan / DX12 自动选择;字体图集 LRU + 脏区域;文字模糊处理改善低分屏阅读
- **分屏全家桶**:嵌套水平 / 垂直分屏、拖拽调比例、等分 `⌘=`、交换 `⌘⇧X`、临时最大化 `⌘⇧Enter`、焦点 DFS 循环 `⌘]` / `⌘[`、移动到新 Tab / 新窗口 —— 键盘、右键菜单、命令面板三入口
- **Vi 复制模式**:`Ctrl+Shift+Space` 进入,hjkl 导航、v/V/Ctrl+v 选区、y 复制、`/` `?` 搜索,GPU 渲染 vi 光标与搜索高亮
- **智能选择 9 规则**:URL / SSH URL / 邮箱 / 引号字符串 / 文件路径 / C++ namespace / dotted include / Git SHA / IPv4
- **命令面板**:`⌘⇧P`,nucleo 模糊搜索,Actions / SSH Profiles 双模式
- **状态栏**:工作目录(OSC 7)· Git 分支 · SSH 指示 · 前台进程 · agent 指示 · 光标位置
- 完整 VTE 解析(CSI / ESC / OSC / DCS / APC / PM)、IME 输入法候选

---

## 快捷键速查

以 macOS 为例;完整映射可在 Settings → Keyboard Shortcuts 查看与编辑。

| 动作 | 按键 | 说明 |
|---|---|---|
| 新建标签 | `⌘T` | 继承当前工作目录 |
| 关闭标签 / pane | `⌘W` | SSH tab 走优雅关闭(exit + SIGHUP 兜底) |
| 切换上 / 下一标签 | `⌘⇧[` / `⌘⇧]` | |
| 跳到第 N 标签 | `⌘1` … `⌘9` | |
| 水平 / 垂直分屏 | `⌘D` / `⌘⇧D` | |
| 等分所有分屏 | `⌘=` | 递归等分 |
| 交换 Pane | `⌘⇧X` | |
| 临时最大化 Pane | `⌘⇧Enter` | 再按恢复 |
| 焦点下 / 上一个 Pane | `⌘]` / `⌘[` | DFS 先序循环 |
| 移动 Pane 到新 Tab / 新窗口 | `⌘⇧T` / `⌘⇧N` | |
| 命令面板 | `⌘⇧P` | nucleo 模糊搜索 |
| SSH 连接 | `⌘⇧S` | 弹 SSH Profile 选择 |
| 设置 | `⌘,` | 启动独立 Settings UI 进程 |
| Vi 复制模式 | `Ctrl+Shift+Space` | hjkl 移动,v 选区,y 复制,`/` 搜索 |
| 搜索向前 / 向后 | `⌘F` / `⌘⇧F` | |

---

## 架构

PureTerm 是一个 Rust workspace:

| Crate | 用途 |
|---|---|
| `pureterm` | 主二进制,Iced 0.14 daemon —— 标签 / 分屏 / SSH / 命令面板 / IPC / agent hooks 均在主路径 |
| `pureterm-settings` | 独立设置 UI 进程(Iced daemon),`⌘,` 启动 |
| `pureterm-daemon` | `puretermd` 会话守护进程 —— 持有 PTY master fd,GUI 关闭后会话存活 |
| `pureterm-ssh` | SSH 后端:ControlMaster 复用、加密凭证库、断线重连 |
| `alacritty_terminal`(+ `alacritty_config*`) | vendored 上游 —— 终端内核 / PTY / Grid / VTE 解析 |
| 其余(`pureterm-jsonrpc` / `pureterm-lsp` / `pureterm-codeindex` / …) | IPC 协议、LSP、代码索引等支撑 |

维护规则:新功能一律在 `crates/pureterm/src/` 主路径实现;vendored Alacritty 不做无关改动(便于上游同步),确需修改处在代码注释中说明原因。详见 [`crates/AGENTS.md`](crates/AGENTS.md)。

---

## 构建与打包

所有跨平台 / 跨目标工作统一通过 `./build.sh` 完成,同一脚本可产出
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

- 同时构建 `aarch64-apple-darwin` + `x86_64-apple-darwin` 两个目标,用 `lipo` 合并成单个 universal2 Mach-O,Apple Silicon 与 Intel Mac 原生运行。
- 前提:同时安装两个 rustup 目标,且主机必须是 macOS。
- 工件名带 `-universal` 后缀以与单架构产物区分。

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

打包脚本在每个工具缺失时优雅降级,不会因为缺 `hdiutil` 就拒绝构建:

| 工具 | 用途 | 缺失时的行为 |
|---|---|---|
| `sccache` | 编译缓存 | 静默降级,直接调用 rustc |
| `cargo-nextest` | 并行测试 | 回退到 `cargo test` |
| `mold` / `lld` | Linux 链接加速 | 自动用系统 ld |
| `hdiutil` | macOS .dmg | 跳过 .dmg,仍产出 .app.zip |
| `codesign` + `CODESIGN_IDENTITY` | macOS 代码签名 | 跳过签名 |
| `xcrun notarytool` + `APPLE_ID` 等 | macOS 公证 | 跳过公证 |
| `zstd` | Linux .tar.zst | 跳过 zst 格式 |
| `zip` / `powershell` | Windows .zip | 二者皆无时报错 |
| `signtool` + 证书环境变量 | Windows 代码签名 | 跳过签名 |

### 跨平台构建矩阵

`./build.sh matrix` 遍历以下 8 个目标,跳过主机上没有的 rustup 目标与无法交叉编译的目标;`--auto-install-targets` 允许自动 `rustup target add`:

| 主机 | x86_64 | aarch64 |
|---|---|---|
| macOS | `x86_64-apple-darwin` | `aarch64-apple-darwin` |
| Linux (gnu) | `x86_64-unknown-linux-gnu` | `aarch64-unknown-linux-gnu` |
| Linux (musl, 完全静态) | `x86_64-unknown-linux-musl` | `aarch64-unknown-linux-musl` |
| Windows (MSVC) | `x86_64-pc-windows-msvc` | `aarch64-pc-windows-msvc` |

### CI

- [`ci.yml`](.github/workflows/ci.yml) — push / PR 到 `main` 时,在 `ubuntu-22.04`、
  `macos-14`、`windows-2022` 三个 runner 上跑 `cargo build` + `cargo test`。
  clippy 另设 advisory job,只报告不阻断(仓库有历史 lint 积压,
  且 vendored 的 `alacritty_*` 不做无关改动)。
- [`release.yml`](.github/workflows/release.yml) — 推送 `v*` tag 自动构建 5 个
  target(macOS arm64/x64、Linux x64/arm64、Windows x64)并发布 GitHub Release
  (附 `SHA256SUMS`);tag 名须与 `crates/pureterm/Cargo.toml` 版本一致。
  也可在 Actions 页手动 `workflow_dispatch` 试跑:只构建并上传 5 平台产物与
  校验和,**不创建 Release**,适合上线前验证。

两个工作流都自行处理 CI 环境差异:Windows 安装 NASM(`aws-lc-rs` 需要)、
Linux 用 `RUSTFLAGS` 把 `.cargo/config.toml` 的 `-fuse-ld=mold` 覆盖为 runner
自带的 `lld`。

### 未实现的打包格式

`.deb` / `.rpm` / `.AppImage` / `.msi` / NSIS 暂未提供(v1 以 `.tar.gz` / `.zip` / `.dmg` 便携包为主)。需要请开 issue。

---

## 路线图

**已完成**(2026 Q2):会话持久守护进程 + 跨重启恢复、SSH 一等公民(后端 / 凭证库 / 设置面板 / 自动重连)、Socket API v2 + 结构化 CLI、分屏全家桶、Vi 复制模式、状态栏、命令面板、Agent Hooks 框架 + Claude Code 适配器一键安装、主题系统与跨平台标题栏适配。

**进行中**:Agent 体验闭环的 GUI 端到端验证(审批面板 / 休眠 / 冷恢复 / 外部 agent 发现)、更多安装包格式(`.deb` / `.rpm` / `.msi`)。

**计划**:Browser 面板(WebView + SOCKS5 隧道)、双侧边栏 + Workspace 抽象、agent 适配器扩展(Codex / OpenCode 等)。

完整路线图见 [`docs/todo.md`](docs/todo.md)(待办)、[`docs/progressing.md`](docs/progressing.md)(进行中)、[`docs/done.md`](docs/done.md)(已完成)。

---

## 贡献

欢迎 issue 与 PR,参见 [`CONTRIBUTING.md`](CONTRIBUTING.md)。

给(AI 辅助)贡献者的项目导航:[`AGENTS.md`](AGENTS.md)、[`CLAUDE.md`](CLAUDE.md)、[`crates/AGENTS.md`](crates/AGENTS.md)。

三条硬规则:

1. vendored Alacritty 源码不做无关格式化或重构,便于上游同步
2. 新功能在主路径 `crates/pureterm/src/` 实现
3. 修改 vendored 源码时,在修改处加注释说明原因

## 协议与致谢

MIT LICENSE。终端内核基于 [Alacritty](https://github.com/alacritty/alacritty)(vendored,Apache-2.0 / MIT);GUI 基于 [iced](https://github.com/iced-rs/iced);渲染基于 [wgpu](https://github.com/gfx-rs/wgpu);模糊搜索基于 [nucleo](https://github.com/helix-editor/nucleo)。感谢这些优秀的开源项目。
