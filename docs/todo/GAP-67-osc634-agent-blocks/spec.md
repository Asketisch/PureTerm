# GAP-67 — OSC 634 Agent 语义块协议

## 背景 / 动机

OSC 634 是**首个专为 LLM Agent ↔ 终端通信设计的语义协议提案**（Claude Code issue #44795）。它让 Agent 用结构化标记告知终端：这里是对话轮次起点、这里是代码块、这里是工具调用——终端据此提供 claude.ai 桌面端级别的 UX（代码块复制按钮、工具调用折叠、轮次导航）。

拟议格式（private-use，不与标准冲突，未知 OSC 被合规终端静默忽略，可穿透 ConPTY）：
- `634;A` 对话轮次起 / `634;B` 轮次止
- `634;C;lang` 代码块起 / `634;D` 代码块止
- `634;T;name` 工具调用起 / `634;U` 工具调用止
- `634;S` 流式 token 起止（可选）

PureTerm 若率先在宿主侧识别 634，将获得明显差异化。

参考：[Claude Code #44795](https://github.com/anthropics/claude-code/issues/44795)。

## 目标

1. 扫描器解析 `634;X[;params]`。
2. 新 `Event::AgentMark` + `AgentBlockMark` 数据类型。
3. GUI 侧 `AgentBlockStore` 收集标记，驱动 overlay（代码块复制、工具折叠、轮次导航）。

## 现状证据

- 全仓库 0 命中（提案级全新协议）
- 可复用：`event.rs` Event enum、`output_blocks.rs` sum-tree 模式、`scan_osc133` 扫描器模式

## 变更点

### A. 数据类型
`crates/alacritty_terminal/src/event.rs`：
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentBlockKind {
    TurnStart, TurnEnd,
    CodeStart { lang: Option<String> }, CodeEnd,
    ToolStart { name: Option<String> }, ToolEnd,
    StreamStart, StreamEnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentBlockMark {
    pub kind: AgentBlockKind,
    pub row: usize,           // grid row when detected
    pub pane_id: u64,         // which pane
}

// Event enum 加：
pub enum Event {
    ...
    /// GAP-67: OSC 634 agent semantic block mark.
    AgentMark(AgentBlockMark),
}
```
Debug impl 补一行。

### B. 扫描器
`event_loop.rs`，新增 `scan_osc634(buf) -> SmallVec<[AgentBlockMark; 4]>`（仿 `scan_osc133:613`），解析 `634;X[;params]`：
- `A`→TurnStart、`B`→TurnEnd、`C[;lang]`→CodeStart{lang}、`D`→CodeEnd、`T[;name]`→ToolStart{name}、`U`→ToolEnd、`S`→StreamStart、`R`→StreamEnd（流式起止用 S/R 避免与 A/B 歧义）
- 未知 X 忽略
- 在 `event_loop.rs:166` 循环接入

### C. GUI Store + Overlay（状态存储与浮层）
`crates/pureterm/src/agent_blocks.rs`（新模块，仿 `output_blocks.rs`）：
- `AgentBlockStore`：按 pane 维护标记序列，支持 sum-tree 增量、按 row 过滤
- `iced_app.rs`：`TerminalEvent::AgentMark` → store.insert；构建 draw 时读 store
- Overlay：代码块（C...D）边界 → 渲染「复制」按钮图元；工具调用（T...U）→ 可折叠 chevron；轮次（A...B）→ 侧栏导航列表

> 本轮最小闭环：扫描器 + Event + Store + 基础 overlay（代码块复制按钮）。折叠/导航视进度。

### D. 测试
`mod tests_osc634`：覆盖各 kind、params 解析、跨 buffer 边界、未知忽略。

## 验证 / Test Plan

**单测**：scan_osc634 各 kind + lang/name 解析。

**手测**：
```sh
printf '\x1b]634;C;rust\x07fn main() {}\n\x1b]634;D\x07\n'
```
预期：代码块出现复制按钮。

**回归**：新 Event variant 需在 `TerminalEvent`（pureterm crate 包装）补 match arm；确认所有 match 穷尽。

## 优先级 / 范围

- **优先级**：P1（差异化主线，本轮最小闭环）
- **范围**：扫描器 + Event + Store + 代码块复制 overlay
- **不做**：工具调用折叠/轮次导航的完整交互（留 spec 续做）、与 LLM 流式的深度集成
