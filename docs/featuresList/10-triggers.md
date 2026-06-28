# GAP-10 Triggers

**状态**: ✅ v1 + v2 已交付（2026-06-28）

正则匹配终端输出，命中后自动执行动作。适合 AI agent 工作流（检测输出模式并响应）。

## 动作

| action | 说明 |
|--------|------|
| `highlight` | 匹配 span 半透明 overlay（默认绿色，可配 `color`） |
| `notify` | Toast + EventBus `trigger.matched` |
| `send_text` | 写 PTY（默认命中 pane；`pane_id` 可指定跨 pane） |
| `exec` | `Command::new` + args（无 shell）；`route = toast \| pane` |
| `set_variable` | 展开 `value` 写入 per-pane 变量（供 `${var:NAME}`） |
| `set_mark` | 命中行记为命名 mark |
| `fold` | 从 mark（或 `lines_back`）到命中行 conceal 折叠 |

## 模板

`notify.title`、`send_text.text`、`exec` 参数、`set_variable.value` 支持：

- `{match}` / `$0` / `\0` — 全文
- `$1`–`$9` / `{1}`–`{9}` — 捕获组
- `${var:NAME}` / `{var:NAME}` — pane 变量
- `\n` `\r` `\t` `\xNN`

## 配置

在 `alacritty.toml` 中添加 `[[triggers]]` 段，见 [spec](../features/GAP-10-triggers/spec.md)。

## 状态栏

组件 id `trigger_count`：显示 `⚡ N`（全局命中累计，N=0 时隐藏）。**默认配置不包含**，需手动加入 `status_bar.components`。

## MCP

- `trigger_list` → 规则列表（name / pattern / action），**不含** send_text 的 `text`、set_variable 的 `value`

## 事件

```bash
pureterm events tail --type trigger
```

- `trigger.matched` — 每条命中（含 `captures`）
- `trigger.variable_set` — 变量名（不含值）
