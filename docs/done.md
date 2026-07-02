# 已完成任务

> 已实现但未经 review 的特性 spec 放在 [`done/`](done/) 目录。按任务类型分组，含项目优先级。
> 最近一次批量闭环（2026-07-02）把视觉 / 平台（macOS）9 个 spec 与 GAP-46 i18n 推入 [`reviewed/`](reviewed/)，本目录只保留当前活跃的 done 项。

---

## 概览

| 类别 | 数量 | P0 | P1 | P2 | P3 | P4 |
|------|------|----|----|----|----|----|
| 平台 | 1 | — | — | — | — | 1 |
| 集成 | 4 | — | — | — | — | 4 |
| **合计** | **5** | **—** | **—** | **—** | **—** | **5** |

---

## 平台

| GAP | 简述 | 详情 |
|-----|------|------|
| GAP-45 | 自动更新：`auto_update.rs` 真实 GitHub Releases HTTP（reqwest）+ 简易 semver 比较 + `pureterm update check --remote` CLI + 6 项单测 | [spec](done/GAP-45-auto-update/spec.md) |

## 集成

| GAP | 简述 | 详情 |
|-----|------|------|
| GAP-49 | 插件系统：目录扫描 + `plugin.toml` 清单解析/校验 + `install` / `uninstall` + `list` 报告 manifest 错误 + 9 项单测（运行时仍延后到 Phase D） | [spec](done/GAP-49-plugin-system/spec.md) |
| GAP-50 | 云同步：本地 bundle（`~/.config/pureterm/sync/latest.json`）+ GitHub Gist push/pull（reqwest + bearer auth）+ schema 版本控制 + `pureterm sync push/pull --remote` CLI + 8 项单测 | [spec](done/GAP-50-cloud-sync/spec.md) |
| GAP-52 | Workflows：参数化 `{{key}}` 展开 + 缺失参数检查 + 类别（palette 分组）+ 别名（name 优先）+ `pureterm workflow run` CLI + 7 项单测 | [spec](done/GAP-52-workflows/spec.md) |
| GAP-53 | Notebooks：`.pureterm` JSON cell 格式 + cell 执行引擎（`$SHELL -c`，捕获 stdout/stderr）+ `pureterm notebook show/new/run` CLI + 9 项单测 | [spec](done/GAP-53-notebooks/spec.md) |

---

## 性能优化

| 计划 | 简述 | 进度 | 详情 |
|------|------|------|------|
| render-input-lag | 渲染输入延迟优化（Batch 1 已完成，Batch 2-3 待执行） | 1/3 | [plan](done/render-input-lag/plan.md) |

---

**更新约定**：新特性闭环 → 从 [`progressing.md`](progressing.md) 移入对应类别分表 → 更新 `done/GAP-XX/spec.md` frontmatter → Review 通过后 `git mv` 到 `reviewed/`。
