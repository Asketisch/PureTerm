# PureTerm 已完成特性（路线图）

> 已从 [`todo.md`](todo.md) 毕业的 GAP 项。完整实现记录见 [`FEATURES.md`](../FEATURES.md)。

## 概览

| 类别 | 数量 | P0 | P1 | P2 | P3 | P4 | Review ✅ | 进度 |
|------|------|----|----|----|----|----|-----------|------|
| 核心架构 | 1 | 1 | — | — | — | — | 1 | 1/1 |
| AI Agent | 9 | 2 | 2 | 3 | 1 | 1 | 5 | 5/9 |
| 终端交互 | 12 | 2 | 1 | 4 | 4 | 1 | 4 | 4/12 |
| UI/外壳 | 15 | — | 2 | 3 | 3 | 7 | 2 | 2/15 |
| 开发者工具 | 6 | — | 1 | 3 | 1 | 1 | 1 | 1/6 |
| SSH/网络 | 11 | 1 | 1 | 4 | 3 | 2 | 2 | 2/11 |
| 平台 | 4 | — | — | 2 | 1 | 1 | — | 0/4 |
| 集成 | 6 | 1 | — | 1 | — | 4 | 1 | 1/6 |
| 视觉 | 9 | — | — | — | — | 9 | — | 0/9 |
| **合计** | **73** | **7** | **7** | **20** | **13** | **26** | **16** | **16/73** |

**合计**：73 项 · Review ✅ 16 · 未完成 57

---

## 核心架构（1）

| GAP | Review# | 简述 | 规划类型 | 优先级 | 闭环日期 | Review | 详情 |
|-----|---------|------|----------|--------|----------|--------|------|
| GAP-2.1 | — | Surface/Pane/Workspace/Window 四层抽象 | arch | P0 | 2026-06-25 | ✅ | [spec](features/GAP-2.1-workspace/spec.md) |

## AI Agent（9）

| GAP | Review# | 简述 | 规划类型 | 优先级 | 闭环日期 | Review | 详情 |
|-----|---------|------|----------|--------|----------|--------|------|
| GAP-01 | — | AI Chat Panel（⌘⇧L 右侧面板 + 流式 LLM + 上下文注入） | feature | P0 | 2026-06-28 | ✅ | [spec](features/GAP-01-ai-chat-panel/spec.md) |
| GAP-02 | 1 | Error Recovery（非零 exit → LLM 修复建议 + Apply） | wrap-up | P0 | 2026-06-28 | ✅ | [spec](features/GAP-02-error-recovery/spec.md) |
| GAP-03 | — | Codebase Indexing MVP（Tree-sitter + SQLite FTS + MCP 搜索工具） | feature | P1 | 2026-06-28 | ✅ | [spec](features/GAP-03-codebase-indexing/spec.md) |
| GAP-7.8 | 5 | Agent-Browser 兼容（9 browser MCP tool + CLI） | wrap-up | P1 | 2026-06-28 | ✅ | [spec](features/GAP-3.5-browser-panel/spec.md) |
| GAP-04 | 10 | Computer Use MVP（macOS 截图 + 输入 stub + MCP tools） | feature | P2 | 2026-06-29 | ✅ | [spec](features/GAP-04-computer-use/spec.md) |
| GAP-07 | 11 | Soul System（SOUL.md 等注入 AI Chat system prompt） | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-07-soul-system/spec.md) |
| GAP-08 | 12 | Natural Language Commands（`# 描述` → shell 命令卡片） | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-08-natural-language-commands/spec.md) |
| GAP-06 | 25 | Ambient Agents 本地任务队列 MVP | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-06-ambient-agents/spec.md) |
| GAP-05 | 37 | 语音转文本输入 MVP | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-05-voice-input/spec.md) |

## 终端交互（12）

| GAP | Review# | 简述 | 规划类型 | 优先级 | 闭环日期 | Review | 详情 |
|-----|---------|------|----------|--------|----------|--------|------|
| GAP-09 | — | Notification System（OSC 9/99/777 + Bell → ring/面板/Dock/toast/CLI） | feature | P0 | 2026-06-28 | ✅ | [spec](features/GAP-09-notification/spec.md) |
| GAP-15 | — | Shell Integration（OSC 133 A/B/C/D + shell.context MCP） | wrap-up | P0 | 2026-06-28 | ✅ | [spec](features/GAP-15-shell-integration/spec.md) |
| GAP-10 | — | Triggers（regex 匹配输出 → highlight/notify/send_text/exec） | feature | P1 | 2026-06-28 | ✅ | [spec](features/GAP-10-triggers/spec.md) |
| GAP-1.3 | 2 | 标题栈 (CSI 22/23t + OSC 0/2) | feature | P2 | 2026-06-25 | ✅ | [spec](features/GAP-1.3-title-stack/spec.md) |
| GAP-11 | 13 | Instant Replay MVP（events.jsonl 回放 + CLI export） | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-11-instant-replay/spec.md) |
| GAP-12 | 14 | Annotations & Marks MVP（OSC 133 marks + 行注释存储） | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-12-annotations-marks/spec.md) |
| GAP-4.2 | 15 | Smart Selection Hover Preview（tooltip + 语义右键 / ⌘⇧O） | enhance | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-4.2-hover-preview/spec.md) |
| GAP-1.2 | 26 | DECSCL（VT100–VT400 级别跟踪） | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-1.2-decscl/spec.md) |
| GAP-13 | 27 | Broadcast Input（⌘⌃B 多 pane 广播） | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-13-broadcast-input/spec.md) |
| GAP-14 | 28 | Enhanced Vim（operators/registers MVP） | enhance | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-14-enhanced-vim-mode/spec.md) |
| GAP-18 | 29 | BiDi 双向文本（unicode-bidi MVP） | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-18-bidi-support/spec.md) |
| GAP-17 | 38 | Sixel/Kitty 图形协议 scaffold | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-17-sixel-kitty-graphics/spec.md) |

## UI/外壳（15）

| GAP | Review# | 简述 | 规划类型 | 优先级 | 闭环日期 | Review | 详情 |
|-----|---------|------|----------|--------|----------|--------|------|
| GAP-19 | 3 | Dual Sidebar（⌘B 左 / ⌘⇧B 右 — Workspace + Find/Feed/Git） | feature | P1 | 2026-06-29 | ✅ | [spec](features/GAP-19-dual-sidebar/spec.md) |
| GAP-3.5 | 4 | 内置 Browser 面板 / WebView | feature | P1 | 2026-06-25 | ✅ | [spec](features/GAP-3.5-browser-panel/spec.md) |
| GAP-24 | 16 | Enhanced Status Bar（40+ 组件） | enhance | P2 | 2026-06-26 | 未完成 | [spec](features/GAP-24-status-bar/spec.md) |
| GAP-25 | 17 | Multi-Window MVP（⌘⇧N 新窗 + 跨实例 workspace.switch） | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-25-multi-window/spec.md) |
| GAP-3.2 | 18 | 可编程 Sidebar DSL MVP（TOML + EventBus） | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-3.2-programmable-sidebar/spec.md) |
| GAP-26 | 30 | Workspace Groups（左侧栏分组） | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-26-workspace-groups/spec.md) |
| GAP-30 | 31 | Toolbelt（右侧栏 Tools 标签） | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-30-toolbelt/spec.md) |
| GAP-31 | 32 | Quake-style Dock（F12 下拉窗口 MVP） | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-31-quake-dock/spec.md) |
| GAP-20 | 39 | macOS 毛玻璃效果 MVP | platform | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-20-glass-effect/spec.md) |
| GAP-21 | 40 | Dock Badge（通知 + Feed 角标 + Dock 最近 Workspace 菜单） | wrap-up | P4 | 2026-06-28 | 未完成 | [spec](features/GAP-21-dock-badge/spec.md) |
| GAP-22 | 41 | Touch Bar scaffold | platform | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-22-touch-bar/spec.md) |
| GAP-23 | 42 | 可视化 Tab Bar 配置 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-23-tab-bar/spec.md) |
| GAP-27 | 43 | Settings Panel（独立 Iced 进程） | feature | P4 | 2026-06-08 | 未完成 | [spec](features/GAP-27-settings-panel/spec.md) |
| GAP-28 | 44 | 自定义窗口边框 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-28-window-border/spec.md) |
| GAP-29 | 45 | 环境标识徽章 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-29-badge-overlay/spec.md) |

## 开发者工具（6）

| GAP | Review# | 简述 | 规划类型 | 优先级 | 闭环日期 | Review | 详情 |
|-----|---------|------|----------|--------|----------|--------|------|
| GAP-35 | — | LSP Client（多语言 server 配置 + hover/completion + 诊断） | integration | P1 | 2026-06-29 | ✅ | [spec](features/GAP-35-lsp-client/spec.md) |
| GAP-32 | 19 | Git Integration（porcelain + gh PR + 面板） | integration | P2 | 2026-06-27 | 未完成 | [spec](features/GAP-32-git-integration/spec.md) |
| GAP-33 | 20 | Port Scanner（右侧边栏 Ports 标签 + lsof 周期扫描） | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-33-port-scanner/spec.md) |
| GAP-36 | 21 | Semantic History（⌘-click file:line → 编辑器） | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-36-semantic-history/spec.md) |
| GAP-37 | 33 | Password Manager（op/lpass/keychain） | integration | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-37-password-manager/spec.md) |
| GAP-34 | 46 | 文件预览 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-34-file-preview/spec.md) |

## SSH/网络（11）

| GAP | Review# | 简述 | 规划类型 | 优先级 | 闭环日期 | Review | 详情 |
|-----|---------|------|----------|--------|----------|--------|------|
| GAP-5.6 | — | SSH Reconnect 状态机 | integration | P0 | 2026-06-25 | ✅ | [spec](features/GAP-5.6-ssh-reconnect/spec.md) |
| GAP-5.2 | — | SOCKS5/HTTP CONNECT 隧道 | integration | P1 | 2026-06-27 | ✅ | [spec](features/GAP-5.2-socks5-tunnel/spec.md) |
| GAP-40 | 22 | SSH 端口转发 -L/-R/-X UI（MVP） | integration | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-40-ssh-forwarding/spec.md) |
| GAP-43 | 23 | 远程文件浏览（`remote.listdir/read/write/find` + Files 侧栏） | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-43-remote-server/spec.md) |
| GAP-5.3 | 24 | smallest-screen-wins PTY resize | feature | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-5.3-pty-resize/spec.md) |
| GAP-5.4 | 25 | SSH Agent Forwarding | integration | P2 | 2026-06-24 | 未完成 | [spec](features/GAP-40-ssh-forwarding/spec.md) |
| GAP-39 | 34 | Serial & Telnet（CLI 探测 + profile 字段） | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-39-serial-telnet/spec.md) |
| GAP-41 | 35 | SSH Jump Host（ProxyJump -J） | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-41-jump-host/spec.md) |
| GAP-42 | 36 | SSH Login Scripts（login_commands） | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-42-login-scripts/spec.md) |
| GAP-38 | 47 | SCP 文件传输 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-38-file-transfer/spec.md) |
| GAP-44 | 48 | 共享会话 stub | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-44-shared-sessions/spec.md) |

## 平台（4）

| GAP | Review# | 简述 | 规划类型 | 优先级 | 闭环日期 | Review | 详情 |
|-----|---------|------|----------|--------|----------|--------|------|
| GAP-47 | 26 | macOS AppleScript 自动化 MVP（CLI + MCP） | platform | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-47-apple-script/spec.md) |
| GAP-48 | 27 | `pureterm://` URL Scheme（macOS 注册 + tab/workspace/ssh） | platform | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-48-url-scheme/spec.md) |
| GAP-46 | 49 | i18n（主应用 + Settings en/zh） | feature | P3 | 2026-06-29 | 未完成 | [spec](features/GAP-46-i18n/spec.md) |
| GAP-45 | 50 | 自动更新检查 | platform | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-45-auto-update/spec.md) |

## 集成（6）

| GAP | Review# | 简述 | 规划类型 | 优先级 | 闭环日期 | Review | 详情 |
|-----|---------|------|----------|--------|----------|--------|------|
| GAP-6.7 | — | Event Stream + 持久 JSONL 日志 | integration | P0 | 2026-06-26 | ✅ | [spec](features/GAP-6.7-event-stream/spec.md) |
| GAP-51 | 28 | 块级输出（Blocks 侧栏 + sum-tree 索引 + grid 切片 + 搜索） | arch | P2 | 2026-06-29 | 未完成 | [spec](features/GAP-51-block-based-output/spec.md) |
| GAP-49 | 51 | 插件系统 stub | arch | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-49-plugin-system/spec.md) |
| GAP-50 | 52 | 配置同步（本地 bundle） | integration | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-50-cloud-sync/spec.md) |
| GAP-52 | 53 | Workflows | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-52-workflows/spec.md) |
| GAP-53 | 54 | Notebooks | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-53-notebooks/spec.md) |

## 视觉（9）

| GAP | Review# | 简述 | 规划类型 | 优先级 | 闭环日期 | Review | 详情 |
|-----|---------|------|----------|--------|----------|--------|------|
| GAP-4.6 | 55 | VoiceOver 无障碍 | platform | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-4.6-accessibility/spec.md) |
| GAP-54 | 56 | 背景图片配置 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-54-background-image/spec.md) |
| GAP-55 | 57 | 窗口透明度/模糊 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-55-transparency-blur/spec.md) |
| GAP-56 | 58 | 焦点跟随鼠标 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-56-focus-follows-mouse/spec.md) |
| GAP-57 | 59 | 触觉反馈 | platform | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-57-haptic-feedback/spec.md) |
| GAP-58 | 60 | 扩展主题 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-58-custom-theme-css/spec.md) |
| GAP-59 | 61 | Emoji 渲染通道 | enhance | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-59-emoji-rendering/spec.md) |
| GAP-60 | 62 | Pane 截图/导出 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-60-screen-capture/spec.md) |
| GAP-61 | 63 | 打印支持 | feature | P4 | 2026-06-29 | 未完成 | [spec](features/GAP-61-print-support/spec.md) |

---

**更新约定**：新特性 flip ✅ 时，从 [`todo.md`](todo.md) 移入对应 `## 类别` 分表（按 `features/GAP-XX/spec.md` 的 `category`）；表内插入位置按 `priority`（P0→P4）+ GAP 编号；同步 spec 的 `status: done` 与 `closed` 日期；`Review` 列默认为 `未完成`，人工验收通过后改为 `✅`。
