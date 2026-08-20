# GAP-70 — OSC 52 按次确认（可选增强）

## 背景 / 动机

GAP-65 核查确认 OSC 52 已有白名单策略（`Osc52::OnlyCopy` 默认，`term/mod.rs:378-394,1736-1761`）——拒绝远程程序读剪贴板，允许写。但"写剪贴板"仍可能被恶意程序用来窃取数据外传（写进剪贴板后另一进程读）。可选增强：增加「每次写剪贴板弹确认」档位。

## 目标

新增 `Osc52::OnlyCopyWithConfirm`：OSC 52 写剪贴板前弹用户确认。

## 变更点

1. `term/mod.rs:378` enum 加 `OnlyCopyWithConfirm`。
2. `clipboard_store`（`mod.rs:1736`）：该档位时发 `Event::ClipboardStoreConfirm` → GUI 弹确认。
3. 设置 UI 加选项（`pureterm-settings/src/i18n/mod.rs:233`）。

## 优先级 / 范围

- **优先级**：P3（视进度）
- **不做**：按进程白名单

## 验证

手测：OSC 52 写剪贴板弹确认。
