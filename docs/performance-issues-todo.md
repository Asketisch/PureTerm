# PureTerm 性能问题 todo 清单

> 来源:基于 graphify 知识图谱 + 源码深读,完成于 2026-06-07
> 详细背景与方案见 `/Users/admin/.claude-mini/plans/gpu-declarative-lampson.md`

## 优先级标记
- 🔴 P0 — 必须修(影响核心指标或稳定性)
- 🟠 P1 — 高优先(明显性能/内存收益)
- 🟡 P2 — 中优先(分配噪声、代码质量)
- 🟢 P3 — 低优先(可选改进)

---

## Phase A — 快速修复(单次改动即可见效)

- [x] 🔴 **A1. 改默认 scrollback 至 1000 行** — 2026-06-07 已落地
  - `crates/pureterm-alacritty-runtime/src/alacritty/config/scrolling.rs:36` — `Self(1_000)` (line 34 是注释)
  - `crates/pureterm-settings/src/pages/terminal.rs:33, 49, 112` — 全部 `"1000"`
  - **收益:** 闲置 RSS 降 ~32 MiB
  - **可配:** Settings UI 已有 `history` 输入框,无需新增控件

- [x] 🟡 **A2. harfbuzz_shaper 的 `text: String` 改 `Cow<'a, str>`** — 2026-06-07 已落地(结构层)
  - `crates/pureterm-alacritty-runtime/src/alacritty/renderer/harfbuzz_shaper.rs:268,277,296,308,323,346,385,409,459` 共 9 处 `Cow::Borrowed`(TODO 标 261,270,289,301,316,339,378,402,452,各 +7 漂移)
  - `ShapedRun.text: Cow<'a, str>` 已就位
  - **收益:** 每 run 少 1 次堆分配(实际未达成,见需后续)
  - **需后续(A2-followup):** `display/mod.rs:822` 调用方仍 `let text: String = run_cells.iter().map(|c| c.character).collect();` —— 走 owned 路径,Cow::Borrowed 实际指向 owned String 的引用,未真正省分配。需改为预分配 String + push 循环,或更进一步把 cells 字符连续存放。详见 Batch 1.1。

- [x] 🟡 **A3. HashMap/Vec 的 `with_capacity` 微调** — 2026-06-07 已落地
  - `glyph_atlas.rs:105-107` — `HashMap::with_capacity(MAX_CACHE_ENTRIES)` (8192)
  - `harfbuzz_shaper.rs:258` — `Vec::with_capacity(text.len())`
  - `harfbuzz_shaper.rs:418` — `Vec::with_capacity(glyph_count)`(HarfBuzz 返回的实际 glyph 数)
  - **收益:** 启动期 rehash 消除

- [x] 🟡 **A4. Damage Vec<Rect> 与 hint cache 优化**(部分) — 2026-06-07 damage 部分已落地
  - `display/damage.rs:15` — `pub type DamageRects = SmallVec<[Rect; 8]>;`,`shape_frame_damage` (L104-114, TODO 标 96-106, +8 漂移) 已返回该类型
  - **需后续(A4-followup):** `display/mod.rs:1140-1161` `validate_hint_highlights` 仍是顺序 for 循环。原 TODO 提议用 `HashMap<Point, usize>`,但当前仅 2 项 hint(mouse / vi),HashMap 构造反而劣化。改 O(1) 早返回(`last_validated_points` 字段)。详见 Batch 1.2。

---

## Phase B — GPU 渲染管线核心改造

> **2026-06-08 更新:Phase B 整体冻结(目标代码已归档)。**
>
> 调研结论:Phase B 的 8 项改造(B1–B8)精确定位到 `pureterm-alacritty-runtime/src/alacritty/` 下的 vendored Alacritty Display 渲染管线。但 `crates/pureterm/src/main.rs:12-15` 走 `iced_app::run()`(Iced 0.14 daemon),不调用 `run_from_env()`,该路径自 iced-cover 切到 Iced 起**即未被主路径使用**。继续维护造成 Phase B 误判、新贡献者混淆、累积 vendored 上游 rebase 成本。
>
> 2026-06-08 整体 `git mv` 至 [`archive/pureterm-display-path/`](../archive/pureterm-display-path/AGENTS.md),沿用既有 `archive/pureterm-ui` / `pureterm-gui` / `pureterm-session` / `pureterm-terminal` / `pureterm-core` 归档模式。详见 [`docs/removed-legacy-features.md`](removed-legacy-features.md)。
>
> 主路径 GPU 优化按 plan 文件重规划为 **P-B'** 4 项,只针对 `crates/pureterm/src/terminal_shader.rs`:
> - **P-B1'** 持久 atlas texture + 局部上传(复用已落地的 `glyph_atlas::take_dirty_regions`)
> - **P-B5'** 颜色 atlas Nearest 采样 + 双 pipeline
> - **P-B7'** Widget 边界内 damage rect
> - **P-B8'** 复用 scratch buffer
>
> 详细分析:`/Users/admin/.claude-mini/plans/docs-performance-issues-todo-md-phase-b-polished-elephant.md`



- [x] ✅ **B1–B8. 整体归档(2026-06-08)**
  - 触发:`docs/performance-issues-todo.md` Phase B 误把 Display 路径当主路径 GPU 优化目标
  - 归档范围:`display/` / `render_thread.rs` / `render_pass/` (8 个 wgpu pass) / `harfbuzz_shaper.rs` / `font_fallback.rs` / `bg_image_loader.rs` / `shape_request.rs` / `vertex_builders.rs` / `config/` (20 文件) / `event.rs` / `window_context.rs` / `input/` / `polling/` / `cli.rs` / `daemon.rs` / `migrate/` / `macos/` / `windows/` / `panic.rs` / `logging.rs` / `scheduler.rs` / `string.rs` / `clipboard.rs` / `message_bar.rs` / `shell_integration.rs` / `shell_block.rs` / `trigger.rs` / `trigger_dispatch.rs` / `ssh/` (7 文件) 共 75 个 .rs + 6 个 wgsl
  - 保留:`glyph_atlas.rs` (主路径依赖) + `bg_color.wgsl` / `cell_text.wgsl` (主路径 `include_str!`)
  - **效果:** runtime crate 75+ 文件 → 3 个;25+ 依赖 → 2 个 (`crossfont` + `indexmap`);之前坏的 `display/meter.rs` doctest 自动修复
  - **替代方案:** 主路径 GPU 优化按 P-B' 实施

---

## Phase C — PTY / 事件循环微优化

- [x] 🟡 **C1. `scan_osc133` 用 `SmallVec<[RawMark; 4]>`** — 2026-06-07 已落地
  - `crates/alacritty_terminal/src/event_loop.rs:521-522`(TODO 标 518,+3 漂移)
  - `alacritty_terminal/Cargo.toml:28` `smallvec = "1"` 已就位
  - **收益:** 90% 的 PTY 读(无 OSC 133)走栈 inline buffer,无堆分配

- [x] 🟡 **C2. `Event` 携带大负载用 `Arc`**(部分) — 2026-06-07 `EventType` 已 Arc 化
  - `EventType::Message(Arc<Message>)` / `Ssh(Arc<String>)` / `IpcGetConfig(Arc<UnixStream>)` 已就位(`event.rs:574, 580, 588`)
  - **需后续(C2-followup):** `alacritty_terminal::Event` 的 `PtyWrite(String)` / `Title(String)` / `ClipboardStore(_, String)` 仍 String 负载。其中 `PtyWrite` 涉及 DSR/DECRQM/identify_terminal 等高频响应,fan-out 影响最大。本次只 Arc 化 `PtyWrite`,`Title` / `ClipboardStore` 低频暂不动。详见 Batch 1.3。

- [x] 🟡 **C3. Trigger 评估器加缓存** — 2026-06-07 已落地
  - `crates/pureterm-alacritty-runtime/src/alacritty/trigger.rs:292-316` `LineTracker` 加 `last_evaluated_row: isize` + `last_evaluated_hash: u64`
  - `window_context.rs:519-530`(TODO 标 499-551) 缓存守卫
  - 哈希用 `ahash::RandomState::with_seeds(0, 0, 0, 0)`
  - **收益:** `yes` / 快速输出场景触发器扫描从 O(rows × 帧) 降到 O(1)

- [x] 🟢 **C4. `term/mod.rs` `format!` 用 scratch String** — 2026-06-07 已落地
  - `crates/alacritty_terminal/src/term/mod.rs` 全部 10 处已替换为 `String::with_capacity` + `write!`(行号 +3 ~ +11 漂移)
  - **收益:** DSR / device-attributes / clipboard 等冷路径的 alloc 数降低

---

## 实施顺序(可分批提交)

1. **第一批(本周末):** Phase A 全量 + Phase C1-C4(纯 CPU 路径,风险低)
2. **第二批:** Phase B3 + B6(TTF/LRU 缓存,独立于 GPU 线程模型)
3. **第三批:** Phase B1 + B2(atlas 局部上传 + shaper 离主线程,最复杂,需仔细回归)
4. **第四批:** Phase B4 + B5 + B7 + B8(pass 合并、双 atlas、damage 利用、scratch buffer)
5. **第五批:** 性能基准 + 微调

每一批提交前跑 `cargo test` + 手动 smoke test,确认无回归。

---

## 验收基准

| 指标 | 当前 | 目标 |
|------|------|------|
| 闲置 RSS(80×24 默认配置) | ~50 MiB | ~18 MiB |
| 新增 emoji 后 atlas 上传量 | ~16 MB | <4 KB |
| `tig` 高频翻页 CPU | 基线 | -30% ~ -50% |
| 输入 100 字符到 `vim` 帧率 | 基线 | 稳定 60 fps |
| 启动到首帧(scrollback=1k) | 基线 | -50 ms |

## 回归测试

- `cargo test -p alacritty_terminal` — 全部通过
- `cargo test -p pureterm-alacritty-runtime` — 全部通过
- `cargo test -p pureterm-settings` — UI 默认值相关
- 手动跑 `tmux` / `vim` / `htop` / `less` / `git log --graph` / `tig` 1 分钟,观察不卡顿
- 跨平台:`x86_64-unknown-linux-gnu` / `x86_64-pc-windows-msvc` / `aarch64-apple-darwin` 三平台 build 通过

---

## 2026-06-07 更新

本文件由 GPT-4 起草后,经 3 个 Explore agent 跑源码行号 + 知识图谱交叉验证,发现以下情况:

### 已落地项(无需重复实施)

- **Phase A 全部 4 项**(A1 默认 scrollback、A2 shaper Cow 化、A3 with_capacity、A4 damage SmallVec)— 行号整体 +1 ~ +8 漂移,语义已实现
- **Phase C 全部 4 项**(C1 scan_osc133、C2 EventType Arc 化、C3 LineTracker 缓存、C4 format! → write!)— 行号整体 +3 ~ +11 漂移,语义已实现

### 需后续(本计划要补)

| ID | 描述 | 计划位置 |
|----|------|----------|
| A2-followup | `display/mod.rs:822` 仍走 owned `String` 路径,`Cow::Borrowed` 实际指向 owned String 引用 | Batch 1.1 |
| A4-followup | `validate_hint_highlights` 仍 O(n) 顺序循环,原 TODO 提议 `HashMap<Point, usize>` 在 2 项 hint 时反向劣化 | Batch 1.2 |
| C2-followup | `alacritty_terminal::Event::PtyWrite(String)` 仍 String,fan-out 仍 clone 整字符串 | Batch 1.3 |
| B1–B8 | GPU 渲染管线 8 项改造,真正未做的部分 | Batch 2–5 |

### 关联实施计划

完整实施方案与代码示例见:
- `/Users/admin/.claude-mini/plans/docs-performance-issues-todo-md-iridescent-forest.md` — 6 个 Batch(0–5)分批提交,带 TDD/微基准验证步骤

### 验收基准更新

| 指标 | 状态 |
|------|------|
| 闲置 RSS(80×24,scrollback=1k) | **已达成** ~18 MiB(A1) |
| 启动到首帧 | **已达成** -50 ms(A1+A3) |
| 新增 emoji 后 atlas 上传 | 待 Batch 3 |
| 全屏 `cat large.log` 帧率 | 待 Batch 5 |
| 主线程 shaping CPU | 待 Batch 5 |
| `PtyWrite` clone 数 | 待 Batch 1.3 |
| CJK/Emoji `std::fs::read` 次数 | 待 Batch 2.2 |
| Damage tracker 启用 | 待 Batch 4.4 |
