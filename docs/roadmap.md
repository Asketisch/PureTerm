# PureTerm 路线图

> 分阶段实施顺序与统计。待办索引 → [`todo.md`](todo.md)；已完成 → [`done.md`](done.md)。

---

## 总览（未完成）

| 状态 | 数量 |
|------|------|
| 🚧 进行中 | 7 |
| ❌ 未开始 | 52 |
| **合计** | **59** |

### 优先级分布（未完成）

| 优先级 | 数量 | 说明 |
|--------|------|------|
| **P0** | 4 | 3 进行中 + 1 未开始 |
| **P1** | 5 | 2 进行中 + 3 未开始 |
| **P2** | 16 | 1 进行中 + 15 未开始 |
| **P3** | 13 | 全部未开始 |
| **P4** | 21 | 1 进行中 + 20 未开始 |

---

## 按类别统计（未完成）

| 类别 | 进行中 | 未开始 | 合计 |
|------|--------|--------|------|
| **AI Agent** | 2 | 5 | **7** |
| **终端交互** | 3 | 6 | **9** |
| **UI/外壳** | 1 | 8 | **9** |
| **开发者工具** | 0 | 4 | **4** |
| **SSH/网络** | 0 | 6 | **6** |
| **平台** | 0 | 4 | **4** |
| **集成** | 0 | 5 | **5** |
| **视觉** | 0 | 8 | **8** |
| **核心架构** | 0 | 0 | **0** |
| **合计** | **7** | **52** | **59** |

---

## 工作量估算（参考）

| 工作量 | 数量 | 说明 |
|--------|------|------|
| **S（2-3 天）** | 8 | PTY Resize, Broadcast Input, Login Scripts, Dock Badge 收尾, Window Border, Badge, Focus Follows Mouse, Haptic Feedback |
| **M（1 周）** | 20 | Error Recovery, NL Commands, Hover Preview, Port Scanner, Semantic History, AppleScript, URL Scheme, etc. |
| **L（1.5-2 周）** | 14 | Shell Integration 收尾, Triggers, Codebase Indexing, Multi-Window, Enhanced Vim, BiDi, Toolbelt, Serial/Telnet, Jump Host, i18n, File Transfer, Voice Input, Sixel/Kitty, Notebooks, 可编程 Sidebar |
| **XL（2-3 周）** | 6 | AI Chat Panel GUI, Computer Use, LSP Client, Remote Server, Shared Sessions, Plugin System |
| **XXL（4+ 周）** | 1 | Block-based Output |

---

## 建议实施顺序

### Phase 1（P0 收尾 + 核心，2-3 周）

1. Shell Integration GUI E2E（L，收尾）
2. Notification System toast/CLI（M，收尾）
3. Error Recovery（M，1 周）

### Phase 2（P0 新功能 + P1，3-4 周）

1. AI Chat Panel GUI 接线（XL，2-3 周；Phase 1+2 headless ✅）
2. Agent-Browser 用户页面 E2E（S，收尾）
3. Codebase Indexing（L，1.5 周）

### Phase 3（P1 剩余 + P2 核心，4-6 周）

1. Triggers Wakeup 扫描 + highlight 接线（L，1.5 周）
2. LSP Client（XL，2-3 周）
3. Dual Sidebar（L，1.5 周）
4. SSH 端口转发 -L/-R（M，1 周）
5. Semantic History（M，1 周）

### Phase 4（P2 剩余 + P3+P4，持续）

按需实现，优先社区反馈最高的功能。

---

## 相关文件

- **竞品分析**：[`cmux-comparison.md`](cmux-comparison.md), [`iterm2-comparison.md`](iterm2-comparison.md), [`tabby-comparison.md`](tabby-comparison.md), [`kaku-comparison.md`](kaku-comparison.md), [`warp-comparison.md`](warp-comparison.md)
- **E2E 手册**：[`manual-e2e/`](manual-e2e/)
- **Superpowers 设计**：[`superpowers/plans/`](superpowers/plans/), [`superpowers/specs/`](superpowers/specs/)

**维护约定**：季度末复盘 P0/P1 焦点；竞品重大发布 → 整体复盘并增补 GAP。
