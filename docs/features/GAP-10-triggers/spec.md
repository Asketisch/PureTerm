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
- **远程输出 RCE 防护（review 2026-06-30 新增）**：当匹配来自 SSH/远程 pane 时，`exec` 与**跨 pane** `send_text`（`pane_id=Some`）默认**阻断**——远端控制的文本（经 `$1` 捕获组）不可驱动本地命令参数 / 本地面键盘注入。发 `trigger.blocked` 事件（`reason: remote_output_exec_blocked`），匹配本身（`trigger.matched` + 计数）仍记。
  - 顶层 opt-in 开启：`allow_exec_on_remote = true`（顶层 key，**不放 `[triggers]` 下**——`[[triggers]]` 使 `triggers` 为数组，标量不能共享该键）。
  - **同 pane `send_text`**（`pane_id=None`）永不阻断：这是 `reply-sudo` 主用例（向自己远程会话回密码，文本为用户自配）。
  - `highlight`/`notify`/`set_variable`/`set_mark`/`fold` 对本机无副作用，永不阻断。
- `exec` 子进程有 60s 硬超时（`EXEC_TIMEOUT_SECS`），超时 SIGKILL + toast，防挂死命令泄漏进程。

## 已知局限

- **per-line 匹配上限跨所有 trigger 共享**：`scan_lines` 的 `MAX_MATCHES_PER_LINE=8` 在一行内对所有 trigger 共享计数，首个 trigger 在某行命中 8 次会饿死该行后续 trigger。有意约束（bound pathological regexes）；若需每 trigger 独立上限待后续调整。
- 首 wake 只 seed cursor、不扫 pane 创建时已存在的 scrollback（只扫「新输出」）。
- 仅扫活跃窗口·活跃 workspace 的 pane；后台 pane 的 trigger 延迟到切回处理。
- `trigger_count` 状态栏组件需手动加入 `status_bar.components`。
