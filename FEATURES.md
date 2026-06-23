# PureTerm 已实现特性

> 本文件汇总 PureTerm 所有已闭环(✅)特性,按类别组织。未实现/进行中特性见 [`docs/TODOLIST.md`](docs/TODOLIST.md)。
> 更新约定:新特性闭环后同步本文件 + `docs/TODOLIST.md` 状态更新。

---

## A. 核心架构

### GAP-2.2 嵌套水平+垂直分屏(2026-06-10)
- 水平/垂直递归分屏,拖拽调整比例(MIN_RATIO=0.1, MAX_RATIO=0.9)
- 补齐操作:等分所有分屏 `⌘=` / 交换 Pane `⌘⇧X` / 临时最大化 `⌘⇧Enter` / 焦点 Next/Prev `⌘]` `⌘[` / 移动到新 Tab `⌘⇧T` / 新窗口 `⌘⇧N`
- 键盘 + 右键菜单 + 命令面板三入口,命令面板搜 `split.*` 7 个 builtin
- 实现:`crates/pureterm/src/split_tree.rs`(equalize_all / swap_with_sibling / neighbor_in_order) + `iced_app.rs`(Tab.zoom_state + 6 Message + 5 ContextMenuItem + 7 命令面板 builtin)
- 单测:`split_tree::tests` 11 个(嵌套 3 层 / wrap / swap 二次回归 / 根 Leaf 边界)

### GAP-2.3 Socket API 协议 + 端口模型(2026-06-13)
- JSON-RPC 2.0 over Unix domain socket,31 个 method 跨 8 命名空间
- 命名空间:system(4) / window(4) / tab(4) / pane(6) / surface(4) / config(4) / ssh(3) / debug(2)
- 写操作通过 `IpcSideEffect` 枚举回写 iced 主循环,真实驱动 UI 状态变更
- 实现:`crates/pureterm/src/ipc/`(protocol / methods / listener / client)
- 单测:45 个(`ipc::protocol::tests` 6 + `ipc::methods::tests` 39)

### GAP-2.4 远程守护进程 puretermd 等价物(2026-06-22)
- 由 GAP-5.1 的 `puretermd` 实质满足:独立守护进程 + 持久 PTY + 关 GUI 不丢会话
- 本机 Unix-domain socket,非网络远程(网络远程属 GAP-5.2 SSH tunnel 范畴)

### GAP-2.5 多 UI 路径统一治理(2026-06-12)
- CLAUDE.md/AGENTS.md 一致描述"主路径=Iced";`pureterm-alacritty-runtime` 仅含 `glyph_atlas`(964 行)
- 新贡献者无歧义

---

## B. UI/UX

### GAP-3.3 命令面板 ⌘⇧P(2026-06-08)
- nucleo-matcher 模糊搜索,Actions / Profiles 双模式
- 实现:`crates/pureterm/src/iced_app.rs`

### GAP-3.4 状态栏(2026-06-12)
- 底部 24px:CWD(OSC 7) · Git 分支(30s 轮询) · SSH 连接指示 · 前台进程名 · 光标位置(row:col)
- 2026-06-22 GAP-7.6 加 agent 指示:`⚡ kind` / `❄ kind` / `⏳ N`
- 留后续迭代:顶部 title bar shortcut hint pills / 菜单栏 extra badge / CPU 内存

### GAP-3.7 完整快捷键自定义 + 模板(2026-06-12)
- Action 30 变体 + Keymap HashMap + TOML 解析 + CLI `pureterm shortcuts`
- 6 套预设模板:vim / tmux / iterm / vscode / agent-triage / custom
- 热重载:存盘即生效;Chord 多键序列数据结构已预留(`chord_bindings` + `ResolveResult::ChordPending`)
- 实现:`crates/pureterm/src/keymap.rs` + `iced_app.rs`(dispatch_action 统一派发) + CLI `shortcuts` + `resources/presets/`
- 单测:`keymap::tests` 22 个

### GAP-3.8 搜索体系(2026-06-21)
- `⌘F` 当前 surface 内搜索(正则 / case sensitive / whole word / 结果高亮跳转)
- `⌘⇧F` 在工作目录内 ripgrep 子进程搜索(tokio::process::Command)
- 全局跨 surface 搜索延后(需多 pane 扇出)
- 实现:搜索浮层 UI + flag(case/regex/whole-word)
- 196 测试绿 + dist .app 重打包;待手动 GUI E2E

---

## C. 终端交互

### GAP-4.1 Vim 复制模式(2026-06-12)
- `Ctrl+Shift+Space` 进入,hjkl 导航,w/b/e 按词,0/$ 行首尾,g/G 屏顶底,Ctrl+u/d 半页
- v/V/Ctrl+v 选区(字符/行/矩形),y 复制,yy 复制行,/ ? 搜索,n/N 遍历
- GPU 渲染:青色 vi cursor + 搜索匹配 amber 高亮;状态栏 -- COPY -- / -- VISUAL -- 指示
- 实现:`crates/pureterm/src/vi_mode_handler.rs`(ViSubMode 状态机 + 18 ViMotion + visual 选区 + yank + 搜索) + `terminal_shader.rs`(vi cursor 渲染 + 搜索高亮) + `iced_app.rs`(键盘拦截 + 搜索栏 UI + 状态栏指示)
- 单测:`vi_mode_handler::tests` 3 + Alacritty vi_mode 19

### GAP-4.3 Tail-Find 自动滚动搜索(2026-06-21)
- follow_tail:开启时若在底部自动 arm;新输出重锚(LayoutSaveTick 250ms 检测 bottommost 增长)
- 用户滚动取消(两 scroll handler);toggle 支持
- rotation-safe vi cursor 复用为锚
- 196 测试绿 + dist .app 重打包;待手动 GUI E2E

---

## D. SSH

### GAP-5.1 持久 PTY 守护进程(2026-06-17)
- 全里程碑 M1-M5 闭环:本地 + SSH PTY 均由独立 `puretermd` 守护进程拥有
- 本地 `tty::new`,SSH 用共享 `pureterm-ssh` crate 的 `SshPty::spawn` / `SshCommandBuilder` / `SshSessionManager`
- daemon `PumpablePty` trait 统一 pump;GUI `DaemonPty: EventedPty` 包 socketpair,`PtyEventLoop` 不动
- `pureterm ssh connect [user@]host` 真 interactive client
- 滚动回放 1 MiB ring → SCROLLBACK;登录自启动(macOS LaunchAgent / Linux systemd)
- 多 attach 扇出(Vec<Subscriber> 广播) + 有界背压(drop >2MiB 滞后客户端) + control/data socket 0600
- 实现:`crates/pureterm-daemon/`(daemon,含 ring_buffer / PumpablePty / SessionManager / autostart / discovery) + `crates/pureterm-ssh/`(共享 SSH spawn) + `crates/pureterm/src/{daemon_pty.rs,daemon_client.rs,daemon_cli.rs,ssh_cli.rs}` + `terminal_context.rs` + `build.sh`

### GAP-5.7 SSH Tab UI 串通(2026-06-08)
- `⌘⇧S` 弹 SSH Profile 选择对话框 → 异步 spawn → PtyEventLoop 集成 → 标题显示 `🔒 SSH: <name>`
- graceful_close:发送 `exit\r` → 等 500ms → SIGHUP 兜底
- 实现:`crates/pureterm/src/ssh/` + iced UI 串通

### GAP-5.8 SSH 主 UI 设置 UI(2026-06-08)
- 右侧栏 `SshSettingsPanelState` + `CredentialStore` 主密码端到端
- Profile 列表 + 实时编辑 + 保存到 toml
- 加密凭证存储:AES-256-GCM + Argon2id,`Zeroizing` 包裹 master_key

### GAP-5.9 SSH 命令面板集成(2026-06-08)
- `Mode::Profiles` 一跳到底:命令面板搜 `ssh` → 切到 Profiles 模式 → 选 Profile 直接连接

---

## E. 集成与可编程

### GAP-6.1 Socket API v2 完整协议(2026-06-13)
- 31 method / 8 命名空间 + `IpcSideEffect` 写操作副作用机制
- 实现:`crates/pureterm/src/ipc/methods.rs`
- 单测:45 个

### GAP-6.2 CLI 协议(2026-06-13)
- clap derive 8 命令组(system / window / tab / pane / surface / config / ssh / debug) + Msg/Shortcuts
- 全局 `--socket` / `--json` flag;leaf → method 映射复用 send_rpc
- 实现:`cli.rs` + `main.rs`(clap dispatch) + `ipc/client.rs`(`send_rpc` 返回 Value 供 `--json`)
- 单测:14 个

### GAP-6.4 自定义命令注册(2026-06-15)
- `[[commands]]` 声明式配置 → keymap 绑定 + `⌘⇧P` 面板可搜
- 执行模型:触发写 `command+\r` 进当前 pane PTY(复用 `write_to_pty`),零新进程
- 键修饰符:`+` 和 `|` 都接受;shift + 字母自动注册 macOS 双形态
- 实现:`keymap.rs`(`CustomCommand` + `Action::CustomCommand` + `split_mods_and_key` + `register_custom_commands`) + `app_config.rs`(`load_custom_commands` 容错 + id 去重) + `iced_app.rs`(dispatch + `CommandEntry::Custom` 面板)
- 单测:13 个

---

## F. AI Agent

### GAP-7.1 Agent Hooks 通用框架(2026-06-18)
- PureTerm 充当 Claude Code / 其它 agent 的 hook 中转:agent 把 hook 事件 JSON 经 stdin 喂给 `pureterm hook <event>`,PureTerm 按 `[[agent.hook]]` 策略评估
- Standalone:不依赖 daemon / GUI,直接读 `pureterm.toml` + 本地评估 + 起子进程;零 IPC 往返
- 双规则模型:内置 `deny_pattern`(正则,无需写脚本) + 外部 `command`(经 `sh -c`,stdin 喂 JSON,stdout JSON/exit=决策,默认 60s 超时)
- 事件对齐官方协议:PreToolUse / PostToolUse / Notification / UserPromptSubmit / Stop / SubagentStop / PreCompact / SessionStart / SessionEnd
- 决策输出:PreToolUse 走 `hookSpecificOutput.permissionDecision`,其它阻断型走 `{"decision":"block"}`;exit 2=block
- 实现:`agent_hooks.rs`(框架) + `app_config.rs`(`load_agent_hooks`) + `cli.rs`(`Command::Hook`)
- 单测:171 全绿(含真实子进程 exit-2 / stdout JSON / 超时 kill 验证)

### GAP-7.2 Claude Code Agent 适配器 + 安装框架(2026-06-18)
- `AgentAdapter` trait + `ClaudeCodeAdapter`;框架化使 Codex / OpenCode / Grok / Pi 成为机械式后续
- CLI: `pureterm agents install claude-code` / `list` / `uninstall`;默认仅注册已配规则事件,`--all-events` / `--dry-run`
- 安全写配置:对 `~/.claude/settings.json` 走加锁读-改-写(fs2 排他锁 + `NamedTempFile::persist` 原子写),保留既有内容
- 幂等安装(命令路径哨兵去重) + 精准卸载(只删自己的组,清空事件数组与空 hooks 键)
- 实现:`agent_adapters.rs` + `cli.rs`(`Command::Agents` / `AgentsSub`) + `main.rs`
- 单测:181 全绿

### GAP-7.3 Agent Hibernation(2026-06-23)
- daemon idle 采集:PTY 无输出 + CPU 近零 → 双条件判定
- hibernate:SIGSTOP 挂起;resume:SIGCONT 恢复
- GUI 决策面板:HibernationScanTick 双条件 + max_live_terminals 阈值 + 60s 确认倒计时 + pinned overlay
- CLI: `pureterm agent hibernate/resume/list`
- 状态栏指示:`❄ kind`(hibernated) / `⚡ kind`(active) / `⏳ N`(feed pending)
- 实现:`idle_probe.rs`(CPU 采样) + `session.rs`(hibernate/resume) + `hibernate_panel.rs` + `iced_app.rs` + `cli.rs`
- 自动 E2E harness 全绿

### GAP-7.4 Feed 审批面板(2026-06-23)
- 延迟回复(承重墙):shim 与 GUI 单连接单请求,`feed.request_approval` 不立刻回 reply,暂存 `IpcRequest` 到 `FeedPanelState::pending`,用户点按钮时 resolve → 解锁 listener
- `ask_pattern` / `ask_reason` 配置;evaluate 在 deny 后聚合 Ask(deny 永远赢过 ask)
- 5 按钮面板:Once / Always / Deny / Bypass 5min / 等待;120s 软超时自动 Deny
- 作用域记忆:Always(session_id,tool_name) / All(session_id) / Bypass(session_id+5min 过期)
- SessionEnd 清记忆;feed.jsonl 审计
- 实现:`feed_client.rs`(std-only) + `feed_panel.rs` + `agent_hooks.rs`(ask_pattern) + `ipc/methods.rs` + `iced_app.rs`
- 自动 E2E harness 全绿:220 单测

### GAP-7.5 Agent Session Restore(2026-06-23)
- session id 持久化:SessionStart hook → `AgentSessionStore`(FIFO 16 条) → `data_dir()/agents/<kind>-sessions.json`
- 冷恢复:GUI 重启 → `restore()` → daemon session 死 + pane 有 agent_session_id → 查 `[[vault.agents]]` resume_command → `claude --resume <id>` via `create_local(argv)`
- PaneSnapshot 加 `agent_session_id` / `agent_kind`
- 实现:`agent_sessions.rs` + `feed_client.rs::notify_session_started` + `session_restore.rs::cold_resume_agent` + `session.rs::create_local`
- 自动 E2E harness 全绿

### GAP-7.6 Vault agent registry(2026-06-23)
- `[[vault.agents]]` 声明式注册:`kind` / `name` / `detect`(子串) / `resume_command` / `never_hibernate`
- 进程扫描:`agent_detect.rs`(macOS `ps` / Linux `/proc`,detect 子串匹配,排除 grep/rg/ps/shell)
- CLI:`pureterm agents detect`(扫描打表) / `pureterm agents registry`(打印解析结果)
- SessionStart 默认注册修复:`derive_entries` 在 `[[vault.agents]]` 非空时自动追加,打通 GAP-7.5 冷恢复
- detect-resume GUI 面板:`detect_panel.rs` + `DetectScanTick`(30s) + vault 模板插值 → `create_resume_tab`
- 状态栏 `⚡ kind` / `❄ kind` / `⏳ N` 指示(`agent_indicator` 纯函数 + `last_session_list` 缓存,零新 IPC)
- 实现:`agent_detect.rs` + `feed_log.rs` + `agent_adapters.rs` + `iced_app.rs` + `cli.rs`
- 自动 E2E harness 全绿:305 单测

### GAP-7.7 Custom resume bindings(2026-06-23)
- `[[resume.bindings]]` CRUD:`pureterm surface resume set/list/remove`
- 轻量占位符插值:`{session_id}` / `{checkpoint}` / `{kind}` / `{cwd}`
- HMAC-SHA256 持久令牌:secret `<data_dir>/resume.key`(0600) + 摘要 `resume-tokens.json`;绑定精确 shell
- 冷恢复不弹面板:auto 绑定 + 有效令牌 → 静默 spawn;无令牌 → 跳过回落 `claude --resume`
- 复用 Feed 面板:`FeedRequestPayload.resume`;Always → `approve_token`
- 实现:`resume_bindings.rs` + `app_config.rs` + `cli.rs` + `feed_client.rs` + `feed_panel.rs` + `iced_app.rs`
- 自动 E2E harness 全绿:293 单测

### GAP-7.9 PTY 读写原语(2026-06-24)
- 补齐 Socket API 两个承重空壳,让任何 agent / 脚本能「读屏 + 注命令」(免 shell-out、免 JSON 解析)
- `surface.send_text`(`ipc/methods.rs`):`text` 必填 + `pane_id` 可选 → `IpcSideEffect::SendText`(unix);`iced_app::execute_ipc_side_effect` 镜像 `SshDisconnect` 解析 pane → `write_to_pty`(fire-and-forget)
- `surface.snapshot`:**deferred reply**(仿 `browser.*`,全 scrollback 序列化持 `FairMutex<Term>` 锁跨千行 off-main 避免掉帧);`Message::Ipc` 拦截 → clone `Arc<FairMutex<Term>>` → `Task::perform` 内 `spawn_blocking` 复用 `Term::bounds_to_string`(**不重写 cell 迭代**)+ tail 截尾 + `tokio::time::timeout`;oneshot first-wins
- CLI:`pureterm surface send-text --pane-id N --text 'echo hi\r'` / `snapshot --visible-only --lines N`
- 独立可用:CLI / 脚本 / MCP 都消费它;零依赖
- 实现:`surface_snapshot.rs` + `ipc/methods.rs` + `iced_app.rs` + `cli.rs`
- 378 单测绿

### GAP-7.10 MCP server + agent 客户端安装(2026-06-24)
- PureTerm 成为 agent 可驱动的终端宿主:`pureterm mcp serve` 起 **stdio MCP server**,Claude Code / Codex 装一次即获 `capture_pane` / `send_text` / `list_surfaces` / `new_tab` / `split_pane` / `focus_pane` / `browser_navigate` / `browser_extract` 原生结构化工具
- **MCP↔IPC 桥**(`mcp/server.rs`,feature-gated `mcp-server` 默认开):rmcp 1.7.0,**精确双 pin `rmcp`+`rmcp-macros` =1.7.0**(rmcp 1.7.0 只声明 `rmcp-macros ^1.7.0` 会解析到 1.8.0 宏,`#[tool]` 展开引用 1.7.0 runtime 不存在的 `schema_for_input`);每工具 body = `Parameters<T>` → `rpc()` 助手把**阻塞** `send_rpc` 包 `spawn_blocking` 透传 GUI Socket API,`Result<String,String>`(Err → MCP `isError`)
- **安装适配器**(`mcp/adapter.rs`):`McpClientAdapter` trait + ClaudeCodeAdapter(`.mcp.json`)/ CodexAdapter(`~/.codex/config.toml`,TOML `locked_toml_rmw`)/ ClaudeDesktopAdapter;复用 `agent_adapters::locked_json_rmw` + `pureterm_bin`;install 幂等(remove-then-insert,保留他人 server);CLI `mcp install/list/uninstall <client>`
- `[mcp]` 配置(`McpConfig`):`default_capture_lines`(5000)+ `enabled_tools` 白名单(空=全部)
- 安全 stance:同用户所有权是主边界(GUI socket `0600`;`serve` 仅因 agent 住 pane 内才继承 socket);v1 不接 Feed 审批,用户主动 `mcp install` 即知情授权
- 实现:`mcp/{mod,server,adapter}.rs` + `app_config.rs` + `cli.rs` + `Cargo.toml`(feature `mcp-server`)
- E2E:stdio handshake / `tools/list` schema 生成 / 无 GUI 时 `tools/call` 优雅 `isError` 均验证;378 单测绿

---

## G. 平台

### GAP-8.5 跨平台优势(持续维护)
- macOS / Linux / Windows 三平台持续维护

---

## 统计

| 类别 | 已实现数 |
|---|---|
| A. 核心架构 | 4 |
| B. UI/UX | 4 |
| C. 终端交互 | 2 |
| D. SSH | 4 |
| E. 集成与可编程 | 3 |
| F. AI Agent | 9 |
| G. 平台 | 1 |
| **总计** | **27** |

---
