# Shell Integration（Shell 集成）

## 概述

自动向 shell（bash/zsh/fish）注入脚本，实现：命令标记（marks）、命令跟踪、目录感知、主机名感知、用户名感知、点击-SCP、拖拽上传。AI agent 上下文的关键来源。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | Auto-inject for bash/zsh/fish/xonsh，marks at prompts，command tracking，directory/host/username awareness，click-to-SCP，drag-and-drop uploads | `sources/ShellIntegration/` (ShellIntegrationInjection.swift, BashShellIntegrationInjection, ZshShellIntegrationInjection, FishShellIntegrationInjection, XonshShellIntegrationInjection, iTermShellHistoryController, iTermDirectoryTree) |
| **Kaku** | 内置 Shell 套件：z (smart cd), zsh-completions, zsh-syntax-highlighting, zsh-autosuggestions | `Kaku-main/assets/shell-integration/` |

## PureTerm 现状

**v1 已实现（AI 上下文核心 MVP，🚧 代码闭环 / 待人工 GUI E2E）。** zsh + bash 注入 OSC 133 A/B/C/D 标记 + 复用既有 OSC 7 cwd；OSC 133 解析在 vendored `alacritty_terminal` 里早已存在（`scan_osc133`），唯一缺口是事件桥的一个 match 分支，已补齐。

v1 交付：
- `crates/pureterm-daemon/src/shell_integration.rs`：`prepare_shell_env(tag, opts)` 在 `prepare_osc7_env` 基础上覆写 zsh `.zshenv`（precmd/preexec/chpwd）与 bash `PROMPT_COMMAND`（DEBUG trap + 武装哨兵），发 BEL 终结的 OSC 133；`PURETERM_SI_PID` 哨兵防同进程重复注入。
- `TerminalEvent::ShellMark` 桥接（vendored `ShellMark` 补 `PartialEq,Eq` derive + `send_event` arm）。
- `crates/pureterm/src/shell_state.rs`：per-pane A/B/C/D 状态机，D 时从 grid 取命令文本（`bounds_to_string`），记录 exit/duration，裁到 64 条。
- `Category::Shell` + `shell.{prompt_started,command_started,output_started,command_finished}` 事件（JSONL 持久化）。
- `crates/pureterm/src/shell_context.rs` + IPC `shell.context` + MCP `get_shell_context` 工具：返回 cwd / last_command / recent_commands；SSH/fish pane 优雅降级 `integrated:false`。
- `[shell_integration]` 配置（`enabled`/`inject_zsh`/`inject_bash`/`include_command_text`）。

配置门控仅作用于 GUI 直接 PTY 回退路径；daemon 路径（无 config 访问）始终全集成（与 OSC 7 既有 always-on 一致）—— 已知局限，后续 gap 可经 RPC 透传 opts。

**待人工 GUI E2E**（见下「验证 runbook」）。**后续 gap（本 gap 不做）：** 视觉 grid marks / scroll-to-mark（新 `Flags::MARK` + shader）、fish/nu、OSC 1337 host/user、click-to-SCP + 拖拽上传、Settings UI 页。

## 验证 runbook（人工 GUI E2E）

1. `./build.sh app`（已重打包 `dist/PureTerm.app`，含 puretermd）→ `open dist/PureTerm.app`。
2. zsh pane：`cd /tmp; false` 然后 `echo ok`。
3. 另一终端 `pureterm events tail --type shell`：按序见 `shell.command_started`(B) → `shell.command_finished`(D, `exit_code:1`, `duration_us` 非空, `command:"false"`) → 下一对 `exit_code:0`/`command:"echo ok"`。
4. `pureterm events list --type shell --limit 5`（JSONL 持久化）。
5. MCP 客户端（Claude Code `pureterm mcp install`）调 `get_shell_context`：`last_command.text=="echo ok"`、`exit_code==0`、`cwd=="/tmp"`、`integrated==true`。
6. bash pane 重复 2-5。
7. SSH pane 调 `get_shell_context` → `integrated:false, last_command:null`。
8. 集成 zsh 内 `exec zsh` → `events tail` 无重复发（哨兵生效）。
9. 回归：`cd` 后状态栏 cwd 仍更新（OSC 7 未破坏）。

单测已绿：pureterm 595、pureterm-daemon 73（排除 3 个真机 PTY 集成测试 + 2 个既存 discovery 失败，与本 gap 无关）。

## 优先级

**P0 — AI 核心管道**

Shell Integration 是 AI agent 感知用户操作的基础——知道用户在哪个目录、执行了什么命令、命令是否成功。没有它，AI Chat Panel（01）和 Error Recovery（02）的上下文注入效果大打折扣。

## 工作量

**L（1.5-2 周）**

需要：shell 脚本注入机制、prompt 标记协议、命令跟踪、多 shell 支持。

## 实现方向

1. 注入机制：通过 `~/.config/pureterm/shell-integration/` 下的脚本
2. Shell 检测：`$SHELL` 环境变量
3. Bash：`PROMPT_COMMAND` 钩子 + `DEBUG` trap
4. Zsh：`precmd` / `preexec` 钩子
5. Fish：`fish_prompt` / `fish_preexec` 事件
6. 标记协议：OSC 1337（类 iTerm2）或自定义 escape sequence
7. 暴露给 AI：MCP tool `get_shell_context()` 返回当前目录、最近命令、环境变量
