# PureTerm 已归档功能索引

> 归档原因统一说明:这些功能在新主路径(`iced-cover` 分支)上未被使用,继续维护会导致:
> 1. 误判 — 新贡献者以为这就是主路径,改完无人用
> 2. 成本 — 累积 vendored Alacritty 上游 rebase / vendored ghostty 分支 sync 工作
> 3. 误删风险 — 后续清理时易与主路径混淆
>
> 所有归档使用 `git mv`,保留完整历史;归档文件随时可通过 `git checkout HEAD~N -- <path>` 回滚。

## 归档列表

| 归档名 | 归档时间 | 用途 | 替代方案(主路径) | 详细 |
|--------|----------|------|------------------|------|
| `archive/pureterm-core` | 2026 年初 | 实验性核心库抽象 | `alacritty_terminal`(直接依赖) | [AGENTS.md](../archive/pureterm-core/AGENTS.md) |
| `archive/pureterm-terminal` | 2026 年初 | 实验性终端面板 | `alacritty_terminal` + `iced` widget | [AGENTS.md](../archive/pureterm-terminal/AGENTS.md) |
| `archive/pureterm-session` | 2026 年初 | 实验性会话管理 | `terminal_context.rs` 内联 | [AGENTS.md](../archive/pureterm-session/AGENTS.md) |
| `archive/pureterm-gui` | 2026 年初 | 实验性 GUI 框架层 | `iced` 直接使用 | [AGENTS.md](../archive/pureterm-gui/AGENTS.md) |
| `archive/pureterm-ui` | 2026 年初 | egui UI 组件层 | `iced` 0.14 + 自定义 wgpu | [AGENTS.md](../archive/pureterm-ui/AGENTS.md) |
| `archive/pureterm-display-path` | 2026-06-08 | Alacritty Display 8-pass wgpu 渲染管线 + 全套 vendored 模块 | `crates/pureterm/src/terminal_shader.rs`(Iced widget shader) | [AGENTS.md](../archive/pureterm-display-path/AGENTS.md) |

## 详细归档记录

### pureterm-display-path(2026-06-08)

**触发问题:** `docs/performance-issues-todo.md` Phase B (B1–B8) 误把 Display 路径当作主路径 GPU 优化目标,改完用户感知为 0。

**归档范围:**
- Display 8-pass 渲染管线(`display/`、`render_pass/`、`render_thread.rs`、6 个 wgsl shader)
- 字体处理(`harfbuzz_shaper.rs`、`font_fallback.rs`、`bg_image_loader.rs`、`shape_request.rs`、`vertex_builders.rs`)
- 配置系统整个 `config/` 目录(20 个文件)
- 事件循环 / IPC(`event.rs`、`window_context.rs`、`polling/`、`cli.rs`、`daemon.rs`、`migrate/`、`scheduler.rs`、`string.rs`、`logging.rs`、`clipboard.rs`、`message_bar.rs`、`input/`)
- Shell integration 与触发器(`shell_integration.rs`、`shell_block.rs`、`trigger.rs`、`trigger_dispatch.rs`)
- SSH 后端整个 `ssh/`(7 个文件,主路径的 SSH 在 `terminal_context.rs` 独立实现)
- 平台特定(`macos/`、`windows/`、`panic.rs`)

**保留:**
- `glyph_atlas.rs` — 主路径唯一依赖,提供 GPU 字体图集 LRU + dirty region API
- `bg_color.wgsl` + `cell_text.wgsl` — 主路径 `include_str!` 引用的 2 个 wgsl shader
- `lib.rs` + `renderer/mod.rs` — 精简为只导出 `renderer` 模块

**效果:**
- `pureterm-alacritty-runtime` 源文件数从 75+ 减到 3
- `Cargo.toml` 依赖从 25+ 减到 2(`crossfont` + `indexmap`)
- Release build 时间从 10.73s 降到 1m12s(冷启动)→ 增量 4.12s(因重 dep 解析)
- 之前坏的 `display/meter.rs` doctest 自动修复(随归档移走)
- Phase B (B1–B8) 整体冻结,主路径 GPU 优化重规划为 P-B' 4 项

**回滚:** `git checkout HEAD~1 -- crates/pureterm-alacritty-runtime/src/`

## 维护规则

- **不删 git 历史** — 用 `git mv` 而不是 `git rm`,便于回滚
- **保留 `AGENTS.md`** — 每个归档目录有详细的文件清单 + 替代方案 + 回滚步骤
- **不修改归档内容** — 归档目录是历史快照,bug 修复不 backport
- **回滚前先看 AGENTS.md** — 确认归档原因和当前主路径是否仍不需要

## 关联文档

- [CLAUDE.md](../CLAUDE.md) — 项目维护规则
- [performance-issues-todo.md](performance-issues-todo.md) — 性能优化 todo(Phase B 已归档,P-B' 重规划)
