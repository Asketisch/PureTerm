---
gap: GAP-10
title: Triggers
category: 终端交互
plan_type: feature
priority: P1
effort: L
status: done
closed: 2026-06-28
plan: ../../superpowers/plans/2026-05-15-triggers-system.md
---

# Triggers（模式匹配自动化）

## 概述

定义规则匹配终端输出中的模式（正则表达式），自动执行动作。v1 交付四动作；v2（2026-06-28）补齐捕获组模板、`set_variable`、`set_mark` + `fold`、状态栏 `trigger_count`。

## 已交付

- `[[triggers]]` TOML 配置 + `load_triggers()` 热重载
- Wakeup per-pane 增量扫描 + `trigger.matched` EventBus（含 `captures` 数组）
- GPU highlight overlay + fold 区间 conceal 渲染
- toast / PTY send_text / exec 子进程（argv，无 shell）
- 模板展开：`{match}`、`$1`–`$9`、`${var:NAME}`、`\n\r\t\xNN`
- `set_variable` → per-pane 变量；`trigger.variable_set` 事件（不含值）
- `set_mark` + `fold`（命名 mark 到命中行；`lines_back` 回退）
- MCP `trigger_list` + IPC `trigger.list`（脱敏）
- 状态栏组件 id `trigger_count`（显示 `⚡ N`，需手动加入 `status_bar.components`）

## 配置示例

```toml
[[triggers]]
name = "error-spot"
pattern = "(?i)error:|panic:"
action = "highlight"
# color = "#55ff55"   # 可选 RGBA hex

[[triggers]]
name = "build-done"
pattern = "Built in \\d+s"
action = "notify"
title = "Build complete"

[[triggers]]
name = "open-link"
pattern = "(https://\\S+)"
action = "set_variable"
variable = "last_url"
value = "$1"

[[triggers]]
name = "mark-build"
pattern = "^Building"
action = "set_mark"
mark = "build_start"

[[triggers]]
name = "fold-build"
pattern = "SUCCESS"
action = "fold"
mark = "build_start"
# lines_back = 5   # 无 mark 时折叠 [line-lines_back, line]

[[triggers]]
name = "reply-sudo"
pattern = "Password:"
action = "send_text"
text = "mysecret\n"   # 支持 $1 / {var:NAME} 模板

[[triggers]]
name = "open-url"
pattern = "https://example\\.com/\\S+"
action = "exec"
command = "open"
args = ["$1"]
route = "toast"   # 或 "pane"
```

### 状态栏 trigger_count

```toml
[status_bar]
components = ["trigger_count", "cwd", "git"]
```

## Fold 语义（v2）

- `set_mark`：命中行记入 `named_marks[mark]`（同名覆盖）
- `fold`：`top_line` = mark 行（若已滚出 scrollback 则回退 `lines_back` 或单行）；`bottom_line` = 命中行
- 渲染：折叠区间内行 conceal（保留行高）；v1 不做点击展开

## 安全

- `trigger.list` / MCP 不返回 `send_text.text`、`set_variable.value`、`exec.args`
- `trigger.variable_set` 只含变量名
