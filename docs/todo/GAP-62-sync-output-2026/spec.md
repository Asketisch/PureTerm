# GAP-62 — Synchronized Output (DEC private mode 2026) 真正生效

## 背景 / 动机

DEC private mode 2026（BSU/ESU = Begin/End Synchronized Update）是现代全屏 TUI Agent（Claude Code / Codex CLI / Cursor CLI，均用 Ink 或 Ratatui）防撕裂的关键手段。程序在重绘前发 `CSI ? 2026 h`，重绘后发 `CSI ? 2026 l`，终端应在 BSU 期间累积 damage、ESU 时一次性提交一帧。

PureTerm 现状：vte 0.15 解析层把 2026 映射到 `NamedPrivateMode::SyncUpdate`，但 `term/mod.rs` 的 set/unset/report 三处都是**空操作**（`mod.rs:2025,2074,2117`），且 `TermMode` bitflags 无对应位，渲染层完全不消费。结果：解析器吃掉了序列，但 BSU/ESU 对渲染毫无去抖/批量收益，Agent 全屏 TUI 高频重绘仍可能撕裂。

参考：[Synchronized Output spec (christianparpart gist)](https://gist.github.com/christianparpart/d8a62cc1ab659194337d73e399004036)；Claude Code 已默认使用 mode 2026（issue #32635、#44795）。

## 目标

1. BSU/ESU 真正驱动渲染层批量提交，消除高频重绘撕裂。
2. mode report 返回真实状态，满足 Agent 运行时探测。

## 现状证据

- vte 解析：`vte-0.15.0/src/ansi.rs:916`（`2026 => SyncUpdate`）、`:967`（`SyncUpdate = 2026`）、`:1606-1609`（opaque 提前终止帧）
- 空操作：`crates/alacritty_terminal/src/term/mod.rs:2025`（set `SyncUpdate => (),`）、`:2074`（unset `=> (),`）、`:2117`（report `=> ModeState::Reset`）
- TermMode bitflags `mod.rs:56-90` 无 `SYNC_UPDATE` 位
- 渲染层零消费：`grep sync_update|synchronized|SYNC_OUTPUT` 在 `crates/pureterm/src/` 无命中

## 变更点

### A. TermMode 增加 flag
`crates/alacritty_terminal/src/term/mod.rs` bitflags（`mod.rs:56-90`）：
```rust
const SYNC_UPDATE = 1 << 23;
```
（注：`1 << 18..22` 已被 Kitty keyboard 占用；`1 << 23` 空闲）

### B. set / unset / report 接通 flag
- set（`mod.rs:2025`）：`NamedPrivateMode::SyncUpdate => self.mode.insert(TermMode::SYNC_UPDATE),`
- unset（`mod.rs:2074`）：`NamedPrivateMode::SyncUpdate => self.mode.remove(TermMode::SYNC_UPDATE),`
- report（`mod.rs:2117`）：`NamedPrivateMode::SyncUpdate => self.mode.contains(TermMode::SYNC_UPDATE).into(),`

### C. 渲染层消费 flag（去抖）
渲染侧 `crates/pureterm/src/iced_app.rs` 的 draw / damage 调度：
- 当 active terminal `mode.contains(TermMode::SYNC_UPDATE)` 时，标记当前 pane 处于 sync-batching 状态；
- 期间 `Wakeup` 不立即触发重绘请求（或设 dirty 标记但不 `request_redraw`）；
- 一旦 flag 被 unset（ESU），立即把累积的 dirty 区域一次性提交一帧。

> 实现细节：在 terminal context 上加一个 `sync_batching: bool` 字段；`update` 收到 `TerminalEvent` 时检测 mode 变化（或在 PTY reader 线程的 parser 回调里感知）。鉴于 mode 变化在 term 线程内发生、UI 侧需感知，最干净的方式是在每帧构建 draw list 时读 `term.mode().contains(SYNC_UPDATE)` 决定是否 `request_redraw`。

### D. 单元测试
- 新增 `mod tests_sync_2026`（`term/mod.rs` 测试模块内）：set/unset/report 三态正确。
- 渲染层不写集成单测（依赖 wgpu），靠手测。

## 验证 / Test Plan

**单测**：
- `set_private_mode(2026, SetMode)` 后 `mode.contains(SYNC_UPDATE)` 为 true。
- `unset_private_mode(2026)` 后为 false。
- `report_private_mode(2026)` 在 set 时返回 `Set`，unset 时返回 `Reset`。

**手测**：
```sh
# 高频闪烁测试（无 2026 时撕裂，启用后应平滑）
python3 -c '
import sys,time
sys.stdout.write("\x1b[?2026h")  # BSU begin
for i in range(200):
  sys.stdout.write(f"\x1b[H{i}: line\n"*5)
sys.stdout.write("\x1b[?2026l")  # ESU end
sys.stdout.flush()
'
```
预期：BSU 期间无中间帧闪烁，ESU 后整屏刷新一次。

**回归**：`cargo nextest run -p alacritty_terminal` 全绿。

## 优先级 / 范围

- **优先级**：P0（基础必需层）
- **范围**：TermMode flag + 三处接通 + 渲染去抖调度
- **不做**：不引入 frame timing / VSync 改造，仅在现有 redraw 调度上叠加 batching
