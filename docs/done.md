# PureTerm 已完成特性（路线图）

> 已从 [`todo.md`](todo.md) 毕业的 GAP 项。完整实现记录见 [`FEATURES.md`](../FEATURES.md)。

| GAP | 简述 | 类别 | 规划类型 | 优先级 | 闭环日期 | Review | 详情 |
|-----|------|------|----------|--------|----------|--------|------|
| GAP-2.1 | Surface/Pane/Workspace/Window 四层抽象 | 核心架构 | arch | P0 | 2026-06-25 | ✅ | [spec](features/GAP-2.1-workspace/spec.md) |
| GAP-3.5 | 内置 Browser 面板 / WebView | UI/外壳 | feature | P1 | 2026-06-25 | 未完成 | [spec](features/GAP-3.5-browser-panel/spec.md) |
| GAP-1.3 | 标题栈 (CSI 22/23t + OSC 0/2) | 终端交互 | feature | P2 | 2026-06-25 | 未完成 | [spec](features/GAP-1.3-title-stack/spec.md) |
| GAP-5.4 | SSH Agent Forwarding | SSH/网络 | integration | P2 | 2026-06-24 | 未完成 | [spec](features/GAP-40-ssh-forwarding/spec.md) |
| GAP-5.6 | SSH Reconnect 状态机 | SSH/网络 | integration | P0 | 2026-06-25 | 未完成 | [spec](features/GAP-5.6-ssh-reconnect/spec.md) |
| GAP-5.2 | SOCKS5/HTTP CONNECT 隧道 | SSH/网络 | integration | P1 | 2026-06-27 | 未完成 | [spec](features/GAP-5.2-socks5-tunnel/spec.md) |
| GAP-6.7 | Event Stream + 持久 JSONL 日志 | 集成 | integration | P0 | 2026-06-26 | 未完成 | [spec](features/GAP-6.7-event-stream/spec.md) |
| GAP-32 | Git Integration（porcelain + gh PR + 面板） | 开发者工具 | integration | P2 | 2026-06-27 | 未完成 | [spec](features/GAP-32-git-integration/spec.md) |
| GAP-09 | Notification System（OSC 9/99/777 + Bell → ring/面板/Dock/toast/CLI） | 终端交互 | feature | P0 | 2026-06-28 | 未完成 | [spec](features/GAP-09-notification/spec.md) |
| GAP-10 | Triggers（regex 匹配输出 → highlight/notify/send_text/exec） | 终端交互 | feature | P1 | 2026-06-28 | 未完成 | [spec](features/GAP-10-triggers/spec.md) |
| GAP-02 | Error Recovery（非零 exit → LLM 修复建议 + Apply） | AI Agent | wrap-up | P0 | 2026-06-28 | 未完成 | [spec](features/GAP-02-error-recovery/spec.md) |
| GAP-01 | AI Chat Panel（⌘⇧L 右侧面板 + 流式 LLM + 上下文注入） | AI Agent | feature | P0 | 2026-06-28 | 未完成 | [spec](features/GAP-01-ai-chat-panel/spec.md) |
| GAP-04 | Computer Use MVP（macOS 截图 + 输入 stub + MCP tools） | AI Agent | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-04-computer-use/spec.md) |
| GAP-07 | Soul System（SOUL.md 等注入 AI Chat system prompt） | AI Agent | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-07-soul-system/spec.md) |
| GAP-08 | Natural Language Commands（`# 描述` → shell 命令卡片） | AI Agent | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-08-natural-language-commands/spec.md) |
| GAP-15 | Shell Integration（OSC 133 A/B/C/D + shell.context MCP） | 终端交互 | wrap-up | P0 | 2026-06-28 | 未完成 | [spec](features/GAP-15-shell-integration/spec.md) |
| GAP-4.2 | Smart Selection Hover Preview（tooltip + 语义右键 / ⌘⇧O） | 终端交互 | enhance | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-4.2-hover-preview/spec.md) |
| GAP-40 | SSH 端口转发 -L/-R/-X UI（MVP） | SSH/网络 | integration | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-40-ssh-forwarding/spec.md) |
| GAP-5.3 | smallest-screen-wins PTY resize | SSH/网络 | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-5.3-pty-resize/spec.md) |
| GAP-7.8 | Agent-Browser 兼容（9 browser MCP tool + CLI） | AI Agent | wrap-up | P1 | 2026-06-28 | 未完成 | [spec](features/GAP-3.5-browser-panel/spec.md) |
| GAP-24 | Enhanced Status Bar（40+ 组件） | UI/外壳 | enhance | P2 | 2026-06-26 | 未完成 | [spec](features/GAP-24-status-bar/spec.md) |
| GAP-27 | Settings Panel（独立 Iced 进程） | UI/外壳 | feature | P4 | 2026-06-08 | 未完成 | [spec](features/GAP-27-settings-panel/spec.md) |
| GAP-21 | Dock Badge（通知 + Feed 角标 + Dock 最近 Workspace 菜单） | UI/外壳 | wrap-up | P4 | 2026-06-28 | 未完成 | [spec](features/GAP-21-dock-badge/spec.md) |
| GAP-03 | Codebase Indexing MVP（Tree-sitter + SQLite FTS + MCP 搜索工具） | AI Agent | feature | P1 | 2026-06-28 | 已完成 | [spec](features/GAP-03-codebase-indexing/spec.md) |
| GAP-19 | Dual Sidebar（⌘B 左 / ⌘⇧B 右 — Workspace + Find/Feed/Git） | UI/外壳 | feature | P1 | 2026-06-29 | 未完成 | [spec](features/GAP-19-dual-sidebar/spec.md) |
| GAP-35 | LSP Client（多语言 server 配置 + hover/completion + 诊断） | 开发者工具 | integration | P1 | 2026-06-29 | 已完成 | [spec](features/GAP-35-lsp-client/spec.md) |
| GAP-47 | macOS AppleScript 自动化 MVP（CLI + MCP） | 平台 | platform | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-47-apple-script/spec.md) |
| GAP-48 | `pureterm://` URL Scheme（macOS 注册 + tab/workspace/ssh） | 平台 | platform | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-48-url-scheme/spec.md) |
| GAP-33 | Port Scanner（右侧边栏 Ports 标签 + lsof 周期扫描） | 开发者工具 | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-33-port-scanner/spec.md) |
| GAP-36 | Semantic History（⌘-click file:line → 编辑器） | 开发者工具 | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-36-semantic-history/spec.md) |
| GAP-11 | Instant Replay MVP（events.jsonl 回放 + CLI export） | 终端交互 | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-11-instant-replay/spec.md) |
| GAP-12 | Annotations & Marks MVP（OSC 133 marks + 行注释存储） | 终端交互 | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-12-annotations-marks/spec.md) |
| GAP-25 | Multi-Window MVP（⌘⇧N 新窗 + 跨实例 workspace.switch） | UI/外壳 | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-25-multi-window/spec.md) |
| GAP-3.2 | 可编程 Sidebar DSL MVP（TOML + EventBus） | UI/外壳 | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-3.2-programmable-sidebar/spec.md) |
| GAP-06 | Ambient Agents 本地任务队列 MVP | AI Agent | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-06-ambient-agents/spec.md) |
| GAP-13 | Broadcast Input（⌘⌃B 多 pane 广播） | 终端交互 | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-13-broadcast-input/spec.md) |
| GAP-14 | Enhanced Vim（operators/registers MVP） | 终端交互 | enhance | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-14-enhanced-vim-mode/spec.md) |
| GAP-18 | BiDi 双向文本（unicode-bidi MVP） | 终端交互 | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-18-bidi-support/spec.md) |
| GAP-26 | Workspace Groups（左侧栏分组） | UI/外壳 | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-26-workspace-groups/spec.md) |
| GAP-30 | Toolbelt（右侧栏 Tools 标签） | UI/外壳 | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-30-toolbelt/spec.md) |
| GAP-31 | Quake-style Dock（F12 下拉窗口 MVP） | UI/外壳 | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-31-quake-dock/spec.md) |
| GAP-37 | Password Manager（op/lpass/keychain） | 开发者工具 | integration | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-37-password-manager/spec.md) |
| GAP-39 | Serial & Telnet（CLI 探测 + profile 字段） | SSH/网络 | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-39-serial-telnet/spec.md) |
| GAP-41 | SSH Jump Host（ProxyJump -J） | SSH/网络 | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-41-jump-host/spec.md) |
| GAP-42 | SSH Login Scripts（login_commands） | SSH/网络 | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-42-login-scripts/spec.md) |
| GAP-46 | i18n（主应用 + Settings en/zh） | 平台 | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-46-i18n/spec.md) |
| GAP-1.2 | DECSCL（VT100–VT400 级别跟踪） | 终端交互 | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-1.2-decscl/spec.md) |
| GAP-05 | 语音转文本输入 MVP | AI Agent | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-05-voice-input/spec.md) |
| GAP-17 | Sixel/Kitty 图形协议 scaffold | 终端交互 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-17-sixel-kitty-graphics/spec.md) |
| GAP-20 | macOS 毛玻璃效果 MVP | UI/外壳 | platform | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-20-glass-effect/spec.md) |
| GAP-22 | Touch Bar scaffold | UI/外壳 | platform | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-22-touch-bar/spec.md) |
| GAP-23 | 可视化 Tab Bar 配置 | UI/外壳 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-23-tab-bar/spec.md) |
| GAP-28 | 自定义窗口边框 | UI/外壳 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-28-window-border/spec.md) |
| GAP-29 | 环境标识徽章 | UI/外壳 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-29-badge-overlay/spec.md) |
| GAP-34 | 文件预览 | 开发者工具 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-34-file-preview/spec.md) |
| GAP-38 | SCP 文件传输 | SSH/网络 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-38-file-transfer/spec.md) |
| GAP-44 | 共享会话 stub | SSH/网络 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-44-shared-sessions/spec.md) |
| GAP-45 | 自动更新检查 | 平台 | platform | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-45-auto-update/spec.md) |
| GAP-49 | 插件系统 stub | 集成 | arch | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-49-plugin-system/spec.md) |
| GAP-50 | 配置同步（本地 bundle） | 集成 | integration | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-50-cloud-sync/spec.md) |
| GAP-52 | Workflows | 集成 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-52-workflows/spec.md) |
| GAP-53 | Notebooks | 集成 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-53-notebooks/spec.md) |
| GAP-54 | 背景图片配置 | 视觉 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-54-background-image/spec.md) |
| GAP-55 | 窗口透明度/模糊 | 视觉 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-55-transparency-blur/spec.md) |
| GAP-56 | 焦点跟随鼠标 | 视觉 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-56-focus-follows-mouse/spec.md) |
| GAP-57 | 触觉反馈 | 视觉 | platform | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-57-haptic-feedback/spec.md) |
| GAP-58 | 扩展主题 | 视觉 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-58-custom-theme-css/spec.md) |
| GAP-59 | Emoji 渲染通道 | 视觉 | enhance | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-59-emoji-rendering/spec.md) |
| GAP-60 | Pane 截图/导出 | 视觉 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-60-screen-capture/spec.md) |
| GAP-61 | 打印支持 | 视觉 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-61-print-support/spec.md) |
| GAP-4.6 | VoiceOver 无障碍 | 视觉 | platform | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-4.6-accessibility/spec.md) |
| GAP-43 | 远程文件浏览（`remote.listdir/read/write/find` + Files 侧栏） | SSH/网络 | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-43-remote-server/spec.md) |
| GAP-51 | 块级输出（Blocks 侧栏 + sum-tree 索引 + grid 切片 + 搜索） | 集成 | arch | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-51-block-based-output/spec.md) |

---

**更新约定**：新特性 flip ✅ 时，从 [`todo.md`](todo.md) 移入本表，并同步 `features/GAP-XX/spec.md` 的 `status: done` 与 `closed` 日期；`Review` 列默认为 `未完成`，人工验收通过后改为 `已完成`。
