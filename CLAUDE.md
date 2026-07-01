# PureTerm - 基于 Alacritty 的终端模拟器

基于 Alacritty 上游源码构建的 GPU 加速终端模拟器。

## 快速开始

```bash
cargo build --release
./target/release/pureterm
```

配置文件：`~/.config/pureterm/alacritty.toml`（兼容回退 `~/.config/alacritty/alacritty.toml`）

## 架构

6 个 crate 的工作区：
- `pureterm` — 二进制入口（薄封装，基于 Iced 0.14 daemon）
- `pureterm-alacritty-runtime` — Alacritty 内核 + GPU 字体图集（精简版，2026-06-08 起仅含 `renderer::glyph_atlas`）
- `pureterm-settings` — Iced 0.14 设置 UI（独立进程，Cmd+, 打开）
- `alacritty_terminal` — 终端内核、PTY、Grid（vendored 上游源码）
- `alacritty_config` — 配置反序列化抽象（vendored 上游源码）
- `alacritty_config_derive` — 配置 derive 宏（vendored 上游源码）

## Agent 指南

- **项目导航**：[AGENTS.md](AGENTS.md)
- **工作区总览**：[crates/AGENTS.md](crates/AGENTS.md)
- **已实现特性**：[FEATURES.md](FEATURES.md)
- **待办特性**：[docs/todo.md](docs/todo.md)
- **进行中特性**：[docs/progressing.md](docs/progressing.md)
- **已完成路线图**：[docs/done.md](docs/done.md)
- **已 Review 特性**：[docs/reviewed.md](docs/reviewed.md)

## 特性生命周期

```
todo/ → progressing/ → done/（待 review）→ reviewed/（已 review）
```

| 阶段 | 目录 | 索引文件 | 说明 |
|------|------|----------|------|
| 待办 | `todo/` | `todo.md` | 规划中的特性 spec，按类别分表，含项目优先级 |
| 进行中 | `progressing/` | `progressing.md` | 正在开发的特性 spec，代码未闭环 |
| 已完成 | `done/` | `done.md` | 代码已闭环但未经 review 的特性 spec |
| 已 Review | `reviewed/` | `reviewed.md` | 已通过 review 的特性 spec，最终归档 |

**流转规则**：
1. 新增特性 → 创建 `todo/GAP-XX-slug/spec.md` → 写入 `todo.md`
2. 开发启动 → `git mv` 到 `progressing/` → 更新 `progressing.md`
3. 代码闭环 → `git mv` 到 `done/` → 更新 `done.md` + `FEATURES.md`
4. Review 通过 → `git mv` 到 `reviewed/` → 更新 `reviewed.md`

每个 spec 目录包含 `spec.md`（需求 + 竞品 + 现状 + 优先级）和可选的 `test-plan.md`（手动 E2E runbook）。

## 维护规则

- vendored Alacritty 源码不做无关格式化或重构，便于同步上游
- 新功能应在主路径（`crates/pureterm/src/`）实现；`pureterm-alacritty-runtime` 已精简为只承载 `glyph_atlas`，不再扩展
- 修改 alacritty 源码时在修改处添加注释说明原因
- 已实现特性维护在 [FEATURES.md](FEATURES.md)，新特性闭环后同步更新
- 特性 spec 按生命周期维护在 `docs/` 对应目录，详见上方「特性生命周期」

## graphify

本项目在 graphify-out/ 目录下有一个知识图谱，包含核心节点、社区结构和跨文件关系。

规则：
- 回答代码库相关问题时，先运行 `graphify query "<问题>"`（当 graphify-out/graph.json 存在时）。用 `graphify path "<A>" "<B>"` 查找两个概念之间的关系，用 `graphify explain "<概念>"` 深入了解某个概念。这些命令返回一个局部子图，通常比 GRAPH_REPORT.md 或原始 grep 输出小得多。
- 如果 graphify-out/wiki/index.md 存在，用它做宽泛导航，而不是直接浏览源码。
- 只有在架构概览或 query/path/explain 无法提供足够上下文时，才阅读 graphify-out/GRAPH_REPORT.md。
- 修改代码后，运行 `graphify update .` 保持图谱最新（仅 AST，无需 API 消耗）。


## Superpowers 使用策略

先判断任务复杂度，再决定是否使用 Superpowers。

- 简单任务：直接完成，不使用 Superpowers。
- 中等任务：按需使用必要的 Skill。
- 复杂任务：使用完整 Superpowers 工作流。

始终选择最轻量的方案。

不要为了使用 Skill 而使用 Skill，仅在其能明显提升质量、降低风险或改善可维护性时才启用。
