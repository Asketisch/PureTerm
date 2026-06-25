# Shell Integration（Shell 集成）

## 概述

自动向 shell（bash/zsh/fish）注入脚本，实现：命令标记（marks）、命令跟踪、目录感知、主机名感知、用户名感知、点击-SCP、拖拽上传。AI agent 上下文的关键来源。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | Auto-inject for bash/zsh/fish/xonsh，marks at prompts，command tracking，directory/host/username awareness，click-to-SCP，drag-and-drop uploads | `sources/ShellIntegration/` (ShellIntegrationInjection.swift, BashShellIntegrationInjection, ZshShellIntegrationInjection, FishShellIntegrationInjection, XonshShellIntegrationInjection, iTermShellHistoryController, iTermDirectoryTree) |
| **Kaku** | 内置 Shell 套件：z (smart cd), zsh-completions, zsh-syntax-highlighting, zsh-autosuggestions | `Kaku-main/assets/shell-integration/` |

## PureTerm 现状

**未实现。** PureTerm 的 terminal 模块处理 VT100 序列，但没有主动向 shell 注入脚本。当前的 CWD 检测依赖 OSC 7/ pwd 转义，不是完整的 shell integration。

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
