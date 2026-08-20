# GAP-64 — OSC 8 超链接渲染 + 点击

## 背景 / 动机

OSC 8（`\x1b]8;params;URI\e\\text\x1b]8;;\e\\`）让终端把文本变成可点击超链接。Agent CLI 用它输出文件路径、URL、issue 号。iTerm2/Kitty/WezTerm/Windows Terminal/VSCode 均支持。

PureTerm 现状：**解析进 cell 已实现**（vte `ansi.rs:1393-1419` → `term/mod.rs:1907 set_hyperlink`，cell 存 `Hyperlink` `cell.rs:44-90`），但**渲染层完全不消费**：`grep hyperlink|Hyperlink` 在 `iced_app.rs` / `terminal_shader/*` / `output_blocks.rs` 命中 0。没有下划线、没有特殊色、没有点击 hit-testing。Agent 输出的链接目前与普通文字无异。

参考：[OSC 8 spec (egmontkob gist)](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda)。

## 目标

1. 含 hyperlink 的 cell 渲染时加下划线 + 可配区分色（默认蓝色）。
2. ⌘+Click（Cmd+Click）该 cell 时打开链接（系统 opener）。
3. 鼠标悬停显示链接（可选，视进度）。

## 现状证据

- cell 存储：`crates/alacritty_terminal/src/term/cell.rs:44-90,128,201-220`（`Hyperlink` 结构 + `cell.hyperlink()`）
- 解析：vte `ansi.rs:1393-1419`、`term/mod.rs:1907-1909`
- 渲染零消费：`grep -rn hyperlink|Hyperlink` 在 `crates/pureterm/src/iced_app.rs`、`terminal_shader/*.rs`、`output_blocks.rs` 全 0

## 变更点

### A. shader 渲染 hyperlink 装饰
`crates/pureterm/src/terminal_shader/`：
- `primitive_prepare.rs`：遍历 cell 构建 glyph 顶点时，若 `cell.hyperlink()` 非空，为该 cell 追加一条下划线图元（复用现有 underline 机制，颜色用 link 色）。
- 或在 `primitive.rs` 的图元列表里加 `Underline { row, col, color }`。
- link 色：从 config 取（默认 `#0066ff` 或主题 link 色）。

### B. hit-test + 点击
`iced_app.rs` 鼠标点击处理（搜 `MousePress`/`PointerButton::Pressed`）：
- 当 modifiers 含 `command`（⌘）且点击落在 terminal grid 区域：
  - 计算点击的 grid 坐标（已有 cell hit-test 逻辑，vi 模式有类似）
  - 取该 cell 的 `hyperlink()`
  - 非空则用 `open` crate / 平台 opener 打开 URL
- 非 ⌘+Click 不拦截，保持原有选中行为

### C. 悬停光标（可选）
鼠标移到含 link 的 cell 上时显示手形光标（视进度，本轮可不做）。

## 验证 / Test Plan

**手测**：
```sh
printf '\e]8;;https://example.com\e\\click here\e]8;;\e\\\n'
```
预期：「click here」带蓝色下划线；⌘+Click 打开浏览器。

**回归**：渲染单测较难（wgpu），靠手测 + 确认不破坏现有 glyph/underline 渲染。

## 优先级 / 范围

- **优先级**：P0（重要：Agent 输出路径/URL 不可点是明显缺陷）
- **范围**：下划线渲染 + ⌘+Click 打开
- **不做**：悬停 tooltip、link 去重/分组、相对路径转绝对路径
