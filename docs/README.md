# PureTerm 文档索引

## 三层读取策略

| 目的 | 读什么 | 体量 |
|------|--------|------|
| 查待办 | [`todo.md`](todo.md) | ~70 行 |
| 查路线图已闭合项 | [`done.md`](done.md) | ~15 行 |
| 查分阶段计划与统计 | [`roadmap.md`](roadmap.md) | ~80 行 |
| 查某特性需求/现状 | [`features/GAP-XX-slug/spec.md`](features/) | 按需 |
| 查实施计划 | [`features/GAP-XX/plan.md`](features/) | 按需（可选） |
| 查已交付代码细节 | [`FEATURES.md`](../FEATURES.md) | 按需 |

## 特性目录结构

```
docs/features/
  GAP-09-notification/
    spec.md      # 需求、竞品、现状、优先级（含 YAML frontmatter）
    plan.md      # 可选：详细实施计划
```

### Frontmatter 字段

| 字段 | 说明 |
|------|------|
| `gap` | GAP 编号，如 `GAP-09` |
| `title` | 特性名称 |
| `category` | 类别：AI Agent / 终端交互 / UI/外壳 / 开发者工具 / SSH/网络 / 平台 / 集成 / 视觉 / 核心架构 |
| `plan_type` | 规划类型：`feature` / `enhance` / `wrap-up` / `arch` / `integration` / `platform` |
| `priority` | P0–P4 |
| `effort` | S / M / L / XL / XXL |
| `status` | `not-started` / `in-progress` / `done` |
| `closed` | 闭环日期（done 时填写） |
| `aliases` | 可选，如 GAP-7.8 与 GAP-3.5 共用 browser spec |
| `e2e` | 可选，指向 `manual-e2e/` runbook |

## 维护约定

特性 flip ✅ 时：

1. 从 [`todo.md`](todo.md) 删除对应行
2. 写入 [`done.md`](done.md)
3. 更新 `features/GAP-XX/spec.md` 的 `status` 与 `closed`
4. 同步 [`FEATURES.md`](../FEATURES.md) 实现记录

新增 GAP 时：创建 `features/GAP-XX-slug/spec.md` → 写入 [`todo.md`](todo.md) 一行。

## 其他文档

- [`manual-e2e/`](manual-e2e/) — 手动 GUI E2E runbook
- [`superpowers/`](superpowers/) — 设计 spec 与实施计划
- [`removed-legacy-features.md`](removed-legacy-features.md) — 已移除 legacy 功能
