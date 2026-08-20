# GAP-71 — MCP capture_pane ANSI 剥离模式

## 背景 / 动机

Agent 通过 MCP `capture_pane` 读终端内容时，若内容含 ANSI 转义（来自 `cat` 文件/抓网页），可能构成 **prompt injection → RCE**（Trail of Bits）。提供纯文本模式让 Agent 安全读取。

参考：[Trail of Bits](https://blog.trailofbits.com/2025/10/22/prompt-injection-to-rce-in-ai-agents/)、[Terminal DiLLMa](https://embracethered.com/blog/posts/2024/terminal-dillmas-prompt-injection-ansi-sequences/)。

## 目标

`mcp/server.rs` `capture_pane` 加 `strip_ansi: bool` 参数（默认 true）。

## 变更点

1. `crates/pureterm/src/mcp/server.rs` capture_pane 工具 schema 加 `strip_ansi`。
2. 实现用 `vte` 或手写 ANSI 剥离器（去掉 CSI/OSC 控制序列）。
3. 默认 true（安全默认）。

## 优先级 / 范围

- **优先级**：P3（视进度）
- **范围**：capture_pane 加参数 + ANSI 剥离

## 验证

单测：含 `\x1b[31mred\x1b[0m` 的 pane → `red`。
