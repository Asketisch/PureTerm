# 已 Review 任务

> 已通过 review 的特性 spec 放在 [`reviewed/`](reviewed/) 目录。按任务类型分组，含项目优先级。

---

## 概览

| 类别 | 数量 | P0 | P1 | P2 | P3 | P4 |
|------|------|----|----|----|----|----|
| 核心架构 | 1 | 1 | — | — | — | — |
| AI Agent | 8 | 2 | 1 | 3 | 1 | 1 |
| 终端交互 | 12 | 2 | 1 | 4 | 4 | 1 |
| UI/外壳 | 22 | — | 2 | 3 | 3 | 14 |
| 开发者工具 | 6 | — | 1 | 3 | 1 | 1 |
| SSH/网络 | 11 | 1 | 1 | 4 | 3 | 2 |
| 平台 | 3 | — | — | 2 | — | 1 |
| 集成 | 6 | 1 | — | 1 | — | 4 |
| **合计** | **70** | **7** | **6** | **20** | **13** | **24** |

---

## 核心架构（1）

| GAP | 简述 | 规划类型 | 项目优先级 | Review | 详情 |
|-----|------|----------|-----------|--------|------|
| GAP-2.1 | Surface/Pane/Workspace/Window 四层抽象 | arch | P0 | ✅ | [spec](reviewed/GAP-2.1-workspace/spec.md) · [plan](reviewed/GAP-2.1-workspace/plan.md) · [test-plan](reviewed/GAP-2.1-workspace/test-plan.md) |

## AI Agent（8）

| GAP | 简述 | 规划类型 | 项目优先级 | Review | 详情 |
|-----|------|----------|-----------|--------|------|
| GAP-01 | AI Chat Panel（⌘⇧L 右侧面板 + 流式 LLM + 上下文注入） | feature | P0 | ✅ | [spec](reviewed/GAP-01-ai-chat-panel/spec.md) |
| GAP-02 | Error Recovery（非零 exit → LLM 修复建议 + Apply） | wrap-up | P0 | ✅ | [spec](reviewed/GAP-02-error-recovery/spec.md) · [test-plan](reviewed/GAP-02-error-recovery/test-plan.md) |
| GAP-03 | Codebase Indexing MVP（Tree-sitter + SQLite FTS + MCP 搜索工具） | feature | P1 | ✅ | [spec](reviewed/GAP-03-codebase-indexing/spec.md) |
| GAP-04 | Computer Use（macOS 截图 + 输入 stub + MCP tools） | feature | P2 | ✅ | [spec](reviewed/GAP-04-computer-use/spec.md) |
| GAP-07 | Soul System（SOUL.md 等注入 AI Chat system prompt） | feature | P2 | ✅ | [spec](reviewed/GAP-07-soul-system/spec.md) |
| GAP-08 | Natural Language Commands（`# 描述` → shell 命令卡片） | feature | P2 | ✅ | [spec](reviewed/GAP-08-natural-language-commands/spec.md) |
| GAP-06 | Ambient Agents 本地任务队列 MVP | feature | P3 | ✅ | [spec](reviewed/GAP-06-ambient-agents/spec.md) |
| GAP-05 | 语音转文本输入 MVP | feature | P4 | ✅ | [spec](reviewed/GAP-05-voice-input/spec.md) |

## 终端交互（12）

| GAP | 简述 | 规划类型 | 项目优先级 | Review | 详情 |
|-----|------|----------|-----------|--------|------|
| GAP-09 | Notification System（OSC 9/99/777 + Bell → ring/面板/Dock/toast/CLI） | wrap-up | P0 | ✅ | [spec](reviewed/GAP-09-notification/spec.md) · [test-plan](reviewed/GAP-09-notification/test-plan.md) |
| GAP-15 | Shell Integration（OSC 133 A/B/C/D + shell.context MCP） | wrap-up | P0 | ✅ | [spec](reviewed/GAP-15-shell-integration/spec.md) |
| GAP-10 | Triggers（regex 匹配输出 → highlight/notify/send_text/exec） | feature | P1 | ✅ | [spec](reviewed/GAP-10-triggers/spec.md) |
| GAP-1.3 | 标题栈 (CSI 22/23t + OSC 0/2) | feature | P2 | ✅ | [spec](reviewed/GAP-1.3-title-stack/spec.md) · [test-plan](reviewed/GAP-1.3-title-stack/test-plan.md) |
| GAP-11 | Instant Replay MVP（events.jsonl 回放 + CLI export） | feature | P2 | ✅ | [spec](reviewed/GAP-11-instant-replay/spec.md) |
| GAP-12 | Annotations & Marks MVP（OSC 133 marks + 行注释存储） | feature | P2 | ✅ | [spec](reviewed/GAP-12-annotations-marks/spec.md) |
| GAP-4.2 | Smart Selection Hover Preview（tooltip + 语义右键 / ⌘⇧O） | enhance | P2 | ✅ | [spec](reviewed/GAP-4.2-hover-preview/spec.md) |
| GAP-1.2 | DECSCL（VT100–VT400 级别跟踪） | feature | P3 | ✅ | [spec](reviewed/GAP-1.2-decscl/spec.md) |
| GAP-13 | Broadcast Input（⌘⌃B 多 pane 广播） | feature | P3 | ✅ | [spec](reviewed/GAP-13-broadcast-input/spec.md) |
| GAP-14 | Enhanced Vim（operators/registers MVP） | enhance | P3 | ✅ | [spec](reviewed/GAP-14-enhanced-vim-mode/spec.md) |
| GAP-18 | BiDi 双向文本（unicode-bidi MVP） | feature | P3 | ✅ | [spec](reviewed/GAP-18-bidi-support/spec.md) |
| GAP-17 | Sixel/Kitty 图形协议 scaffold | feature | P4 | ✅ | [spec](reviewed/GAP-17-sixel-kitty-graphics/spec.md) |

## UI/外壳（22）

| GAP | 简述 | 规划类型 | 项目优先级 | Review | 详情 |
|-----|------|----------|-----------|--------|------|
| GAP-19 | Dual Sidebar（⌘B 左 / ⌘⇧B 右 — Workspace + Find/Feed/Git） | feature | P1 | ✅ | [spec](reviewed/GAP-19-dual-sidebar/spec.md) |
| GAP-3.5 | 内置 Browser 面板 / WebView | feature | P1 | ✅ | [spec](reviewed/GAP-3.5-browser-panel/spec.md) · [test-plan](reviewed/GAP-3.5-browser-panel/test-plan.md) |
| GAP-24 | Enhanced Status Bar（40+ 组件） | enhance | P2 | ✅ | [spec](reviewed/GAP-24-status-bar/spec.md) |
| GAP-25 | Multi-Window MVP（⌘⇧N 新窗 + 跨实例 workspace.switch） | feature | P2 | ✅ | [spec](reviewed/GAP-25-multi-window/spec.md) |
| GAP-3.2 | 可编程 Sidebar DSL MVP（TOML + EventBus） | feature | P2 | ✅ | [spec](reviewed/GAP-3.2-programmable-sidebar/spec.md) |
| GAP-26 | Workspace Groups（左侧栏分组） | feature | P3 | ✅ | [spec](reviewed/GAP-26-workspace-groups/spec.md) |
| GAP-30 | Toolbelt（右侧栏 Tools 标签） | feature | P3 | ✅ | [spec](reviewed/GAP-30-toolbelt/spec.md) |
| GAP-31 | Quake-style Dock（F12 下拉窗口 MVP） | feature | P3 | ✅ | [spec](reviewed/GAP-31-quake-dock/spec.md) |
| GAP-20 | macOS 毛玻璃效果 MVP | platform | P4 | ✅ | [spec](reviewed/GAP-20-glass-effect/spec.md) |
| GAP-21 | Dock Badge（通知 + Feed 角标 + Dock 最近 Workspace 菜单） | wrap-up | P4 | ✅ | [spec](reviewed/GAP-21-dock-badge/spec.md) |
| GAP-22 | Touch Bar scaffold | platform | P4 | ✅ | [spec](reviewed/GAP-22-touch-bar/spec.md) |
| GAP-23 | 可视化 Tab Bar 配置 | feature | P4 | ✅ | [spec](reviewed/GAP-23-tab-bar/spec.md) |
| GAP-27 | Settings Panel（独立 Iced 进程） | feature | P4 | ✅ | [spec](reviewed/GAP-27-settings-panel/spec.md) |
| GAP-28 | 自定义窗口边框 | feature | P4 | ✅ | [spec](reviewed/GAP-28-window-border/spec.md) |
| GAP-29 | 环境标识徽章 | feature | P4 | ✅ | [spec](reviewed/GAP-29-badge-overlay/spec.md) |
| GAP-4.6 | VoiceOver 无障碍 | platform | P4 | ✅ | [spec](reviewed/GAP-4.6-accessibility/spec.md) |
| GAP-54 | 背景图片配置 | feature | P4 | ✅ | [spec](reviewed/GAP-54-background-image/spec.md) |
| GAP-55 | 窗口透明度/模糊 | feature | P4 | ✅ | [spec](reviewed/GAP-55-transparency-blur/spec.md) |
| GAP-56 | 焦点跟随鼠标 | feature | P4 | ✅ | [spec](reviewed/GAP-56-focus-follows-mouse/spec.md) |
| GAP-57 | 触觉反馈 | platform | P4 | ✅ | [spec](reviewed/GAP-57-haptic-feedback/spec.md) |
| GAP-58 | 扩展主题 | feature | P4 | ✅ | [spec](reviewed/GAP-58-custom-theme-css/spec.md) |
| GAP-60 | Pane 截图/导出（文本快照 MVP） | feature | P4 | ✅ | [spec](reviewed/GAP-60-screen-capture/spec.md) |

## 开发者工具（6）

| GAP | 简述 | 规划类型 | 项目优先级 | Review | 详情 |
|-----|------|----------|-----------|--------|------|
| GAP-37 | Password Manager（op/lpass/keychain） | integration | P3 | ✅ | [spec](reviewed/GAP-37-password-manager/spec.md) |
| GAP-36 | Semantic History（⌘-click file:line → 编辑器） | feature | P2 | ✅ | [spec](reviewed/GAP-36-semantic-history/spec.md) |
| GAP-33 | Port Scanner（右侧边栏 Ports 标签 + lsof 周期扫描） | feature | P2 | ✅ | [spec](reviewed/GAP-33-port-scanner/spec.md) |
| GAP-32 | Git Integration（porcelain + gh PR + 面板） | integration | P2 | ✅ | [spec](reviewed/GAP-32-git-integration/spec.md) · [test-plan](reviewed/GAP-32-git-integration/test-plan.md) |
| GAP-35 | LSP Client（多语言 server 配置 + hover/completion + 诊断） | integration | P1 | ✅ | [spec](reviewed/GAP-35-lsp-client/spec.md) |
| GAP-34 | 文件预览 | feature | P4 | ✅ | [spec](reviewed/GAP-34-file-preview/spec.md) |

## SSH/网络（10）

| GAP | 简述 | 规划类型 | 项目优先级 | Review | 详情 |
|-----|------|----------|-----------|--------|------|
| GAP-5.6 | SSH Reconnect 状态机 | integration | P0 | ✅ | [spec](reviewed/GAP-5.6-ssh-reconnect/spec.md) · [test-plan](reviewed/GAP-5.6-ssh-reconnect/test-plan.md) |
| GAP-5.2 | SOCKS5/HTTP CONNECT 隧道 | integration | P1 | ✅ | [spec](reviewed/GAP-5.2-socks5-tunnel/spec.md) · [test-plan](reviewed/GAP-5.2-socks5-tunnel/test-plan.md) |
| GAP-40 | SSH 端口转发 -L/-R/-X UI（MVP） | integration | P2 | ✅ | [spec](reviewed/GAP-40-ssh-forwarding/spec.md) |
| GAP-5.4 | SSH Agent Forwarding | integration | P2 | ✅ | [spec](reviewed/GAP-40-ssh-forwarding/spec.md) |
| GAP-43 | 远程文件浏览（`remote.listdir/read/write/find` + Files 侧栏） | feature | P2 | ✅ | [spec](reviewed/GAP-43-remote-server/spec.md) |
| GAP-5.3 | smallest-screen-wins PTY resize | feature | P2 | ✅ | [spec](reviewed/GAP-5.3-pty-resize/spec.md) |
| GAP-39 | Serial & Telnet（CLI 探测 + profile 字段） | feature | P3 | ✅ | [spec](reviewed/GAP-39-serial-telnet/spec.md) |
| GAP-41 | SSH Jump Host（ProxyJump -J） | feature | P3 | ✅ | [spec](reviewed/GAP-41-jump-host/spec.md) |
| GAP-42 | SSH Login Scripts（login_commands） | feature | P3 | ✅ | [spec](reviewed/GAP-42-login-scripts/spec.md) |
| GAP-38 | SCP 文件传输 | feature | P4 | ✅ | [spec](reviewed/GAP-38-file-transfer/spec.md) |
| GAP-44 | 共享会话 stub（config + invite + serve stub） | feature | P4 | ✅ | [spec](reviewed/GAP-44-shared-sessions/spec.md) |

## 平台（3）

| GAP | 简述 | 规划类型 | 项目优先级 | Review | 详情 |
|-----|------|----------|-----------|--------|------|
| GAP-47 | macOS AppleScript 自动化 MVP（CLI + MCP） | platform | P2 | ✅ | [spec](reviewed/GAP-47-apple-script/spec.md) |
| GAP-48 | `pureterm://` URL Scheme（macOS 注册 + tab/workspace/ssh） | platform | P2 | ✅ | [spec](reviewed/GAP-48-url-scheme/spec.md) |
| GAP-46 | i18n（主应用 + Settings en/zh） | feature | P3 | ✅ | [spec](reviewed/GAP-46-i18n/spec.md) |
| GAP-45 | 自动更新检查 | platform | P4 | ✅ | [spec](reviewed/GAP-45-auto-update/spec.md) |

## 集成（6）

| GAP | 简述 | 规划类型 | 项目优先级 | Review | 详情 |
|-----|------|----------|-----------|--------|------|
| GAP-6.7 | Event Stream + 持久 JSONL 日志 | integration | P0 | ✅ | [spec](reviewed/GAP-6.7-event-stream/spec.md) · [test-plan](reviewed/GAP-6.7-event-stream/test-plan.md) |
| GAP-51 | 块级输出（Blocks 侧栏 + sum-tree 索引 + grid 切片 + 搜索） | arch | P2 | ✅ | [spec](reviewed/GAP-51-block-based-output/spec.md) |
| GAP-49 | 插件系统 stub | arch | P4 | ✅ | [spec](reviewed/GAP-49-plugin-system/spec.md) |
| GAP-50 | 配置同步（本地 bundle） | integration | P4 | ✅ | [spec](reviewed/GAP-50-cloud-sync/spec.md) |
| GAP-52 | Workflows（参数化命令序列） | feature | P4 | ✅ | [spec](reviewed/GAP-52-workflows/spec.md) |
| GAP-53 | Notebooks（交互式文档数据模型 MVP） | feature | P4 | ✅ | [spec](reviewed/GAP-53-notebooks/spec.md) |

## 手动 E2E Test Plans

> 跨多个 GAP 的共享 E2E runbook。

| 文件 | 覆盖范围 | 详情 |
|------|----------|------|
| Agent Features Smoke | GAP-7.3–7.7 AI Agent 特性 | [runbook](reviewed/test-plan/agent-features-smoke.md) |
| Tmux Features Smoke | GAP-6.6 tmux 兼容子集（12 命令） | [runbook](reviewed/test-plan/tmux-features-smoke.md) |

---

**更新约定**：Review 通过 → 从 [`done.md`](done.md) 移入对应类别分表 → `git mv` feature 目录到 `reviewed/` → 更新 `reviewed/GAP-XX/spec.md` frontmatter。
