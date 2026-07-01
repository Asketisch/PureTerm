# 已完成任务

> 已实现但未经 review 的特性 spec 放在 [`done/`](done/) 目录。按任务类型分组，含项目优先级。

---

## 概览

| 类别 | 数量 | P0 | P1 | P2 | P3 | P4 |
|------|------|----|----|----|----|----|
| 平台 | 1 | — | — | — | — | 1 |
| 集成 | 4 | — | — | — | — | 4 |
| 视觉 | 9 | — | — | — | — | 9 |
| **合计** | **14** | **—** | **—** | **1** | **1** | **14** |

---

## 平台（1）

| GAP | 简述 | 规划类型 | 项目优先级 | 详情 |
|-----|------|----------|-----------|------|
| GAP-45 | 自动更新检查 | platform | P4 | [spec](done/GAP-45-auto-update/spec.md) |

## 集成（4）

| GAP | 简述 | 规划类型 | 项目优先级 | 详情 |
|-----|------|----------|-----------|------|
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
