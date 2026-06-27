# PureTerm 功能缺口总览

> 基于竞品分析（cmux、iTerm2、Tabby、Kaku、Warp）+ 内部 GAP 追踪识别的缺失功能。
> 每个功能的详细文档见 [`docs/featuresList/`](featuresList/) 目录。
> 已闭环(✅)特性见 [`FEATURES.md`](../FEATURES.md)。

---

## 总览

| 状态 | 数量 | 说明 |
|------|------|------|
| ✅ 已完成 | 9 | 代码闭环，待或已完成 E2E |
| 🚧 进行中 | 2 | 代码闭环待 E2E / 部分实现 |
| ❌ 未实现 | 58 | 待排期 |
| ⛔ 不做 | 0 | — |

### 优先级分布

| 优先级 | 数量 | 说明 |
|--------|------|------|
| **P0** | 6 | AI 核心管道 + 基础保障，必须实现 |
| **P1** | 6 | AI 差异化能力，高优先级 |
| **P2** | 18 | 终端增强，中优先级 |
| **P3** | 13 | 兼容性/国际化，低优先级 |
| **P4** | 21 | 视觉/分发，最低优先级 |

---

## ✅ 已完成（9 个）

| GAP | 功能 | 状态 | 文档 |
|-----|------|------|------|
| GAP-3.5 | [Browser 面板](featuresList/67-browser-panel.md) | ✅ 代码闭环 + harness 全绿 | `crates/pureterm/src/browser/` |
| GAP-1.3 | 标题栈 (CSI 22/23t) | ✅ GUI E2E 通过 | `crates/pureterm/src/terminal_context.rs` |
| GAP-5.4 | [SSH Agent Forwarding](featuresList/40-ssh-forwarding.md) | ✅ Track A DoD 闭环 | `crates/pureterm/src/ssh/` |
| GAP-5.6 | [SSH Reconnect 状态机](featuresList/68-ssh-reconnect.md) | ✅ 代码闭环 + harness 绿（GUI 真机 E2E 后补） | `crates/pureterm/src/ssh/` |
| GAP-5.2 | [SOCKS5 隧道](featuresList/69-socks5-tunnel.md) | ✅ 代码闭环 + 单测绿（真实网关 E2E 待人工） | `crates/pureterm-daemon/src/{tunnel,known_hosts,persist}.rs` |
| GAP-6.7 | [Event Stream](featuresList/66-event-stream.md) | ✅ 代码闭环 + harness 绿（GUI E2E 后补） | `crates/pureterm/src/events_log.rs` |
| GAP-32 | [Git Integration](featuresList/32-git-integration.md) | ✅ 代码闭环 + harness 绿（GUI E2E 后补） | `crates/pureterm/src/{status_bar/git.rs,gh_pr.rs,git_panel.rs}` |
| — | Settings Panel | ✅ 已有 | `crates/pureterm-settings/src/app.rs` |
| — | [Dual Close](featuresList/) | ✅ 已有 | `crates/pureterm/src/iced_app.rs` |

---

## 🚧 进行中（2 个）

| GAP | 功能 | 优先级 | 文档 |
|-----|------|--------|------|
| GAP-7.8 | [Agent-Browser 兼容](featuresList/67-browser-panel.md) | P1 | 代码闭环，9 browser tool，待用户页面 E2E |
| GAP-4.2 | [Hover Preview](featuresList/16-hover-preview.md) | P2 | 智能选择已实现，无 hover preview |

---

## P0 — AI 核心管道 + 基础保障（6 个）

| # | GAP | 功能 | 类别 | 工作量 | 文档 |
|---|-----|------|------|--------|------|
| 01 | — | [AI Chat Panel](featuresList/01-ai-chat-panel.md) | AI Agent | XL | 内置 AI 聊天面板，流式 Markdown + 工具调用 |
| 02 | — | [Error Recovery](featuresList/02-error-recovery.md) | AI Agent | M | 命令失败时自动分析并建议修复 |
| 09 | — | [Notification System](featuresList/09-notification-system.md) | 终端交互 | M | OSC 9/99/777 通知，视觉提示哪个 pane 需关注 |
| 15 | — | [Shell Integration](featuresList/15-shell-integration.md) | 终端交互 | L | 自动注入 shell 脚本，marks + command tracking |
| GAP-5.6 | ✅ | [SSH Reconnect](featuresList/68-ssh-reconnect.md) | SSH | S | 完整 reconnect/disconnect 状态机（收尾） |
| GAP-6.7 | ✅ | [Event Stream](featuresList/66-event-stream.md) | 集成 | S | 事件流 + 持久 JSONL 日志（收尾） |

## P1 — AI 差异化能力（6 个）

| # | GAP | 功能 | 类别 | 工作量 | 文档 |
|---|-----|------|------|--------|------|
| 03 | — | [Codebase Indexing](featuresList/03-codebase-indexing.md) | AI Agent | L | Tree-sitter + 嵌入向量，AI 代码理解基础 |
| 10 | — | [Triggers](featuresList/10-triggers.md) | 终端交互 | L | 67+ 种模式匹配自动化规则 |
| 19 | — | [Dual Sidebar](featuresList/19-dual-sidebar.md) | UI/外壳 | L | 左侧 Workspace + 右侧工具面板 |
| 35 | — | [LSP Client](featuresList/35-lsp-client.md) | 开发者工具 | XL | 内置语言服务器，代码补全/诊断 |
| GAP-7.8 | 🚧 | [Agent-Browser 兼容](featuresList/67-browser-panel.md) | AI Agent | S | browser.* ops + MCP + screenshot（收尾） |
| GAP-5.2 | ✅ | [SOCKS5 隧道](featuresList/69-socks5-tunnel.md) | SSH | M | SOCKS5/HTTP CONNECT 代理（收尾：TOFU + 持久化闭环，真实网关 E2E 待人工） |

## P2 — 终端增强（18 个）

| # | GAP | 功能 | 类别 | 工作量 | 文档 |
|---|-----|------|------|--------|------|
| 04 | — | [Computer Use](featuresList/04-computer-use.md) | AI Agent | XL | AI 桌面操控：截图 + 鼠标/键盘 |
| 07 | — | [Soul System](featuresList/07-soul-system.md) | AI Agent | S | AI 人格/记忆/风格配置 |
| 08 | — | [Natural Language Commands](featuresList/08-natural-language-commands.md) | AI Agent | M | `# 描述` → 自动生成 shell 命令 |
| 11 | — | [Instant Replay](featuresList/11-instant-replay.md) | 终端交互 | L | DVR 风格会话回放 |
| 12 | — | [Annotations & Marks](featuresList/12-annotations-marks.md) | 终端交互 | M | 行内注释 + shell prompt 标记 |
| 16 | 🚧 | [Hover Preview](featuresList/16-hover-preview.md) | 终端交互 | M | 悬停 URL/路径显示 tooltip |
| 24 | ✅ | [Enhanced Status Bar](featuresList/24-status-bar-enhanced.md) | UI/外壳 | L | 40+ 组件：CPU/内存/网络/Git/电池 |
| 25 | — | [Multi-Window](featuresList/25-multi-window.md) | UI/外壳 | L | 多独立窗口 + 跨窗口通信 |
| 32 | ✅ | [Git Integration](featuresList/32-git-integration.md) | 开发者工具 | M | porcelain 分计数 + gh PR + 面板 + Create-PR + tab 分支 + git.* EventBus（代码闭环，574 单测绿，GUI E2E 后补） |
| 33 | — | [Port Scanner](featuresList/33-port-scanner.md) | 开发者工具 | S | 侧边栏显示监听端口 |
| 36 | — | [Semantic History](featuresList/36-semantic-history.md) | 开发者工具 | M | Cmd-click 路径在编辑器中打开 |
| 40 | — | [SSH Forwarding](featuresList/40-ssh-forwarding.md) | SSH/网络 | M | 端口转发（本地/远程/动态） |
| 43 | — | [Remote Server](featuresList/43-remote-server.md) | SSH/网络 | XL | 远程 daemon：文件浏览 + 协作编辑 |
| 47 | — | [AppleScript Bridge](featuresList/47-apple-script.md) | 平台 | M | macOS AppleScript 自动化 |
| 48 | — | [URL Scheme](featuresList/48-url-scheme.md) | 平台 | M | `pureterm://` URL handler |
| 51 | — | [Block-based Output](featuresList/51-block-based-output.md) | 集成 | XXL | Warp 风格块级输出（重大重构） |
| GAP-3.2 | — | [可编程 Sidebar](featuresList/62-programmable-sidebar.md) | UI/外壳 | L | DSL 解释器 + IPC 事件订阅 |
| GAP-5.3 | — | [PTY Resize](featuresList/63-pty-resize.md) | SSH | S | smallest-screen-wins 策略 |

## P3 — 兼容性/国际化（13 个）

| # | GAP | 功能 | 类别 | 工作量 | 文档 |
|---|-----|------|------|--------|------|
| 06 | — | [Ambient Agents](featuresList/06-ambient-agents.md) | AI Agent | XL | 云端异步 AI 任务 |
| 13 | — | [Broadcast Input](featuresList/13-broadcast-input.md) | 终端交互 | S | 键盘输入广播到多个 pane |
| 14 | — | [Enhanced Vim Mode](featuresList/14-enhanced-vim-mode.md) | 终端交互 | M | 完整 vim：operators/text objects/registers |
| 18 | — | [BiDi Support](featuresList/18-bidi-support.md) | 终端交互 | L | RTL 双向文本渲染 |
| 26 | — | [Workspace Groups](featuresList/26-workspace-groups.md) | UI/外壳 | S | 工作区分组管理 |
| 30 | — | [Toolbelt](featuresList/30-toolbelt.md) | UI/外壳 | L | 常驻工具面板（历史/书签/片段） |
| 31 | — | [Quake-style Dock](featuresList/31-quake-dock.md) | UI/外壳 | M | 下拉式终端（全局快捷键） |
| 37 | — | [Password Manager](featuresList/37-password-manager.md) | 开发者工具 | M | 1Password/LastPass 集成 |
| 39 | — | [Serial & Telnet](featuresList/39-serial-telnet.md) | SSH/网络 | L | 串口 + Telnet 连接 |
| 41 | — | [Jump Host](featuresList/41-jump-host.md) | SSH/网络 | M | SSH 跳板机管理 |
| 42 | — | [Login Scripts](featuresList/42-login-scripts.md) | SSH/网络 | S | SSH 登录后自动执行命令 |
| 46 | — | [i18n](featuresList/46-i18n.md) | 平台 | L | 多语言界面 |
| GAP-1.2 | — | [DECSCL](featuresList/64-decscl.md) | 终端交互 | M | 设备控制（VT100-VT400 级别切换） |

## P4 — 视觉/分发（21 个）

| # | GAP | 功能 | 类别 | 工作量 | 文档 |
|---|-----|------|------|--------|------|
| 05 | — | [Voice Input](featuresList/05-voice-input.md) | AI Agent | L | 语音转文本输入 |
| 17 | — | [Sixel & Kitty Graphics](featuresList/17-sixel-kitty-graphics.md) | 终端交互 | L | 终端内图片渲染 |
| 20 | — | [Glass Effect](featuresList/20-glass-effect.md) | UI/外壳 | M | macOS 毛玻璃效果 |
| 21 | — | [Dock Badge](featuresList/21-dock-badge.md) | UI/外壳 | S | macOS Dock 角标 |
| 22 | — | [Touch Bar](featuresList/22-touch-bar.md) | UI/外壳 | M | MacBook Touch Bar |
| 23 | — | [Tab Bar](featuresList/23-tab-bar.md) | UI/外壳 | M | 可视化标签栏 |
| 28 | — | [Window Border](featuresList/28-window-border.md) | UI/外壳 | S | 自定义窗口边框 |
| 29 | — | [Badge Overlay](featuresList/29-badge-overlay.md) | UI/外壳 | S | 环境标识徽章（PROD/DEV） |
| 34 | — | [File Preview](featuresList/34-file-preview.md) | 开发者工具 | M | 图片/PDF/文本预览 |
| 38 | — | [File Transfer](featuresList/38-file-transfer.md) | SSH/网络 | L | SCP/SFTP + 拖拽上传 |
| 44 | — | [Shared Sessions](featuresList/44-shared-sessions.md) | SSH/网络 | XL | 多人协作终端 |
| 45 | — | [Auto Update](featuresList/45-auto-update.md) | 平台 | M | Sparkle/系统包管理器更新 |
| 49 | — | [Plugin System](featuresList/49-plugin-system.md) | 集成 | XL | 可扩展插件架构 |
| 50 | — | [Cloud Sync](featuresList/50-cloud-sync.md) | 集成 | M | 设置云端同步 |
| 52 | — | [Workflows](featuresList/52-workflows.md) | 集成 | M | 可复用命令序列 |
| 53 | — | [Notebooks](featuresList/53-notebooks.md) | 集成 | L | 交互式文档 |
| 54 | — | [Background Image](featuresList/54-background-image.md) | 视觉 | M | 自定义背景图片 |
| 55 | — | [Transparency & Blur](featuresList/55-transparency-blur.md) | 视觉 | M | 窗口透明度和模糊 |
| 56 | — | [Focus Follows Mouse](featuresList/56-focus-follows-mouse.md) | 视觉 | S | X11 风格焦点跟随 |
| 57 | — | [Haptic Feedback](featuresList/57-haptic-feedback.md) | 视觉 | S | MacBook 触觉反馈 |
| 58 | — | [Custom Theme & CSS](featuresList/58-custom-theme-css.md) | 视觉 | M | 完整主题系统 |
| 59 | — | [Emoji Rendering](featuresList/59-emoji-rendering.md) | 视觉 | M | Emoji 专用渲染通道 |
| 60 | — | [Screen Capture](featuresList/60-screen-capture.md) | 视觉 | M | 内置截图功能 |
| 61 | — | [Print Support](featuresList/61-print-support.md) | 视觉 | S | 终端内容打印 |
| GAP-4.6 | — | [Accessibility](featuresList/65-accessibility.md) | 视觉 | M | VoiceOver 无障碍支持 |

---

## 按类别统计

| 类别 | P0 | P1 | P2 | P3 | P4 | 已完成 | 进行中 | 总计 |
|------|----|----|----|----|----|--------|--------|------|
| **AI Agent** | 2 | 1 | 3 | 1 | 1 | 0 | 0 | **8** |
| **终端交互** | 2 | 1 | 3 | 3 | 1 | 0 | 0 | **10** |
| **UI/外壳** | 0 | 1 | 4 | 3 | 5 | 0 | 0 | **13** |
| **开发者工具** | 0 | 1 | 3 | 1 | 1 | 1 | 0 | **6** |
| **SSH/网络** | 1 | 1 | 3 | 3 | 1 | 2 | 0 | **10** |
| **平台** | 0 | 0 | 2 | 1 | 2 | 0 | 0 | **5** |
| **集成** | 1 | 0 | 1 | 0 | 4 | 1 | 0 | **7** |
| **视觉** | 0 | 0 | 0 | 0 | 8 | 0 | 0 | **8** |
| **合计** | **6** | **6** | **19** | **12** | **23** | **4** | **0** | **67** |

---

## 工作量估算

| 工作量 | 数量 | 说明 |
|--------|------|------|
| **S（2-3 天）** | 10 | SSH Reconnect 收尾, Event Stream 收尾, PTY Resize, Broadcast Input, Login Scripts, Dock Badge, Window Border, Badge, Focus Follows Mouse, Haptic Feedback |
| **M（1 周）** | 22 | Error Recovery, NL Commands, Hover Preview, Git Integration, Port Scanner, Semantic History, AppleScript, URL Scheme, Background Image, Transparency, Auto Update, Cloud Sync, etc. |
| **L（1.5-2 周）** | 16 | Shell Integration, Triggers, Codebase Indexing, Enhanced Status Bar, Multi-Window, Enhanced Vim Mode, BiDi, Toolbelt, Serial/Telnet, Jump Host, i18n, File Transfer, Voice Input, Sixel/Kitty, Notebooks, 可编程 Sidebar |
| **XL（2-3 周）** | 6 | AI Chat Panel, Computer Use, LSP Client, Remote Server, Shared Sessions, Plugin System |
| **XXL（4+ 周）** | 1 | Block-based Output |

---

## 建议实施顺序

### Phase 1（P0 收尾 + 核心，2-3 周）
1. SSH Reconnect E2E（S, 1-2 天）
2. ~~Event Stream E2E（S, 1-2 天）~~ ✅ 已完成（2026-06-26）
3. Error Recovery（M, 1 周）
4. Shell Integration（L, 1.5 周）

### Phase 2（P0 新功能 + P1，3-4 周）
1. AI Chat Panel（XL, 2-3 周）
2. Notification System（M, 1 周）
3. Agent-Browser E2E + ~~SOCKS5 隧道收尾~~ ✅ 已完成（2026-06-27，TOFU + 持久化闭环，真实网关 E2E 非阻塞待人工）
4. Codebase Indexing（L, 1.5 周）

### Phase 3（P1 剩余 + P2 核心，4-6 周）
1. Triggers（L, 1.5 周）
2. LSP Client（XL, 2-3 周）
3. Dual Sidebar（L, 1.5 周）
4. ~~Git Integration（M, 1 周）~~ ✅ 已完成（2026-06-27）
5. SSH Forwarding（M, 1 周）
6. Semantic History（M, 1 周）

### Phase 4（P2 剩余 + P3+P4，持续）
按需实现，优先社区反馈最高的功能。

---

## 相关文件

- **已实现特性**: [`FEATURES.md`](../FEATURES.md) — 36 个已闭环功能
- **竞品分析**: [`cmux-comparison.md`](cmux-comparison.md), [`iterm2-comparison.md`](iterm2-comparison.md), [`tabby-comparison.md`](tabby-comparison.md), [`kaku-comparison.md`](kaku-comparison.md), [`warp-comparison.md`](warp-comparison.md)
- **E2E 手册**: [`manual-e2e/`](manual-e2e/) 目录
- **工作区规划**: [`gap-2.1-workspace-plan.md`](gap-2.1-workspace-plan.md)

---

**更新约定**:
- 新特性闭环 → 同步本文件状态 + `FEATURES.md`
- 季度末 → 复盘 P0/P1 焦点
- cmux/Ghostty/iTerm2/Warp 重大发布 → 整体复盘并增补 GAP
