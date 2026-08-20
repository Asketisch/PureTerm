# GAP-69 — 多行粘贴注入防护

## 背景 / 动机

粘贴含换行/控制字符的多行文本到终端时，可能被注入命令（如粘贴网页里藏了 `\nrm -rf\n`）。这是 Agent 场景的高危通道：用户从 Agent 输出复制、或从不可信来源粘贴。Microsoft Terminal #13014 指出多行粘贴警告**不可被 escape 关闭**（否则攻击者用 escape 绕过）。

PureTerm 现状：`iced_app.rs:6872-6887 ClipboardPaste` 直接发 PTY，仅做 `text.replace(['\x1b','\x03'], "")`（剥 ESC/Ctrl+C），**无任何确认对话框**。

参考：[MS Terminal #13014](https://github.com/microsoft/terminal/issues/13014)、[Trail of Bits prompt injection→RCE](https://blog.trailofbits.com/2025/10/22/prompt-injection-to-rce-in-ai-agents/)。

## 目标

1. 粘贴多行（`\n` 数 > 阈值）或含危险控制字符 → 弹 `PasteConfirm` overlay。
2. overlay **不可被 ESC 关闭**，只能显式「粘贴」/「取消」。
3. 可配置阈值（默认多行即弹）。

## 现状证据

- 直发无确认：`iced_app.rs:6872-6887`
- 全仓库无 `paste_confirm`/`paste_warning` 命中

## 变更点

### A. 检测
`iced_app.rs` `ClipboardPaste` 处理：计算危险度
```rust
let newline_count = text.matches('\n').count();
let has_ctrl = text.chars().any(|c| c.is_control() && !matches!(c, '\t'|'\r'|'\n'));
let needs_confirm = newline_count > threshold || has_ctrl;
```

### B. PasteConfirm overlay（粘贴确认浮层）
- 新增 `paste_confirm: Option<PasteConfirmState { text, stats }>` 字段
- overlay UI：显示前几行预览 + 行数 + 「粘贴」「取消」按钮
- ESC 键不关闭（在 key handler 显式不处理 ESC for paste_confirm，或 ESC 等同「取消」——按 MS 建议选「ESC 取消」更安全，但绝不让 ESC = 粘贴）

### C. 配置
`config` 加 `paste_confirm_threshold: usize`（默认 1，即 ≥2 行触发）。

## 验证 / Test Plan

**手测**：
- 粘贴单行 → 不弹，直发。
- 粘贴 3 行 → 弹确认，显示行数。
- 弹窗时按 ESC → 取消（不发），绝不粘贴。

**回归**：单行粘贴行为不变。

## 优先级 / 范围

- **优先级**：P3（安全闸，本轮实现）
- **范围**：多行 + 控制字符检测 + 不可绕过确认 overlay
- **不做**：粘贴内容预览的语法高亮、按来源白名单
