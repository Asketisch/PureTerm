# Triggers（模式匹配自动化）

## 概述

定义规则匹配终端输出中的模式（正则表达式），自动执行动作：高亮文本、添加注释、发送通知、执行脚本、折叠区域等。67+ 种触发器类型。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | 67+ 种 trigger 类型：HighlightTrigger, AnnotateTrigger, MarkTrigger, NotificationTrigger, ScriptTrigger, iTermRPCTigger, iTermHyperlinkTrigger, BellTrigger, BounceTrigger, SendTextTrigger, BrowserTrigger, FoldTrigger, SGRTrigger, SetTabStatusTrigger, SetUserVariableTrigger 等 | `sources/Triggers/` |
| **cmux** | 无明确 Triggers 系统 | — |

## PureTerm 现状

**未实现。** PureTerm 的配置系统（`alacritty.toml`）支持基本的 keybindings 和 colors，但没有基于输出模式的自动化规则。

## 优先级

**P1 — AI 差异化能力**

Triggers 是 iTerm2 最强大的功能之一，将终端从"被动显示"升级为"主动响应"。对 AI agent 工作流尤其重要——可以自动检测 agent 输出中的特定模式并触发动作。

## 工作量

**L（1.5-2 周）**

需要：正则匹配引擎、动作执行器、配置 DSL、性能优化（避免每行都扫描）。

## 实现方向

1. 配置格式：`[[triggers]]` 段落在 `alacritty.toml`
2. 匹配器：`regex` crate，每行输出扫描
3. 动作类型：`highlight`（颜色高亮）、`annotate`（行注释）、`notify`（系统通知）、`exec`（执行命令）、`fold`（折叠区域）、`set_variable`（设置变量）
4. 性能：编译后 regex 缓存，仅在有 trigger 时扫描
5. UI：触发的 trigger 在状态栏显示计数
6. 可扩展：通过 MCP 暴露 trigger 事件给 AI agent
