# 已完成任务

> 已实现但未经 review 的特性 spec 放在 [`done/`](done/) 目录。按任务类型分组，含项目优先级。

---

## 概览

| 类别 | 数量 | P0 | P1 | P2 | P3 | P4 |
|------|------|----|----|----|----|----|
| 平台 | 0 | — | — | — | — | — |
| 集成 | 0 | — | — | — | — | — |
| 视觉 | 2 | — | — | — | — | 2 |
| **合计** | **2** | **—** | **—** | **1** | **1** | **14** |

---

## 视觉（2）

| GAP | 简述 | 规划类型 | 项目优先级 | 详情 |
|-----|------|----------|-----------|------|
| GAP-59 | Emoji 渲染通道 | enhance | P4 | [spec](done/GAP-59-emoji-rendering/spec.md) |
| GAP-61 | 打印支持 | feature | P4 | [spec](done/GAP-61-print-support/spec.md) |

## 性能优化

| 计划 | 简述 | 进度 | 详情 |
|------|------|------|------|
| render-input-lag | 渲染输入延迟优化（Batch 1 已完成，Batch 2-3 待执行） | 1/3 | [plan](done/render-input-lag/plan.md) |

---

**更新约定**：新特性闭环 → 从 [`progressing.md`](progressing.md) 移入对应类别分表 → 更新 `done/GAP-XX/spec.md` frontmatter → Review 通过后 `git mv` 到 `reviewed/`。
