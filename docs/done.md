# 已完成任务

> 已实现但未经 review 的特性 spec 放在 [`done/`](done/) 目录。按任务类型分组，含项目优先级。

---

## 概览

| 类别 | 数量 | P0 | P1 | P2 | P3 | P4 |
|------|------|----|----|----|----|----|
| 平台 | 0 | — | — | — | — | — |
| 集成 | 0 | — | — | — | — | — |
| 视觉 | 9 | — | — | — | 1 | 8 |
| **合计** | **9** | **—** | **—** | **—** | **1** | **8** |

---

## 视觉 / 平台（macOS）

| GAP | 简述 | 详情 |
|-----|------|------|
| GAP-4.6 | VoiceOver 公告：`NSAccessibilityPostNotification` 真实接通 + 分类 helper（tab / 分屏 / 命令面板 / 状态栏 / 通知） | [spec](reviewed/GAP-4.6-accessibility/spec.md) |
| GAP-54 | 终端背景图：image crate 解码 PNG/JPG + 专用 wgpu 管线 + Center/Tile/Stretch 模式 + opacity | [spec](reviewed/GAP-54-background-image/spec.md) |
| GAP-55 | 玻璃材质：NSVisualEffectView 安装在 content view 下方 + blend mode | [spec](reviewed/GAP-55-transparency-blur/spec.md) |
| GAP-56 | 焦点跟随鼠标：CursorMoved + TerminalWakeup tick 钩入 pane_under_point | [spec](reviewed/GAP-56-focus-follows-mouse/spec.md) |
| GAP-57 | 触觉反馈：NSHapticFeedbackManager.defaultPerformer().performFeedbackPattern | [spec](reviewed/GAP-57-haptic-feedback/spec.md) |
| GAP-58 | 系统外观自动跟随 + 社区主题安装路径 | [spec](reviewed/GAP-58-custom-theme-css/spec.md) |
| GAP-59 | Emoji 独立 atlas 通道 + EmojiGlyph 容器 | [spec](reviewed/GAP-59-emoji-rendering/spec.md) |
| GAP-60 | Pane 截图导出：image crate 编码 PNG + SVG stub + text snapshot | [spec](reviewed/GAP-60-screen-capture/spec.md) |
| GAP-61 | 打印：NSPrintOperation 模态面板 + dataWithPDFInsideRect PDF 导出 + Linux lpr fallback | [spec](reviewed/GAP-61-print-support/spec.md) |

---

## 性能优化

| 计划 | 简述 | 进度 | 详情 |
|------|------|------|------|
| render-input-lag | 渲染输入延迟优化（Batch 1 已完成，Batch 2-3 待执行） | 1/3 | [plan](done/render-input-lag/plan.md) |

---

**更新约定**：新特性闭环 → 从 [`progressing.md`](progressing.md) 移入对应类别分表 → 更新 `done/GAP-XX/spec.md` frontmatter → Review 通过后 `git mv` 到 `reviewed/`。
