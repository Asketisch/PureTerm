# GAP-66 — OSC 633 Shell Integration 扫描器

## 背景 / 动机

OSC 633 是 **VSCode 专用 shell integration 协议**（633;A/B/C/D/E/P），被在 VSCode 终端里运行的 Claude Code / Cursor 等强依赖。它比 FinalTerm OSC 133（PureTerm 已支持）多出 `E;cmdline;nonce`（显式命令）和 `P;Cwd=`/`P;IsWindows` 等属性。

为兼容在 VSCode 习惯下编写、会发 633 的 shell rc 脚本与 Agent，PureTerm 应识别 633，**映射到现有 `ShellMark` 体系**（复用 prompt 跳转 / exit code 显示 / block 输出）。

参考：[VSCode Shell Integration](https://code.visualstudio.com/docs/terminal/shell-integration)。

## 目标

1. 识别 `\x1b]633;A`/`B`/`C`/`D[;exitcode]`，映射到现有 `ShellMarkKind`（PromptStart/CommandStart/OutputStart/OutputEnd）。
2. 解析 `633;P;Cwd=<path>` → 更新 cwd（等同 OSC 7）。
3. 解析 `633;E;<cmdline>;<nonce>` → 填充 ShellMark.command（等同 133;B 的 cmd=）。
4. 不识别的 633 子命令静默忽略（符合"未知 OSC 容忍"约定）。

## 现状证据

- vte 不处理 633：`vte-0.15.0/src/ansi.rs:1523` 落入 `_ => unhandled(params)`
- 现有 133 扫描器：`event_loop.rs:613 scan_osc133`（`mod tests_osc133:697`）
- ShellMarkKind：`event.rs:9-20`

## 变更点

### A. 新增 scan_osc633
`crates/alacritty_terminal/src/event_loop.rs`，紧邻 `scan_osc133:613`：
```rust
/// Scan raw bytes for OSC 633 (VSCode shell integration).
/// `\x1b]633;X[;params]\x07` or `\x1b]633;X[;params]\x1b\\`.
/// A/B/C/D → ShellMark; P;Cwd= → cwd; E;cmd;nonce → command.
fn scan_osc633(buf: &[u8]) -> SmallVec<[Raw633; 4]> { ... }
```
返回 `Raw633 { kind: Option<ShellMarkKind>, cwd: Option<String>, command: Option<String> }`。

### B. 接入解析循环
`event_loop.rs:166` 之后，仿 133/7 的调用：
```rust
for raw in scan_osc633(&buf[..unprocessed]) {
    if let Some(kind) = raw.kind {
        self.event_proxy.send_event(Event::ShellMark(ShellMark { kind, row: cursor_row, exit_code, duration_us, command: raw.command }));
    }
    if let Some(cwd) = raw.cwd {
        self.event_proxy.send_event(Event::CwdChanged(cwd));
    }
}
```

### C. 测试模块
`mod tests_osc633`（仿 `tests_osc133:697`）：覆盖 A/B/C/D、P;Cwd=、E;cmd;nonce、未知子命令跳过。

## 验证 / Test Plan

**单测**：
- `\x1b]633;A\x07` → ShellMarkKind::PromptStart
- `\x1b]633;D;exit=0\x07` → OutputEnd exit_code=0
- `\x1b]633;P;Cwd=/tmp\x1b\\` → CwdChanged("/tmp")
- `\x1b]633;Z\x07`（未知）→ 忽略

**手测**：把 VSCode 的 633 注入脚本（zsh）放到 PureTerm 跑，prompt 标记正常显示。

**回归**：不影响现有 133。

## 优先级 / 范围

- **优先级**：P1（语义协议主线）
- **范围**：633 A/B/C/D/P;Cwd=/E 三类，映射到现有 ShellMark
- **不做**：不实现 `633;P;IsWindows`/`P;ContinuationPrompt` 等极少用属性
