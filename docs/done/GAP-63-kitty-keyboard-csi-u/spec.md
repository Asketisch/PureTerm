# GAP-63 — Kitty Keyboard Protocol CSI u 编码

## 背景 / 动机

Kitty keyboard protocol（disambiguate mode）是 Agent CLI 精确接收组合键的事实标准：
- **Claude Code** 依赖 `Shift+Enter` 换行（不触发命令提交）、`Esc`×2 中断、`Ctrl+R` 反向搜索
- **Cursor CLI / Codex CLI** 依赖 `Alt+字母` 快捷键、`Shift+方向` 选择
- 无 disambiguate 时按键歧义（如 `Ctrl+C` 被当 SIGINT 而非可编辑字符）

PureTerm 现状：模式协商（push/pop/report/set）已实现（`term/mod.rs:1300-1356`，受 `config.kitty_keyboard` gate），TermMode 已留好 5 个 flag（`mod.rs:78-88`）。**但按键编码走 legacy 路径**：`iced_app.rs:9674-9731` `key_to_bytes`/`named_key_to_escape` 硬编码传统转义序列（`\x1b[A`、`\r` 等），完全不读 `TermMode::KITTY_KEYBOARD_PROTOCOL`，也不构造 `CSI ... u`。config 默认 `false`。

参考：[Kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)。

## 目标

1. 当 terminal 进入 kitty keyboard 模式时，按键以 `CSI <key>;<mods>u` 格式发送。
2. `Shift+Enter` 在 kitty 模式下发 `\x1b[13;2u`（而非 `\r`），让 Agent 区分换行与提交。
3. config `kitty_keyboard` 默认开启（或提供 `auto`）。

## 现状证据

- TermMode flag：`mod.rs:78-88`（`DISAMBIGUATE_ESC_CODES = 1<<18` 等）
- 协商实现：`mod.rs:1300-1356`（`if !self.config.kitty_keyboard { return; }`）
- 编码缺失：`iced_app.rs:9674-9731` `key_to_bytes` 不读 mode
- Enter 硬编码：`iced_app.rs:9088-9090` 永远发 `\r`；Shift+Enter `iced_app.rs:9054` 走 ToggleZoom
- config 默认 false：`mod.rs:359,372`

## 变更点

### A. 新增 kitty CSI u 编码器
在 `iced_app.rs`（或新模块 `src/kitty_keyboard.rs`）新增：
```rust
/// Encode a key event as Kitty keyboard CSI u sequence.
/// Spec: CSI <key-code>;<modifier-bits>;1u  (text 不带时省略第三参数)
fn encode_kitty_u(key: &Key, named: Option<&Named>, mods: Modifiers) -> Option<Vec<u8>>
```
- key-code：Enter=13、Tab=9、Backspace=127、Esc=27、F1-F12=1-12（kitty 用功能键 code）、方向键 ↑=57346/↓=57347/←=57348/→=57349（或用命名键的 kitty code）、普通可打印字符 = Unicode codepoint
- modifier-bits：bit1=Shift、bit2=Alt、bit3=Ctrl、bit4=Super（值 = 1 + sum(bits)）
- 例：Shift+Enter = `\x1b[13;2u`（mods=1+(1<<1)=2）；Ctrl+C = `\x1b[99;5u`；Alt+Shift+X = `\x1b[88;4u`

### B. key_to_bytes 分支
`iced_app.rs:9674` `key_to_bytes`：开头加
```rust
let ctx = self.active_terminal_ctx()?;
if ctx.terminal().mode().contains(TermMode::KITTY_KEYBOARD_PROTOCOL) {
    if let Some(bytes) = encode_kitty_u(key, named, modifiers) {
        return bytes;
    }
}
// ... 现有 legacy 路径
```

### C. Enter 路径
`iced_app.rs:9051` 的 `Enter` 分支：在 kitty 模式 + Shift 时发 `\x1b[13;2u`（注意保留 `Cmd+Shift+Enter → ToggleZoom` 优先级在 9054）。普通 Enter 在 kitty 模式下仍可发 `\x1b[13;1u` 或保持 `\r`（多数 Agent 接受 `\r`，kitty 模式下严格发 CSI u 更安全——按 config 决定）。

### D. config 默认开启
`mod.rs:359,372` `kitty_keyboard` 默认改为 `true`。或加 `KittyKeyboard::Auto`（探测 alt screen 进入时启用）。本轮先改默认 true，让 Agent 开箱可用。

## 验证 / Test Plan

**单测**（`kitty_keyboard.rs` 内）：
- `encode_kitty_u(Enter, Shift)` → `b"\x1b[13;2u"`
- `encode_kitty_u('c', Ctrl)` → `b"\x1b[99;5u"`
- `encode_kitty_u(ArrowUp, none)` → 正确 code
- modifier-bits 计算覆盖 Shift/Alt/Ctrl/Super 组合

**手测**：
```sh
cat -v   # 按 Shift+Enter 应看到 ^[[13;2u
```
在 Claude Code 内验证 Shift+Enter 换行生效。

**回归**：`cargo nextest run -p pureterm`。

## 优先级 / 范围

- **优先级**：P0（Agent 关键，无此能力 Shift+Enter 等不工作）
- **范围**：disambiguate + CSI u 编码（5 flag 中只实质用 `DISAMBIGUATE_ESC_CODES`，其余 flag 已正确协商，本轮不实现 text reporting 子协议）
- **不做**：`REPORT_ASSOCIATED_TEXT`（带文本的 CSI u）、键盘协议的 pop 栈深度管理
