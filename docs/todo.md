# PureTerm 待办特性

> 仅含**未完成**项。已完成 → [`done.md`](done.md)；详细 spec → [`features/`](features/)；分阶段路线图 → [`roadmap.md`](roadmap.md)；已交付代码记录 → [`FEATURES.md`](../FEATURES.md)。

---

## 🚧 进行中（7）

| GAP | 简述 | 类别 | 规划类型 | 优先级 | 详情 |
|-----|------|------|----------|--------|------|
| GAP-02 | 错误恢复：Shell exit 触发 + LLM 卡片 + Apply，待 GUI E2E | AI Agent | wrap-up | P0 | [spec](features/GAP-02-error-recovery/spec.md) · [E2E](manual-e2e/gap-02-error-recovery.md) |
| GAP-01 | AI Chat Panel：Phase 1+2 headless ✅，GUI/配置未接 | AI Agent | feature | P0 | [spec](features/GAP-01-ai-chat-panel/spec.md) |
| GAP-15 | Shell Integration：OSC 133 + EventBus + MCP，待 GUI E2E | 终端交互 | wrap-up | P0 | [spec](features/GAP-15-shell-integration/spec.md) |
| GAP-10 | Triggers：库+配置 stub，Wakeup 扫描/highlight 未接 | 终端交互 | feature | P1 | [spec](features/GAP-10-triggers/spec.md) |
| GAP-7.8 | Agent-Browser 兼容：9 browser tool，待用户页面 E2E | AI Agent | wrap-up | P1 | [spec](features/GAP-3.5-browser-panel/spec.md) |
| GAP-4.2 | Hover Preview：智能选择已有，悬停 tooltip 未做 | 终端交互 | enhance | P2 | [spec](features/GAP-4.2-hover-preview/spec.md) |
| GAP-21 | Dock Badge：通知角标 ✅，Feed 叠加 + Dock 菜单 ❌ | UI/外壳 | wrap-up | P4 | [spec](features/GAP-21-dock-badge/spec.md) |

---

## ❌ 未开始（51）

### P0

_（无 — GAP-02 已移入进行中）_

### P1

| GAP | 简述 | 类别 | 规划类型 | 优先级 | 详情 |
|-----|------|------|----------|--------|------|
| GAP-03 | Tree-sitter + 嵌入向量，AI 代码理解基础 | AI Agent | feature | P1 | [spec](features/GAP-03-codebase-indexing/spec.md) |
| GAP-19 | 左侧 Workspace + 右侧工具面板 | UI/外壳 | feature | P1 | [spec](features/GAP-19-dual-sidebar/spec.md) |
| GAP-35 | 内置 LSP：代码补全/诊断 | 开发者工具 | integration | P1 | [spec](features/GAP-35-lsp-client/spec.md) |

### P2

| GAP | 简述 | 类别 | 规划类型 | 优先级 | 详情 |
|-----|------|------|----------|--------|------|
| GAP-04 | AI 桌面操控：截图 + 鼠标/键盘 | AI Agent | feature | P2 | [spec](features/GAP-04-computer-use/spec.md) |
| GAP-07 | AI 人格/记忆/风格配置 | AI Agent | feature | P2 | [spec](features/GAP-07-soul-system/spec.md) |
| GAP-08 | `# 描述` → 自动生成 shell 命令 | AI Agent | feature | P2 | [spec](features/GAP-08-natural-language-commands/spec.md) |
| GAP-11 | DVR 风格会话回放 | 终端交互 | feature | P2 | [spec](features/GAP-11-instant-replay/spec.md) |
| GAP-12 | 行内注释 + shell prompt 标记 | 终端交互 | feature | P2 | [spec](features/GAP-12-annotations-marks/spec.md) |
| GAP-25 | 多独立窗口 + 跨窗口通信 | UI/外壳 | feature | P2 | [spec](features/GAP-25-multi-window/spec.md) |
| GAP-33 | 侧边栏显示监听端口 | 开发者工具 | feature | P2 | [spec](features/GAP-33-port-scanner/spec.md) |
| GAP-36 | Cmd-click 路径在编辑器中打开 | 开发者工具 | feature | P2 | [spec](features/GAP-36-semantic-history/spec.md) |
| GAP-40 | SSH 端口转发 -L/-R/-X（Agent/SOCKS 已独立闭环） | SSH/网络 | integration | P2 | [spec](features/GAP-40-ssh-forwarding/spec.md) |
| GAP-43 | 远程 daemon：文件浏览 + 协作编辑 | SSH/网络 | feature | P2 | [spec](features/GAP-43-remote-server/spec.md) |
| GAP-47 | macOS AppleScript 自动化 | 平台 | platform | P2 | [spec](features/GAP-47-apple-script/spec.md) |
| GAP-48 | `pureterm://` URL handler | 平台 | platform | P2 | [spec](features/GAP-48-url-scheme/spec.md) |
| GAP-51 | Warp 风格块级输出（重大重构） | 集成 | arch | P2 | [spec](features/GAP-51-block-based-output/spec.md) |
| GAP-3.2 | DSL 解释器 + IPC 事件订阅 | UI/外壳 | feature | P2 | [spec](features/GAP-3.2-programmable-sidebar/spec.md) |
| GAP-5.3 | smallest-screen-wins PTY resize | SSH/网络 | feature | P2 | [spec](features/GAP-5.3-pty-resize/spec.md) |

### P3

| GAP | 简述 | 类别 | 规划类型 | 优先级 | 详情 |
|-----|------|------|----------|--------|------|
| GAP-06 | 云端异步 AI 任务 | AI Agent | feature | P3 | [spec](features/GAP-06-ambient-agents/spec.md) |
| GAP-13 | 键盘输入广播到多个 pane | 终端交互 | feature | P3 | [spec](features/GAP-13-broadcast-input/spec.md) |
| GAP-14 | 完整 vim：operators/text objects/registers | 终端交互 | enhance | P3 | [spec](features/GAP-14-enhanced-vim-mode/spec.md) |
| GAP-18 | RTL 双向文本渲染 | 终端交互 | feature | P3 | [spec](features/GAP-18-bidi-support/spec.md) |
| GAP-26 | 工作区分组管理 | UI/外壳 | feature | P3 | [spec](features/GAP-26-workspace-groups/spec.md) |
| GAP-30 | 常驻工具面板（历史/书签/片段） | UI/外壳 | feature | P3 | [spec](features/GAP-30-toolbelt/spec.md) |
| GAP-31 | 下拉式终端（全局快捷键） | UI/外壳 | feature | P3 | [spec](features/GAP-31-quake-dock/spec.md) |
| GAP-37 | 1Password/LastPass 集成 | 开发者工具 | integration | P3 | [spec](features/GAP-37-password-manager/spec.md) |
| GAP-39 | 串口 + Telnet 连接 | SSH/网络 | feature | P3 | [spec](features/GAP-39-serial-telnet/spec.md) |
| GAP-41 | SSH 跳板机管理 | SSH/网络 | feature | P3 | [spec](features/GAP-41-jump-host/spec.md) |
| GAP-42 | SSH 登录后自动执行命令 | SSH/网络 | feature | P3 | [spec](features/GAP-42-login-scripts/spec.md) |
| GAP-46 | 多语言界面 | 平台 | feature | P3 | [spec](features/GAP-46-i18n/spec.md) |
| GAP-1.2 | 设备控制（VT100–VT400 级别切换） | 终端交互 | feature | P3 | [spec](features/GAP-1.2-decscl/spec.md) |

### P4

| GAP | 简述 | 类别 | 规划类型 | 优先级 | 详情 |
|-----|------|------|----------|--------|------|
| GAP-05 | 语音转文本输入 | AI Agent | feature | P4 | [spec](features/GAP-05-voice-input/spec.md) |
| GAP-17 | 终端内图片渲染（Sixel/Kitty） | 终端交互 | feature | P4 | [spec](features/GAP-17-sixel-kitty-graphics/spec.md) |
| GAP-20 | macOS 毛玻璃效果 | UI/外壳 | platform | P4 | [spec](features/GAP-20-glass-effect/spec.md) |
| GAP-22 | MacBook Touch Bar | UI/外壳 | platform | P4 | [spec](features/GAP-22-touch-bar/spec.md) |
| GAP-23 | 可视化标签栏 | UI/外壳 | feature | P4 | [spec](features/GAP-23-tab-bar/spec.md) |
| GAP-28 | 自定义窗口边框 | UI/外壳 | feature | P4 | [spec](features/GAP-28-window-border/spec.md) |
| GAP-29 | 环境标识徽章（PROD/DEV） | UI/外壳 | feature | P4 | [spec](features/GAP-29-badge-overlay/spec.md) |
| GAP-34 | 图片/PDF/文本预览 | 开发者工具 | feature | P4 | [spec](features/GAP-34-file-preview/spec.md) |
| GAP-38 | SCP/SFTP + 拖拽上传 | SSH/网络 | feature | P4 | [spec](features/GAP-38-file-transfer/spec.md) |
| GAP-44 | 多人协作终端 | SSH/网络 | feature | P4 | [spec](features/GAP-44-shared-sessions/spec.md) |
| GAP-45 | Sparkle/系统包管理器更新 | 平台 | platform | P4 | [spec](features/GAP-45-auto-update/spec.md) |
| GAP-49 | 可扩展插件架构 | 集成 | arch | P4 | [spec](features/GAP-49-plugin-system/spec.md) |
| GAP-50 | 设置云端同步 | 集成 | integration | P4 | [spec](features/GAP-50-cloud-sync/spec.md) |
| GAP-52 | 可复用命令序列 | 集成 | feature | P4 | [spec](features/GAP-52-workflows/spec.md) |
| GAP-53 | 交互式文档 | 集成 | feature | P4 | [spec](features/GAP-53-notebooks/spec.md) |
| GAP-54 | 自定义背景图片 | 视觉 | feature | P4 | [spec](features/GAP-54-background-image/spec.md) |
| GAP-55 | 窗口透明度和模糊 | 视觉 | feature | P4 | [spec](features/GAP-55-transparency-blur/spec.md) |
| GAP-56 | X11 风格焦点跟随 | 视觉 | feature | P4 | [spec](features/GAP-56-focus-follows-mouse/spec.md) |
| GAP-57 | MacBook 触觉反馈 | 视觉 | platform | P4 | [spec](features/GAP-57-haptic-feedback/spec.md) |
| GAP-58 | 完整主题系统 | 视觉 | feature | P4 | [spec](features/GAP-58-custom-theme-css/spec.md) |
| GAP-59 | Emoji 专用渲染通道 | 视觉 | enhance | P4 | [spec](features/GAP-59-emoji-rendering/spec.md) |
| GAP-60 | 内置截图功能 | 视觉 | feature | P4 | [spec](features/GAP-60-screen-capture/spec.md) |
| GAP-61 | 终端内容打印 | 视觉 | feature | P4 | [spec](features/GAP-61-print-support/spec.md) |
| GAP-4.6 | VoiceOver 无障碍支持 | 视觉 | platform | P4 | [spec](features/GAP-4.6-accessibility/spec.md) |

---

**更新约定**：特性闭环 → 从本文件删行 → 写入 [`done.md`](done.md) → 更新对应 `features/GAP-XX/spec.md` frontmatter → 同步 [`FEATURES.md`](../FEATURES.md)。
