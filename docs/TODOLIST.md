# PureTerm 路线图与待办清单

> 未实现 / 进行中特性追踪。已闭环(✅)特性见 [`FEATURES.md`](../FEATURES.md)。

---

## 总览

| 状态 | 数量 | 说明 |
|---|---|---|
| ❌ 未实现 | 13 | 待排期 |
| 🚧 进行中 | 5 | 代码闭环待 E2E |
| ⛔ 不做 | 4 | 见下方 §拒绝 |

### AI 终端价值排序

> 以 AI agent 工作流(自动化、浏览器控制、MCP 集成)为核心视角重排优先级。

| 价值层 | 优先级 | 特性 |
|--------|--------|------|
| **Tier 0 — AI 核心管道** | P0 | GAP-5.6(SSH 重连) / GAP-6.7(事件流) |
| **Tier 1 — AI 差异化能力** | P1 | GAP-7.8(agent-browser) / GAP-5.2(SOCKS5 隧道) / GAP-5.4(SSH agent forwarding) |
| **Tier 2 — 终端增强** | P2 | GAP-4.2(hover preview) / GAP-5.5(URL scheme) / GAP-1.3(标题栈) / GAP-5.3(PTY resize) / GAP-3.2(可编程 Sidebar DSL) / GAP-6.3(AppleScript 桥) / GAP-6.8(Sidebar 解释器 IPC) |
| **Tier 3 — 兼容性/国际化** | P3 | GAP-3.1(双侧边栏) / GAP-1.2(DECSCL) / GAP-1.4(BiDi) |
| **Tier 4 — 视觉/分发** | P4 | GAP-3.6(Glass) / GAP-6.5(Dock) / GAP-4.6(VoiceOver) / GAP-8.1(auto-update) |

---

## B. UI/UX

### GAP-3.5 内置 Browser 面板(WebView)
- **状态**: ✅ | **优先级**: P0
- **现状**: 代码闭环 + 自动 harness 全绿(2026-06-25)——Phase A(`crates/pureterm/src/browser/` 自实现 wry 嵌入:WKWebView 作 content NSView 子视图 `build_as_child`;几何真值=split_tree layout,reposition pass 按 `mark_layout_dirty` + tab 切换 + 拖拽 burst 触发;快照 `PaneSnapshot.kind="web"` + `#[serde(default)] url` 前向兼容)+ Phase C(per-view SOCKS5 proxy 绑定,wry `mac-proxy`)+ GAP-7.8(`browser.*` ops / MCP / screenshot)。**本轮补齐用户交互层**:`⌘⇧L` 分出新 Web pane 并聚焦地址栏 / `⌘L` 地址栏 overlay(导航活跃 Web pane,无则新建)+ `normalize_url_input`(域名/scheme/搜索归一化)。CI:`browser_address_bar_e2e.rs`(spawn 序列 Arc 身份 + load_url 未 realize 即更新)+ `normalize_url_input` 单测 + keymap `⌘L`/`⌘⇧L`(双形)解析,共 442 绿。pixel smoke 见 [`docs/manual-e2e/gap-3.5-browser.md`](manual-e2e/gap-3.5-browser.md)(异步非 gate)
- **目标**: Pane 中嵌入 WebView — `⌘⇧L` split 打开 / `⌘L` 聚焦地址栏 / `⌘[ ]` 导航;支持 SOCKS5/HTTP CONNECT proxy(绑定 GAP-5.2);跨平台用 `wry` crate
- **依赖**: GAP-2.2(分屏)
- **DoD**: Pane 中打开 `https://example.com`,可渲染和滚动
- **未做**: 后退/前进导航(`⌘[`/`⌘]` 与 pane 焦点切换冲突,有意 defer) / Linux webkit2gtk 原生嵌入(现仅 macOS,Linux 走 open-in-browser 兜底) / detached 多窗口 WebView 精确 NSWindow 匹配
- **对标**: cmux `Sources/BrowserWindowPortal.swift`(173 KB)

### GAP-3.2 可编程 Sidebars(DSL 解释器)
- **状态**: ❌ | **优先级**: P2
- **目标**: 用 `rhai` crate 或 `wasmtime`;sidebar 文件声明触发器 + 渲染回调,DSL 注册到事件总线;沙箱(资源限制 + 权限隔离)
- **依赖**: GAP-3.1(侧边栏框架) + GAP-6.7(事件流)
- **DoD**: 写一个 `~/.config/pureterm/sidebars/show-git.rhai`,实时显示当前 git 状态
- **对标**: cmux `Sources/Sidebar/Interpreter/`

### GAP-3.1 双侧边栏
- **状态**: ❌ | **优先级**: P3
- **目标**: 可隐藏双侧边栏 — 左侧 Workspace + Git 分支(`git2` crate 30s 轮询);右侧多 mode(files / find / sessions / feed);宽度可拖拽(默认 240px,范围 120-480)
- **依赖**: GAP-2.1(Workspace 抽象)
- **DoD**: `Cmd+B` 显示左侧栏,显示工作目录 git 状态 + Workspace 列表
- **对标**: cmux `Sources/Sidebar/`

### GAP-3.6 macOS Glass 效果
- **状态**: ❌ | **优先级**: P4
- **目标**: macOS 26 用 `NSGlassEffectView`,macOS 13-15 回退 `NSVisualEffectView`;窗口 `setOpaque(false)` + 主背景 alpha=0
- **DoD**: macOS 26 启用 Glass,窗口显示半透明玻璃效果
- **对标**: cmux `Sources/GlassEffectView.swift`

---

## C. 终端交互

### GAP-4.2 智能选择 hover preview
- **状态**: 🚧 | **优先级**: P2
- **现状**: 9 规则智能选择已实现(URL/SSH URL/邮箱/引号/路径/C++ namespace/dotted include/Git SHA/IPv4),**无 hover preview**
- **目标**: 鼠标悬停 URL/路径 显示 tooltip — 200ms 延迟,iced `Tooltip` widget;URL preview(`reqwest` 抓 HTML title + favicon),路径 preview(`std::fs::metadata` + `ignore` crate);右键选择后弹出动作菜单(打开/复制/编辑器打开)
- **依赖**: GAP-3.1(侧边栏渲染基础)
- **DoD**: 鼠标悬停 URL 1 秒,显示 tooltip 含页面标题
- **对标**: cmux `Sources/SemanticHistory.swift` + iTerm2 `iTermSemanticHistoryController.m`

### GAP-1.3 标题栈(CSI 22/23t)
- **状态**: ❌ | **优先级**: P2
- **目标**: 实现 CSI 22t(push title)/ 23t(pop title)
- **DoD**: `echo -e "\e[22;0t" && echo -e "\e[23;0t"` 标题栈正常 push/pop
- **对标**: Ghostty `src/terminal/Terminal.zig`

### GAP-5.3 smallest-screen-wins PTY resize
- **状态**: ❌ | **优先级**: P2
- **目标**: 多 Pane attach 同一 SSH session 时,PTY 大小 = min(行列);`SshSessionManager` 维护 `attached_surfaces`
- **依赖**: GAP-5.1
- **DoD**: 2 个 Pane 共享 SSH session,PTY 大小 = 较小者
- **对标**: cmux 多 surface 共享 SSH PTY

### GAP-1.2 DECSCL 设备控制
- **状态**: ❌ | **优先级**: P3
- **目标**: 实现 DECSCL(Set Conformance Level)CSI 序列处理
- **DoD**: VT100/VT200/VT300/VT400 conformance level 切换正常
- **对标**: Ghostty `src/terminal/modes.zig`

### GAP-1.4 BiDi 双向文本
- **状态**: ❌ | **优先级**: P3
- **目标**: 阿拉伯文/希伯来文 RTL 渲染;`unicode-bidi` crate;BiDi 重排序与 HarfBuzz 整形联动
- **依赖**: 无强依赖,但与字体回退一同设计更好
- **DoD**: 输入 RTL 字符串显示正确
- **对标**: Ghostty `src/unicode/`

### GAP-4.6 Touch Bar / VoiceOver
- **状态**: ❌ | **优先级**: P4
- **目标**: VoiceOver 所有 UI 元素可读(iced `accessibility` + `widget::accessible()`);Touch Bar(macOS `NSTouchBar` via `objc2`,Apple 已弱化优先级)
- **DoD**: VoiceOver 开启后,Tab 切换、命令面板弹出都有语音提示
- **对标**: Ghostty `Surface.zig` 完全 VoiceOver,iTerm2 `Accessibility/`

---

## D. SSH

### GAP-5.6 完整 reconnect/disconnect 状态机
- **状态**: 🚧 | **优先级**: P0
- **现状**: 进程内子集闭环(2026-06-14)+ daemon 跨重启恢复(2026-06-17)。**4 项收尾代码闭环(2026-06-25,待 GUI 真机 E2E)**:① `Authenticating` 独立状态——利用"`spawn_pty` fork 后即返回、认证在 ssh 子进程异步"的事实,`Spawned`→reattach→`Authenticating`,首个远端字节(`TerminalEvent::Wakeup`)→`Connected`(修正原先 spawn Ok 即 `Connected` 的过早判定);② Socket API event `ssh.state_changed`(复用 `Category::Session`,5 切换点 emit)+ 只读 `ssh.state` RPC(快照 `SshPaneState`)+ `pureterm ssh state` CLI;③ 多 pane tab SSH 退出重连——撤销 `pane_count<=1` 门控 + 遍历本 tick 全部 `exited_panes`,SSH 非用户发起退出任意 pane_count 都 `trigger_ssh_reconnect`(保留 split);④ 重连成功分隔线——`inject_reconnect_marker`(`ansi::Processor::advance` 向 alacritty 网格注入持久 `── Reconnected ──`)。外加 stderr 分类器 `classify_ssh_failure`(Auth/Network/HostKey/Unknown)→ 带类型 `Failed{kind}` + 分类后对话框。451 harness 绿(+7 新单测)。**待 dist `.app` 重打包 + GUI 真机 E2E**
- **未做**: GUI 真机 E2E 验证(我无法跑 GUI,是 flip ✅ 门槛;runbook 见 [`docs/manual-e2e/gap-5.6-ssh-reconnect.md`](manual-e2e/gap-5.6-ssh-reconnect.md)) / 初始连接路径也接入 `Authenticating`(现仅重连路径用;为保分隔线"仅重连注入"的干净语义有意延后)
- **依赖**: GAP-5.1(daemon 化);进程内子集无新依赖
- **对标**: cmux 完整事件流状态机;iTerm2 / Termius

### GAP-5.2 SOCKS5/HTTP CONNECT Browser Tunnel
- **状态**: 🚧 | **优先级**: P1
- **现状**: 代码闭环(2026-06-23,Phase B)——`puretermd` 持有 russh 0.60 SSH 会话 + 本地 SOCKS5 listener(手写 ~120 LOC,no-auth + CONNECT-only,非 fast-socks5,`atyp=3` domain 原样转给远端解析 DNS);`TunnelManager` + per-tunnel async task(russh `Handle` 非 Sync,包 `Arc<tokio::Mutex>`,只在 `channel_open_direct_tcpip` 那一下持锁);状态机 connecting→connected/need_auth/failed;identity(publickey)/password/`oneshot`-parked need_auth 三路认证;keepalive 30s×3(R6 死网关);`tunnel.{start,list,stop,provide_auth}` JSON-RPC + `ssh tunnel/tunnel-stop/tunnel-list` CLI(daemon-direct)。**与 PTY SSH 双轨**(不能复用 ControlMaster socket)。**待 dist .app 重打包 + 真实 SSH 网关 E2E**
- **目标**: SSH 创建本地 SOCKS5/HTTP CONNECT 代理;CLI `pureterm ssh tunnel user@host --local-port 1080 --socks5`;`tokio` 写 `Socks5Server`,转发到 `russh::Channel`
- **依赖**: GAP-5.1
- **DoD**: `curl --socks5 localhost:1080 example.com` 经 SSH 代理成功
- **未做**: `tunnels` 段持久化进 gui-sessions.json(跨 daemon 重启)/ TOFU known_hosts(`check_server_key` MVP 全接受)/ SSH agent forwarding 证书认证
- **对标**: cmux `Sources/SSH/Tunnel.swift`

### GAP-5.4 SSH agent forwarding
- **状态**: ✅ | **优先级**: P1
- **现状**: 双轨实现 + **Track A DoD 已闭环**(2026-06-25,本地 sshd 真机 E2E)。**Track A(PTY ssh,闭 DoD)**:`SshCommandBuilder.with_forward_agent`→`-o ForwardAgent=yes`;`SshPty::spawn` 显式注入 `SSH_AUTH_SOCK`;新增 `pureterm-ssh::agent_sock`(优先级:配置覆盖 > 继承 env > macOS `launchctl getenv` 回退,`resolve_pure` 纯函数 7 单测);沿 daemon `create_ssh`→ `session.create` RPC → `daemon_client::create_ssh` → GUI `SshProfile.forward_agent`(serde default false)+ 直连 fallback + CLI `ssh connect -A`。**Track A E2E 证据**:起本地 sshd + 临时 key → 跑 PureTerm **`build()` 原样 argv**(`-tt … -o ExitOnForwardFailure=yes -o ForwardAgent=yes -l admin -p 2222 host`)→ 远端 `ssh-add -l` 见转发 key(指纹一致)、ssh exit 0(`ExitOnForwardFailure=yes` 覆盖 agent 转发,0=服务器已接受);对照 `ForwardAgent=no` 远端无 `SSH_AUTH_SOCK`、`ssh-add -l` 失败。远程 `git push` 经同一转发 agent 即免密 = DoD。**Track B(daemon russh)**:`tunnel.rs` `ClientHandler` 带 `agent_sock`、`run_tunnel` 认证后 `channel_open_session`+`Channel::agent_forward`、重写 `server_channel_open_agent_forward` 做转发通道↔Unix socket 字节泵;`authenticate()` 加 `try_agent_auth`(`authenticate_publickey_with`+`AgentClient` 作 `Signer`,无 `--identity` 免密认证隧道);`tunnel.start` RPC + CLI `ssh tunnel -A`。代码闭环 + 编译过(核对 russh 0.60.3 API);**Track B 真机 E2E 待补**(需重启运行中的 puretermd 才能验新 daemon 的 `-A`,非 DoD,与 GAP-5.2 同属"待真机网关验证")。442(harness)+ 21(ssh)绿,daemon 54/55(1 例 `instance_lock` 因运行中的 puretermd 占锁,环境非回归)。
- **目标**: 自动检测 `$SSH_AUTH_SOCK`;`-o ForwardAgent=yes`;`russh::Agent` 协议;转发到 SSH channel
- **依赖**: GAP-5.1
- **DoD**: `ssh-add` 后远程 `git push` 不需输密码
- **未做**: Track B(daemon russh)真机 E2E(需重启 puretermd 验新 daemon 的 `-A`;非 DoD) / 跨 reconnect 保透传(需把 `forward_agent` 存进 `SshCtx`) / `ssh.agent_sock` 配置覆盖的 GUI 读取接线(resolver 已就绪,各 spawn 点暂传 None) / 对存活隧道动态开关 agent 转发(需 `Arc<AsyncMutex<Handle>>` 上提进 `TunnelEntry`)
- **对标**: cmux 标准 ForwardAgent

### GAP-5.5 `pureterm://` URL scheme
- **状态**: ❌ | **优先级**: P2
- **目标**: 注册 `pureterm://` 到 OS:macOS `Info.plist` + `LSSetDefaultHandlerForURLScheme` / Linux `.desktop` / Windows 注册表
- **DoD**: 浏览器点 `pureterm://ssh/test@127.0.0.1:22` 唤起 PureTerm 并打开 SSH Tab
- **对标**: cmux `cmux://`

---

## E. 集成与可编程

### GAP-6.7 事件流 + 持久 JSONL 日志
- **状态**: 🚧 | **优先级**: P0
- **现状**: 代码闭环 + 270 单测全绿 + release 编译通过 + CLI 冒烟通过;**待 dist .app 重打包 + 手动 GUI E2E**
- **架构**: `EventBus` 双写(events.jsonl 16MiB rotation + broadcast 4096);12 类事件;实时=文件 tail(`pureterm events list/tail`);~20 emit 点(feed/tab/pane/session/config/agent)
- **DoD**: `./build.sh release` 重打包 → 启 GUI + Claude Code 触发审批 → 另开终端 `pureterm events tail --type feed` 实时见事件
- **后续**: `events.subscribe` streaming / `notification.*` producer / workspace/sidebar/browser producer / kqueue/inotify 优化
- **对标**: cmux `Sources/EventStream.swift`

### GAP-6.3 URL scheme / AppleScript 等价桥
- **状态**: ❌ | **优先级**: P2
- **目标**: macOS AppleScript 桥(`osascript` via `objc2` + `NSAppleEventManager`);Linux D-Bus(`zbus` crate);Windows COM
- **依赖**: GAP-5.5
- **DoD**: macOS `osascript -e 'tell application "PureTerm" to create ssh tab'` 工作
- **对标**: cmux `Sources/AppleScriptBridge.swift`

### GAP-6.8 可编程 Sidebar 解释器(IPC 侧)
- **状态**: ❌ | **优先级**: P2
- **目标**: 同 GAP-3.2,本侧重 IPC — 解释器订阅事件流,sidebar 视图 = 事件流聚合 + 用户渲染回调
- **依赖**: GAP-3.2、GAP-6.7
- **DoD**: sidebar 文件订阅 `feed.*` 事件,实时显示待审批数

### GAP-6.5 Dock 集成
- **状态**: ❌ | **优先级**: P4
- **目标**: macOS Dock 角标(Feed 项数 / 待审批);`NSApplication.dockTile().setBadgeLabel()`;Dock 右键菜单显示最近 Workspace
- **依赖**: GAP-7.4(Feed 审批)
- **DoD**: macOS Dock 显示 "3" 角标
- **对标**: cmux `Sources/DockController.swift`

---

## F. AI Agent

### GAP-7.8 agent-browser 兼容
- **状态**: 🚧 | **优先级**: P1
- **现状**: 代码闭环(2026-06-24 补齐 DoD 关键面)——8 async ops + 2 sync ops(`navigate/click/type/extract/scroll/wait/get_url/get_title` + **`screenshot` 真实实现**);`browser/commands.rs::BrowserOp` 每 op 经 wry `evaluate_script_with_callback` 跑 IIFE(JS try/catch 永远回 `{ok,...}` —— WKWebView 异常会丢 completion handler 不能挂 reply;`wait` 返回 Promise 单次 resolution 无需轮询)。**screenshot**:`wry::WebViewExtMacOS::webview()` 拿 `WryWebView`(WKWebView 子类)→ `takeSnapshotWithConfiguration:completionHandler:`(`block2` `RcBlock` + `Mutex<Option<Sender>>`,照搬 `macos_eyedropper`)+ NSImage→PNG(TIFF→NSBitmapImageRep→base64 data URL);非 macOS 降级 stub error。**MCP 全 op 暴露(DoD 关键)**:`mcp/server.rs` 现 9 个 browser tool(此前仅 navigate/extract,agent 连 `click` 都发不出);`browser_tools_cover_full_op_set` 测试锁定。**deferred-reply 关联**(核心):`browser.*` 在 `Message::Ipc` handler 先于 `router.dispatch` 拦截 → `handle_browser_request`;per-op oneshot(`tx` 塞 wry `Fn` 闭包用 `Mutex<Option<Sender>>` 守护,wry 回调是 `Fn` 非 `FnOnce`),`rx` 经 `Task::perform + tokio::time::timeout` 兜底;`Message::BrowserOpResolved` first-wins 回复 + emit `Category::Browser`。sensitivity gate(跨 origin navigate)仅审计 emit `browser.request` 并放行(交互式 feed-panel 拦截 deferred)。CLI `pureterm browser {…}` 11 子命令。**dist `.app` 已重打包(release 编译 0 error + 15 browser/mcp 单测全绿);待用户真实页面 GUI E2E**(我无法跑 GUI,是唯一 DoD 闭环门槛)
- **目标**: GAP-3.5 实现后,实现 agent-browser 兼容子集 — `click / type / screenshot / navigate / extract`;参考 `https://agent-browser.dev/protocol`
- **依赖**: GAP-3.5
- **DoD**: agent 发 `click #submit`,Browser 面板点击对应 DOM
- **未做(独立后续)**: 交互式 feed-panel 跨 origin 两阶段拦截(现 audit-only,模板 `FeedPanelState` 已就绪)/ 93→125 命令扩展(约 100+ 命令,无 in-repo 明细,独立大项目)。「`screenshot` 真实实现」已于 2026-06-24 落地(WKWebView `takeSnapshot`),不再列为未做
- **对标**: cmux `Sources/Browser/`(93/125 命令兼容)

---

## G. 平台

### GAP-8.1 Sparkle auto-update
- **状态**: ❌ | **优先级**: P4
- **目标**: macOS 嵌入 Sparkle 框架(`objc2`);Linux `apt/pacman` 仓库 + 检查;Windows `winget` + Squirrel;跨平台抽象 `updater::check_and_notify()`
- **DoD**: macOS 检查新版本,菜单栏通知
- **对标**: cmux `Sources/Updater/SparkleController.swift`

---

## 参考

- **已实现特性**: [`FEATURES.md`](../FEATURES.md) — 按类别组织,含实现位置和单测数
- **对标项目**: [cmux](https://github.com/manaflow-ai/cmux)(v0.64.x)、Ghostty、iTerm2、Alacritty
- **主线**: branch `iced-cover`

**更新约定**:
- 新特性闭环 → 同步本文件状态 + `FEATURES.md`
- 季度末 → 复盘 P0/P1 焦点
- cmux/Ghostty/iTerm2 重大发布 → 整体复盘并增补 GAP
