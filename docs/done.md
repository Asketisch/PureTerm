# 已完成任务

> 已实现但未经 review 的特性 spec 放在 [`done/`](done/) 目录。按任务类型分组，含项目优先级。

---

## 概览

| 类别 | 数量 | P0 | P1 | P2 | P3 | P4 |
|------|------|----|----|----|----|----|
| 开发者工具 | 4 | — | — | 2 | 1 | 1 |
| SSH/网络 | 9 | — | — | 4 | 3 | 2 |
| 平台 | 4 | — | — | 2 | 1 | 1 |
| 集成 | 5 | — | — | 1 | — | 4 |
| 视觉 | 9 | — | — | — | — | 9 |
| **合计** | **31** | **—** | **—** | **9** | **5** | **17** |

---

## 开发者工具（4）

| GAP | 简述 | 规划类型 | 项目优先级 | 详情 |
|-----|------|----------|-----------|------|
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
| GAP-59 | Emoji 渲染通道 | enhance | P4 | [spec](done/GAP-59-emoji-rendering/spec.md) |
| GAP-61 | 打印支持 | feature | P4 | [spec](done/GAP-61-print-support/spec.md) |

## 性能优化

| 计划 | 简述 | 进度 | 详情 |
|------|------|------|------|
| render-input-lag | 渲染输入延迟优化（Batch 1 已完成，Batch 2-3 待执行） | 1/3 | [plan](done/render-input-lag/plan.md) |

---

**更新约定**：新特性闭环 → 从 [`progressing.md`](progressing.md) 移入对应类别分表 → 更新 `done/GAP-XX/spec.md` frontmatter → Review 通过后 `git mv` 到 `reviewed/`。
