# 已完成任务

> 已实现但未经 review 的特性 spec 放在 [`done/`](done/) 目录。按任务类型分组，含项目优先级。

---

## 概览

| 类别 | 数量 | P0 | P1 | P2 | P3 | P4 |
|------|------|----|----|----|----|----|
| 终端交互 | 10 | — | — | 4 | 4 | 2 |
| UI/外壳 | 13 | — | — | 3 | 3 | 7 |
| 开发者工具 | 5 | — | — | 3 | 1 | 1 |
| SSH/网络 | 9 | — | — | 4 | 3 | 2 |
| 平台 | 4 | — | — | 2 | 1 | 1 |
| 集成 | 5 | — | — | 1 | — | 4 |
| 视觉 | 9 | — | — | — | — | 9 |
| **合计** | **55** | **—** | **—** | **17** | **12** | **26** |

---

## 终端交互（10）

| GAP | 简述 | 规划类型 | 项目优先级 | 详情 |
|-----|------|----------|-----------|------|
| GAP-11 | Instant Replay MVP（events.jsonl 回放 + CLI export） | feature | P2 | [spec](done/GAP-11-instant-replay/spec.md) |
| GAP-12 | Annotations & Marks MVP（OSC 133 marks + 行注释存储） | feature | P2 | [spec](done/GAP-12-annotations-marks/spec.md) |
| GAP-4.2 | Smart Selection Hover Preview（tooltip + 语义右键 / ⌘⇧O） | enhance | P2 | [spec](done/GAP-4.2-hover-preview/spec.md) |
| GAP-1.2 | DECSCL（VT100–VT400 级别跟踪） | feature | P3 | [spec](done/GAP-1.2-decscl/spec.md) |
| GAP-13 | Broadcast Input（⌘⌃B 多 pane 广播） | feature | P3 | [spec](done/GAP-13-broadcast-input/spec.md) |
| GAP-14 | Enhanced Vim（operators/registers MVP） | enhance | P3 | [spec](done/GAP-14-enhanced-vim-mode/spec.md) |
| GAP-18 | BiDi 双向文本（unicode-bidi MVP） | feature | P3 | [spec](done/GAP-18-bidi-support/spec.md) |
| GAP-17 | Sixel/Kitty 图形协议 scaffold | feature | P4 | [spec](done/GAP-17-sixel-kitty-graphics/spec.md) |
| GAP-59 | Emoji 渲染通道 | enhance | P4 | [spec](done/GAP-59-emoji-rendering/spec.md) |
| GAP-61 | 打印支持 | feature | P4 | [spec](done/GAP-61-print-support/spec.md) |

## UI/外壳（13）

| GAP | 简述 | 规划类型 | 项目优先级 | 详情 |
|-----|------|----------|-----------|------|
| GAP-24 | Enhanced Status Bar（40+ 组件） | enhance | P2 | [spec](done/GAP-24-status-bar/spec.md) |
| GAP-25 | Multi-Window MVP（⌘⇧N 新窗 + 跨实例 workspace.switch） | feature | P2 | [spec](done/GAP-25-multi-window/spec.md) |
| GAP-3.2 | 可编程 Sidebar DSL MVP（TOML + EventBus） | feature | P2 | [spec](done/GAP-3.2-programmable-sidebar/spec.md) |
| GAP-26 | Workspace Groups（左侧栏分组） | feature | P3 | [spec](done/GAP-26-workspace-groups/spec.md) |
| GAP-30 | Toolbelt（右侧栏 Tools 标签） | feature | P3 | [spec](done/GAP-30-toolbelt/spec.md) |
| GAP-31 | Quake-style Dock（F12 下拉窗口 MVP） | feature | P3 | [spec](done/GAP-31-quake-dock/spec.md) |
| GAP-20 | macOS 毛玻璃效果 MVP | platform | P4 | [spec](done/GAP-20-glass-effect/spec.md) |
| GAP-21 | Dock Badge（通知 + Feed 角标 + Dock 最近 Workspace 菜单） | wrap-up | P4 | [spec](done/GAP-21-dock-badge/spec.md) |
| GAP-22 | Touch Bar scaffold | platform | P4 | [spec](done/GAP-22-touch-bar/spec.md) |
| GAP-23 | 可视化 Tab Bar 配置 | feature | P4 | [spec](done/GAP-23-tab-bar/spec.md) |
| GAP-27 | Settings Panel（独立 Iced 进程） | feature | P4 | [spec](done/GAP-27-settings-panel/spec.md) |
| GAP-28 | 自定义窗口边框 | feature | P4 | [spec](done/GAP-28-window-border/spec.md) |
| GAP-29 | 环境标识徽章 | feature | P4 | [spec](done/GAP-29-badge-overlay/spec.md) |

## 开发者工具（5）

| GAP | 简述 | 规划类型 | 项目优先级 | 详情 |
|-----|------|----------|-----------|------|
| GAP-32 | Git Integration（porcelain + gh PR + 面板） | integration | P2 | [spec](done/GAP-32-git-integration/spec.md) · [test-plan](done/GAP-32-git-integration/test-plan.md) |
| GAP-33 | Port Scanner（右侧边栏 Ports 标签 + lsof 周期扫描） | feature | P2 | [spec](done/GAP-33-port-scanner/spec.md) |
| GAP-36 | Semantic History（⌘-click file:line → 编辑器） | feature | P2 | [spec](done/GAP-36-semantic-history/spec.md) |
| GAP-37 | Password Manager（op/lpass/keychain） | integration | P3 | [spec](done/GAP-37-password-manager/spec.md) |
| GAP-34 | 文件预览 | feature | P4 | [spec](done/GAP-34-file-preview/spec.md) |

## SSH/网络（9）

| GAP | 简述 | 规划类型 | 项目优先级 | 详情 |
|-----|------|----------|-----------|------|
| GAP-40 | SSH 端口转发 -L/-R/-X UI（MVP） | integration | P2 | [spec](done/GAP-40-ssh-forwarding/spec.md) |
| GAP-43 | 远程文件浏览（`remote.listdir/read/write/find` + Files 侧栏） | feature | P2 | [spec](done/GAP-43-remote-server/spec.md) |
| GAP-5.3 | smallest-screen-wins PTY resize | feature | P2 | [spec](done/GAP-5.3-pty-resize/spec.md) |
| GAP-5.4 | SSH Agent Forwarding | integration | P2 | [spec](done/GAP-40-ssh-forwarding/spec.md) |
| GAP-39 | Serial & Telnet（CLI 探测 + profile 字段） | feature | P3 | [spec](done/GAP-39-serial-telnet/spec.md) |
| GAP-41 | SSH Jump Host（ProxyJump -J） | feature | P3 | [spec](done/GAP-41-jump-host/spec.md) |
| GAP-42 | SSH Login Scripts（login_commands） | feature | P3 | [spec](done/GAP-42-login-scripts/spec.md) |
| GAP-38 | SCP 文件传输 | feature | P4 | [spec](done/GAP-38-file-transfer/spec.md) |
| GAP-44 | 共享会话 stub | feature | P4 | [spec](done/GAP-44-shared-sessions/spec.md) |

## 平台（4）

| GAP | 简述 | 规划类型 | 项目优先级 | 详情 |
|-----|------|----------|-----------|------|
| GAP-47 | macOS AppleScript 自动化 MVP（CLI + MCP） | platform | P2 | [spec](done/GAP-47-apple-script/spec.md) |
| GAP-48 | `pureterm://` URL Scheme（macOS 注册 + tab/workspace/ssh） | platform | P2 | [spec](done/GAP-48-url-scheme/spec.md) |
| GAP-46 | i18n（主应用 + Settings en/zh） | feature | P3 | [spec](done/GAP-46-i18n/spec.md) |
| GAP-45 | 自动更新检查 | platform | P4 | [spec](done/GAP-45-auto-update/spec.md) |

## 集成（5）

| GAP | 简述 | 规划类型 | 项目优先级 | 详情 |
|-----|------|----------|-----------|------|
| GAP-51 | 块级输出（Blocks 侧栏 + sum-tree 索引 + grid 切片 + 搜索） | arch | P2 | [spec](done/GAP-51-block-based-output/spec.md) |
| GAP-49 | 插件系统 stub | arch | P4 | [spec](done/GAP-49-plugin-system/spec.md) |
| GAP-50 | 配置同步（本地 bundle） | integration | P4 | [spec](done/GAP-50-cloud-sync/spec.md) |
| GAP-52 | Workflows | feature | P4 | [spec](done/GAP-52-workflows/spec.md) |
| GAP-53 | Notebooks | feature | P4 | [spec](done/GAP-53-notebooks/spec.md) |

## 视觉（9）

| GAP | 简述 | 规划类型 | 项目优先级 | 详情 |
|-----|------|----------|-----------|------|
| GAP-4.6 | VoiceOver 无障碍 | platform | P4 | [spec](done/GAP-4.6-accessibility/spec.md) |
| GAP-54 | 背景图片配置 | feature | P4 | [spec](done/GAP-54-background-image/spec.md) |
| GAP-55 | 窗口透明度/模糊 | feature | P4 | [spec](done/GAP-55-transparency-blur/spec.md) |
| GAP-56 | 焦点跟随鼠标 | feature | P4 | [spec](done/GAP-56-focus-follows-mouse/spec.md) |
| GAP-57 | 触觉反馈 | platform | P4 | [spec](done/GAP-57-haptic-feedback/spec.md) |
| GAP-58 | 扩展主题 | feature | P4 | [spec](done/GAP-58-custom-theme-css/spec.md) |
| GAP-60 | Pane 截图/导出 | feature | P4 | [spec](done/GAP-60-screen-capture/spec.md) |

## 性能优化

| 计划 | 简述 | 进度 | 详情 |
|------|------|------|------|
| render-input-lag | 渲染输入延迟优化（Batch 1 已完成，Batch 2-3 待执行） | 1/3 | [plan](done/render-input-lag/plan.md) |

---

**更新约定**：新特性闭环 → 从 [`progressing.md`](progressing.md) 移入对应类别分表 → 更新 `done/GAP-XX/spec.md` frontmatter → Review 通过后 `git mv` 到 `reviewed/`。
