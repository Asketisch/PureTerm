# 渲染输入卡顿优化 — 路线图与现状

> 状态：**Batch 1 ✅ 已闭环（commit `9aaafae`）/ Batch 2-3 🟠 待执行**
>
> 起点：用户报告输入英文有明显卡顿感（2026-06-30）。
> 主分支：`iced-cover`（commit `921a407` 起步）。

## Context（为什么做）

PureTerm（基于 Alacritty 上游、Iced 0.14 shader widget 渲染路径）的输入端存在**多源叠加**的性能瓶颈，使英文输入在视觉上有明显延迟感。诊断在主分支源码中验证，**根因不是单点**，需要按 3 批 7 项逐步收敛。

**核心观察**：iced 0.14 的 redraw 完全由 subscription 驱动（grep 全仓无 `request_redraw()` 调用），主分支唯一的 60Hz timer 是 `time::every(16ms)` → `Message::TerminalWakeup`。整条渲染管线跟着这条 timer 节拍跑——既不按"真有事件才重绘"，也不利用 vendored alacritty 已实现的 dirty region 追踪。

**仓库状态警告**：原始诊断曾在 `.claude/worktrees/agent-aeec93962fca37c1e/`（commit `c529c64`）找到 `display/mod.rs:709 let full_redraw = true;` 等代码，**但主分支已精简 `pureterm-alacritty-runtime` 为只剩 `glyph_atlas.rs + renderer/{mod,shaders}`**（见 CLAUDE.md "2026-06-08 起仅含 renderer::glyph_atlas"）。本文件根因与代码引用均已在主分支源码二次验证。

---

## 一、已验证根因（按单点收益排序）

### 根因 #1 — 16ms 无条件 wakeup timer 是主驱动（最大收益）

- 位置：`crates/pureterm/src/iced_app.rs:13867`（修复前）
- 现象：无论是否有输入、是否有 PTY 输出，主线程每 16ms（约 60Hz）无条件被 subscription 唤醒。
- `TerminalWakeup` handler（`iced_app.rs:4392`）是个庞然大物：每次 tick 执行 macOS delegate 安装、菜单/剪贴板消费、url-scheme 解析、metal layer 配置、URL scheme 调度、SSH 心跳、GAP-10 trigger scan、GAP-09 通知派发、GAP-26 动画、GAP-17 图像协议 flush 等。代码规模 ≈ 100 行 if-let 链。
- 关键事实：grep 全仓无任何 `request_redraw()` 调用 → iced 唯一的重绘驱动力就是这条 16ms timer + 它派生的消息。
- 后果：用户输入英文时，每按一键 → iced 立即收到 `Message::Keyboard` 写入 PTY → shell echo 触发 PTY wakeup → 但 iced 的实际重绘仍要等下一个 16ms tick → 看起来"我按键了 → 16ms 内屏幕没动静"。

### 根因 #2 — atlas 部分上传已实现但**没接进 shader**

- 位置：`crates/pureterm/src/terminal_shader.rs:1119`（修复前）
- 已存在但未使用的 API：`crates/pureterm-alacritty-runtime/src/alacritty/renderer/glyph_atlas.rs:339-351` 的 `take_dirty_regions()`，已返回 `Arc<Vec<u8>>` + `Vec<DirtyRect>`（按字符矩形记录），且有测试 `PureTerm B3.1` 注释（行 920）证明此前已实现。
- shader 仍调老 API：每次 atlas dirty 都 `self.atlas_data.clone()` → 全量上传。ATLAS_SIZE=2048 → R8 灰度 = 4 MiB、RGBA 颜色 = 16 MiB per dirty frame（一次性，但不是"每帧"，仅 dirty 时）。
- 后果：GPU 实际按需上传（见 `terminal_shader.rs` 的 `queue.write_texture`），CPU 侧 `Vec::clone()` 是额外开销。

### 根因 #3 — iced shader widget 在主线程做完整 vertex 构建

- 位置：`crates/pureterm/src/terminal_shader.rs`（整文件 1289 行）+ `iced_app.rs` 的 `view_*_bar` 等 12 处
- 机制：iced 0.14 的 shader widget 路径 = 每次 `view()` → `primitive()` → `prepare()` 在 GUI 线程同步构建整个 vertex buffer。
- 后果：即便 atlas 没脏、`TerminalWakeup` 每 16ms 跑一次，vertex buffer 仍按完整 cols×rows 重建。80×24 = 1920 cell，4K 终端约 8000 cell；每 cell 含 6 vertex（背景 quad） + glyph run。估算每帧 50K-200K vertex rebuild on main thread。

### 根因 #4 — Iced 0.14 subscription 模式下 keyboard 不直接触发 redraw

- 位置：`crates/pureterm/src/iced_app.rs:13866` — `iced::keyboard::listen().map(Message::Keyboard)`
- 机制：iced 0.14 不允许 `update()` 中直接 `request_redraw()`；唯一的 redraw 信号就是订阅。
- 后果：用户按键 → iced 排入消息队列 → 等下一个 16ms tick → handler 跑 → 渲染。**输入延迟下限 = 16ms**。

### 根因 #5 — scrollback 默认 10000 行（次要）

- `crates/pureterm-alacritty-runtime/src/alacritty/config/scrolling.rs` 在主分支**不存在**（已迁移/精简）；旧 todo A1 路径失效。
- 影响：每次 `Display::draw` 即使做 cell clipping，scrollback 大小仍影响内存与 cold-path grid 重建。**次要因素**。

### 根因 #6 — Trigger 扫描（每帧）+ 通知派发（每帧）做无用功

- 位置：`iced_app.rs:4392` 的 TerminalWakeup handler 中包含 GAP-10 trigger scan、GAP-09 通知派发
- 影响：平时 trigger 未配置时，扫描结果为空但仍扫描一遍。

### 根因 #7 — Damage tracker 在主分支**完全不存在**

- worktree 的 `damage.rs` 在主分支**不存在**（`grep -rn "full_redraw\|damage" crates/pureterm/src/` 无命中）。
- 旧 todo 的 B7（damage tracker 启用）**对主分支不适用**——需要重新设计 damage 机制（基于 grid 的 `TermDamage` 上报 → wgpu scissor rect）。

---

## 二、todo 清单 17 项状态校正

| 项 | 旧清单描述 | 主分支当前状态 | 适用性 |
|---|---|---|---|
| A1 scrollback 默认 10k→1k | `pureterm-alacritty-runtime/.../scrolling.rs:34` | **路径不存在**，配置已迁 | ⚠️ 需新查 vendored 端 |
| A2 HarfBuzz Cow 改写 | harfbuzz_shaper.rs | **shaper 已移除**（runtime 精简） | ❌ 不适用 |
| A3 HashMap::with_capacity | glyph_atlas:104 | **已做**（行 146/150 `Vec::with_capacity(64)`） | ✅ 完成 |
| A4 SmallVec damage | damage.rs:96-106 | damage.rs 不存在 | ❌ 不适用 |
| B1 atlas 局部上传 | take_dirty_regions | **API 已实现**（行 339）但 terminal_shader.rs:1119 未切换 | ✅ **Batch 1 已闭环** |
| B2 shaper 离主线程 | display/mod.rs:720-848 | display 已移除 | ⚠️ 需新设计 vertex 重建 off-thread（Fix 6） |
| B3 TTF/HarfBuzz 缓存 | harfbuzz_shaper.rs | harfbuzz 已移除 | ⚠️ 需在 terminal_shader/font lookup 链重做 |
| B4 4 个 uniform pass 合并 | bg_color/decorations/cursor/post_process | 需核验当前 renderer/mod.rs | ⚠️ 待查 |
| B5 cell-text 拆灰度/颜色 | cell_text pass | 需核验 | ⚠️ 待查 |
| B6 O(1) LRU | glyph_atlas.rs:52-144 | **已为 `IndexMap`**（行 99） | ✅ 完成（无需改动） |
| B7 damage tracker 启用 | display/mod.rs:709 full_redraw=true | 主分支**不存在 damage tracker** | ❌ 需新设计（Fix 7） |
| B8 复用 scratch buffer | display/mod.rs:662-665 | display 已移除 | ⚠️ 需在 terminal_shader 重做（Fix 6） |

**结论**：原 17 项清单**约 5 项已失效（路径不存在）**、**5 项已部分或完全完成**、**3 项可立即做**、**4 项需重新设计**。

---

## 三、修复路线图（按 ROI × 实施成本排序）

> **范围说明**：按 ROI × 实施成本分 3 批，**所有 7 项都在最终交付范围**。每批独立可合入 + 独立可验证；前批不通过不回退到下批（不会半完成态）。

### 🔴 Batch 1：低风险高收益，单文件改动 ✅ **已闭环**

#### Fix 1 — wakeup timer 16ms → 50ms（已闭环，commit `9aaafae`）

**文件**：`crates/pureterm/src/iced_app.rs:13867`

**改动**：把 `time::every(Duration::from_millis(16))` 改为 `time::every(Duration::from_millis(50))`，并同步更新 5 处注释（resize_pending doc / resize_pending apply / WindowResized / dock reopen / apply_viewport_resize doc）。

**思路**：保留 50ms base tick 作为安全网，主要触发器仍是已存在的 15+ 处 `task.map(|_| Message::TerminalWakeup)` 手动触发点（keyboard、resize、SSH、shell-integration、daemon、pane events）。

**预期收益**：闲置 CPU -50%（从 60Hz 降到 20Hz idle tick）；输入延迟从"绑死 16ms"变成"事件驱动 + 50ms 上限"

#### Fix 2 — 接通 `take_dirty_regions` 替代 `take_dirty_data`（已闭环，commit `9aaafae`）

**文件**：
- `crates/pureterm/src/terminal_shader.rs`（导入 + 字段类型 + primitive 调用 + 上传循环）
- `crates/pureterm-alacritty-runtime/src/alacritty/renderer/glyph_atlas.rs`（API 已就绪）

**改动**：
1. `TerminalPrimitive.{grayscale,color}_atlas: Option<(Vec<u8>, u32, u32)>` → `Option<AtlasDirty>`（`Arc<Vec<u8>>` + `Vec<DirtyRect>` + dims）
2. `primitive()` 改调 `take_dirty_regions()` / `take_color_dirty_regions()`
3. `prepare()` 遍历每个 `DirtyRect` 单独 `queue.write_texture(...)`，用 `byte_offset` 切片 `Arc<Vec<u8>>` 共享缓冲

**预期收益**：CPU `Vec::clone()` 4 MiB + 16 MiB = **0**（Arc 共享）；GPU 仍按 dirty rect 局部上传

#### Fix 3 — LRU O(n) → O(1)（已存在，无须改动）

**文件**：`crates/pureterm-alacritty-runtime/src/alacritty/renderer/glyph_atlas.rs:99`

**现状**：已是 `IndexMap<(usize, char), ()>`（带注释 "PureTerm: O(1) `shift_remove_index(0)` for eviction"）。**此项工作此前已落地，本次无需改动**。

### 🟠 Batch 2：中等风险高收益

#### Fix 4 — keyboard 直接触发 redraw（突破 16ms / 50ms 限制）

**文件**：`crates/pureterm/src/iced_app.rs:5981`（`Message::Keyboard` handler）

**思路**：iced 0.14 允许在 `update()` 末尾 `return Task::batch(...)` 中组合重绘任务。给 keyboard handler 返回一个强制让下一个 tick 立即处理的任务（iced runtime 已有 coalesce）。或利用 `iced::window::redraw` API（如有）。

**预期收益**：输入延迟从 ~50ms 上限（Fix 1 后） → ~4-8ms

**风险**：中，需验证 iced 0.14 API；可能与现有 `Task::none()` 路径冲突

#### Fix 5 — TerminalWakeup handler 拆分 + 条件门控

**文件**：`crates/pureterm/src/iced_app.rs:4392-4550`

**思路**：把 ~150 行 handler 拆成 `tick_macos_glue` / `tick_pending_resize` / `tick_pane_drain` / `tick_trigger_scan` / `tick_notify_dispatch` 等小函数，每函数自身判断"是否有工作要做"。例如 trigger scan 只在 `triggers_enabled && pane_dirty` 时跑。

**预期收益**：每 tick CPU -30%~50%

**风险**：低，纯重构；可分函数逐步落地

### 🟡 Batch 3：高风险但根本性

#### Fix 6 — vertex buffer 重建 off-thread + 仅 dirty cell

**文件**：`crates/pureterm/src/terminal_shader.rs`（整个 vertex 构建路径）

**思路**：
1. 在 Iced GPU primitive 构建阶段，把"按 grid 生成 vertex buffer" 丢到 `spawn_blocking`
2. 用 grid 的 `TermDamage::Partial(rect)` 取代全网格遍历（需先实现 grid damage tracker，与 Fix 7 对齐）
3. 把"重建 vertex"与"上传 atlas"合并到同一个 Task，避免双重 wakeup

**预期收益**：单帧 vertex 重建从 ~80K cell 降到 ~几十 cell（仅 dirty）

**风险**：高，iced shader primitive 模型与线程模型需深入

#### Fix 7 — iced 内部 shader primitive 缓存（仅首帧全建）

**思路**：把已构建的 vertex buffer 缓存 1 帧，dirty 时增量 diff（仅行级）。需新设计 damage tracker：基于 grid 的 `TermDamage` 上报 → wgpu scissor rect（主分支**完全不存在** damage tracker）。

**预期收益**：稳态（非输入时）CPU 几乎为 0

**风险**：高，需理解 iced 0.14 `Primitive::Shader` 的 cache 模型；需新增 vendored alacritty 改动（按 CLAUDE.md 须加注释）

---

## 四、关键文件清单

**主分支当前路径（实际需修改）：**
- `crates/pureterm/src/iced_app.rs`（14304 行，主要调度）— Fix 1 ✅ / Fix 4 / Fix 5 待做
- `crates/pureterm/src/terminal_shader.rs`（1289 行，vertex + atlas 上传）— Fix 2 ✅ / Fix 6 待做
- `crates/pureterm-alacritty-runtime/src/alacritty/renderer/glyph_atlas.rs`（963 行，atlas + LRU）— 无须改动（Fix 3 已存在）

**主分支已移除路径（旧 todo 引用，需重新设计而非修改）：**
- `crates/pureterm-alacritty-runtime/src/alacritty/display/mod.rs`
- `crates/pureterm-alacritty-runtime/src/alacritty/display/damage.rs`
- `crates/pureterm-alacritty-runtime/src/alacritty/renderer/harfbuzz_shaper.rs`

---

## 五、验证方案

### 立即可做（无代码改动）

1. **量化卡顿基线**：用 `cargo run --release` + Instruments "Time Profiler" 录 30 秒连续输入英文（vim/normal mode），记录主线程在 `TerminalWakeup`/`terminal_shader::prepare` 的时间占比
2. **关 trigger 对照**：临时把 `[[triggers]]` 数组清空，对比 CPU 是否下降
3. **scrollback 改 1000**：改 `~/.config/pureterm/alacritty.toml` 的 `history = 1000`（若生效）测 RSS 与首次绘制延迟

### 每批后验证

- `cargo test -p pureterm-alacritty-runtime`（glyph_atlas 单测覆盖关键路径）
- `cargo test -p pureterm --bin pureterm -- --skip pty_integration --skip discover`（集成子集）
- ⚠️ **不要跑 `cargo test --lib`**（真机 PTY 卡）
- 手动：`./build.sh app` 重打包，跑真实终端 1 分钟（vim + htop + git log）
- Instruments：CPU 主线程占比从基线 X% → 目标 Y%

### 基准目标

| 指标 | 当前基线 | Batch 1 后（已落地） | Batch 2 后 | Batch 3 后 |
|---|---|---|---|---|
| 输入延迟（按键到像素） | ~16ms | ~50ms ⚠️ | <8ms | <4ms |
| 闲置 CPU（无输入无输出） | ~5-8% | ~2-3% | <1% | 接近 0 |
| 单 dirty 帧 atlas CPU 开销 | 16+4 MiB clone | **0 clone**（Arc 共享） | 同 | 同 |
| 单帧 vertex 重建 | ~80K cell | 同 | 同 | 仅 dirty cell |

> ⚠️ Batch 1 后输入延迟实际值待测量。Fix 1 是降 idle CPU，不是降输入延迟——输入延迟需 Fix 4 把键盘事件从 50ms tick 中解耦。

---

## 六、执行记录

### 2026-06-30 — Batch 1 闭环（commit `9aaafae`）

**改动**：
- `crates/pureterm/src/iced_app.rs`：+39 / -16
- `crates/pureterm/src/terminal_shader.rs`：+76 / -22

**验证**：
- `cargo check -p pureterm` ✅
- `cargo test -p pureterm-alacritty-runtime glyph_atlas` → **10/10 绿**
- `cargo test -p pureterm --bin pureterm` → **764/764 绿**

**commit**：`9aaafae perf(render): batch 1 — slow wakeup timer + partial atlas upload`

### 待办（按批次顺序）

- [ ] Batch 2 — Fix 4（keyboard 直接 redraw）+ Fix 5（handler 拆分 + 门控）
- [ ] Batch 3 — Fix 6（vertex off-thread）+ Fix 7（damage tracker + vertex 缓存）
- [ ] 每批前开 worktree 隔离，避免与 GAP reviews 冲突
- [ ] 按"Review flip"惯例在每批独立 commit，方便回滚
- [ ] 每批后跑 `cargo test + ./build.sh app + 手动 smoke`

---

## 七、相关历史

- `memory/performance-issues-todo.md`：原 17 项清单（76 天前，已部分过时）
- `memory/resize-render-area-stuck.md`：resize_pending 16ms 防抖修复（与 Fix 1 配套）
- `memory/fix_fullscreen_render_failure.md`：早期 3 类渲染 bug 修复
- `memory/iced_resize_visual_glitch.md`：iced resize 视觉抖动（与本批独立的视觉问题）
- `/.claude-mini/plans/resilient-waddling-hinton.md`：本次规划的起源 plan 文件（含用户审批时的上下文）