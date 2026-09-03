# 已完成任务

> 已实现但未经 review 的特性 spec 放在 [`done/`](done/) 目录。按任务类型分组，含项目优先级。
> 最近一次批量闭环（2026-07-02）把视觉 / 平台（macOS）9 个 spec 与 GAP-46 i18n 推入 [`reviewed/`](reviewed/)，本目录只保留当前活跃的 done 项。

---

## 概览

| 类别 | 数量 | P0 | P1 | P2 | P3 | P4 |
|------|------|----|----|----|----|----|
| 终端交互 | 4 | 3 | 1 | — | — | — |
| 平台 | 1 | — | — | — | — | 1 |
| 集成 | 5 | — | — | — | — | 5 |
| **合计** | **10** | **3** | **1** | **—** | **—** | **6** |

---

## 终端交互

| GAP | 简述 | 详情 |
|-----|------|------|
| t-core-gap-closure | 终端模拟核心层差距补齐（graphics / shaper / tmux-CC / 多 shell / ConPTY / plugin runtime），6 个子组件 v1→v2→v3 全部闭环：GPU quad shader + 后台 PNG 解码；rustybuzz shaper + cluster_byte_to_cell 反查 + ime_widget cluster_demo；tmux-cc E2E；fish/nu/pwsh/elvish shell integration E2E；Windows CI runner + conpty_smoke；wasmtime 插件运行时 + scan_plugin_osc parser hook + 2 示例插件 + authoring.md | [spec](done/t-core-gap-closure/spec.md) · [checklist](done/t-core-gap-closure/checklist.md) |
| GAP-63 | kitty keyboard protocol CSI u（disambiguate）：`kitty_keyboard` 纯函数编码器 + `key_to_bytes`/Enter 臂按协商模式分流（Shift+Enter → `\x1b[13;2u`，Ctrl+方向键 → `\x1b[1;5{A-D}`），裸键保持 legacy；`kitty_keyboard` 默认 true；7 单测 | [spec](done/GAP-63-kitty-keyboard-csi-u/spec.md) |
| GAP-65 | 终端自识别：env 注入 `TERM_PROGRAM`/`TERM_PROGRAM_VERSION`/`PURETERM`（daemon 与 GUI 共用 setup_env）；`scan_terminal_queries` 响应 XTVERSION（`DCS>\|PureTerm(版本)ST`）与 XTGETTCAP（Tc/RGB/setrgbf/setrgbb）；DA2 版本号改报 PureTerm；顺带修复 `Event::PtyWrite` 被映射丢弃导致所有查询响应从未写回 PTY 的既有 bug；7 单测 + 实战验证 | [spec](done/GAP-65-terminal-self-identification/spec.md) |
| GAP-66 | OSC 633（VSCode shell integration）扫描器：A/B/C/D 映射 ShellMarkKind + E 命令文本（含分号+nonce 重组）+ P;Cwd= cwd 上报 + 未知子命令忽略 + 跨缓冲跳过；复用 ShellMark 消费链（prompt 跳转/exit code/block/MCP get_shell_context）；11 单测 + 实战验证 | [spec](done/GAP-66-osc633-shell-integration/spec.md) |

## 平台

| GAP | 简述 | 详情 |
|-----|------|------|
| GAP-45 | 自动更新：`auto_update.rs` 真实 GitHub Releases HTTP（reqwest）+ 简易 semver 比较 + `pureterm update check --remote` CLI + 6 项单测 | [spec](done/GAP-45-auto-update/spec.md) |

## 集成

| GAP | 简述 | 详情 |
|-----|------|------|
| GAP-49 | 插件系统：目录扫描 + `plugin.toml` 清单解析/校验 + `install` / `uninstall` + `list` 报告 manifest 错误 + 9 项单测（运行时仍延后到 Phase D） | [spec](done/GAP-49-plugin-system/spec.md) |
| GAP-50 | 云同步：本地 bundle（`~/.config/pureterm/sync/latest.json`）+ GitHub Gist push/pull（reqwest + bearer auth）+ schema 版本控制 + `pureterm sync push/pull --remote` CLI + 8 项单测 | [spec](done/GAP-50-cloud-sync/spec.md) |
| GAP-51 | 块级输出：补齐 `OutputBlock`（session_id/cwd/started_at_us/kind/tags）+ `capture_row_range_async` 走 `spawn_blocking` 不卡帧 + `BlockSumTree` 增量 push + `PaneBlockStore::filtered` 单一过滤源 + 侧栏分页（PAGE_SIZE=50，去掉 64 行硬切）+ 事件名 `block.finished`（原 `shell.block_finished`，无外部消费者直更）+ 7 + 10 + 9 单测 | [spec](done/GAP-51-block-based-output/spec.md) |
| GAP-52 | Workflows：参数化 `{{key}}` 展开 + 缺失参数检查 + 类别（palette 分组）+ 别名（name 优先）+ `pureterm workflow run` CLI + 7 项单测 | [spec](done/GAP-52-workflows/spec.md) |
| GAP-53 | Notebooks：`.pureterm` JSON cell 格式 + cell 执行引擎（`$SHELL -c`，捕获 stdout/stderr）+ `pureterm notebook show/new/run` CLI + 9 项单测 | [spec](done/GAP-53-notebooks/spec.md) |

---

## 性能优化

| 计划 | 简述 | 进度 | 详情 |
|------|------|------|------|
| render-input-lag | 渲染输入延迟优化（Batch 1 已完成，Batch 2-3 待执行） | 1/3 | [plan](done/render-input-lag/plan.md) |

---

**更新约定**：新特性闭环 → 从 [`progressing.md`](progressing.md) 移入对应类别分表 → 更新 `done/GAP-XX/spec.md` frontmatter → Review 通过后 `git mv` 到 `reviewed/`。
