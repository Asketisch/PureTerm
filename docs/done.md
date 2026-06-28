# PureTerm 已完成特性（路线图）

> 已从 [`todo.md`](todo.md) 毕业的 GAP 项。完整实现记录见 [`FEATURES.md`](../FEATURES.md)。

| GAP | 简述 | 类别 | 规划类型 | 优先级 | 闭环日期 | 详情 |
|-----|------|------|----------|--------|----------|------|
| GAP-2.1 | Surface/Pane/Workspace/Window 四层抽象 | 核心架构 | arch | P0 | 2026-06-25 | [spec](features/GAP-2.1-workspace/spec.md) |
| GAP-3.5 | 内置 Browser 面板 / WebView | UI/外壳 | feature | P1 | 2026-06-25 | [spec](features/GAP-3.5-browser-panel/spec.md) |
| GAP-1.3 | 标题栈 (CSI 22/23t + OSC 0/2) | 终端交互 | feature | P2 | 2026-06-25 | [spec](features/GAP-1.3-title-stack/spec.md) |
| GAP-5.4 | SSH Agent Forwarding | SSH/网络 | integration | P2 | 2026-06-24 | [spec](features/GAP-40-ssh-forwarding/spec.md) |
| GAP-5.6 | SSH Reconnect 状态机 | SSH/网络 | integration | P0 | 2026-06-25 | [spec](features/GAP-5.6-ssh-reconnect/spec.md) |
| GAP-5.2 | SOCKS5/HTTP CONNECT 隧道 | SSH/网络 | integration | P1 | 2026-06-27 | [spec](features/GAP-5.2-socks5-tunnel/spec.md) |
| GAP-6.7 | Event Stream + 持久 JSONL 日志 | 集成 | integration | P0 | 2026-06-26 | [spec](features/GAP-6.7-event-stream/spec.md) |
| GAP-32 | Git Integration（porcelain + gh PR + 面板） | 开发者工具 | integration | P2 | 2026-06-27 | [spec](features/GAP-32-git-integration/spec.md) |
| GAP-24 | Enhanced Status Bar（40+ 组件） | UI/外壳 | enhance | P2 | 2026-06-26 | [spec](features/GAP-24-status-bar/spec.md) |
| GAP-27 | Settings Panel（独立 Iced 进程） | UI/外壳 | feature | P4 | 2026-06-08 | [spec](features/GAP-27-settings-panel/spec.md) |

---

**更新约定**：新特性 flip ✅ 时，从 [`todo.md`](todo.md) 移入本表，并同步 `features/GAP-XX/spec.md` 的 `status: done` 与 `closed` 日期。
