# AI Agent 特性 —— 手动 display-smoke runbook

> CI 自动 E2E harness(`crates/pureterm/src/agent_e2e.rs` + 各模块 `#[cfg(test)]`)
> 已覆盖 GAP-7.3–7.7 的**行为逻辑**(deferred-reply 往返、SIGSTOP/SIGCONT、冷恢复
> argv、进程扫描、HMAC token、审计落盘)。CI 无法证明**像素渲染**(右侧 pinned
> overlay 面板可见性、❄/⚡/⏳ 字形、点按钮→resolve)。本页是那条人工置信层 ——
> 每次相关代码改动后建议跑一次,勾选签字。
>
> **非 CI gate**。每个 flip ✅ 时：`todo.md` 删行 → `done.md` 加行；CI 走自动 harness，手动走本文件。

## 前置

```bash
./build.sh app          # 重打包 dist/PureTerm.app(改了 pureterm/puretermd 后必须)
open dist/PureTerm.app
```

配置:`~/.config/pureterm/pureterm.toml`(主路径;兼容回退 `~/.config/alacritty/`)。

---

## GAP-7.4 Feed 审批面板

```toml
[[agent.hook]]
event = "PreToolUse"
ask_pattern = "."          # 任意 Bash 都弹审批
```

```bash
pureterm agents install claude-code   # 写 ~/.claude/settings.json
```

在 PureTerm pane 里跑 `claude`,触发一个 Bash 工具调用:

- [ ] 右侧弹出 5 按钮面板(Once/Always/All/Bypass/Deny)+ 120s 倒计时 + 主体(command)展示
- [ ] 点 **Once** → 命令执行,面板消失
- [ ] 下一条 Bash → 面板再弹(Once 不记忆)
- [ ] 点 **Always** → 此会话此工具后续自动放行(不再弹),`source:memory`
- [ ] 点 **Deny** → Claude Code 阻断该工具
- [ ] 不点 → 120s 后自动 Deny(`source:timeout`)
- [ ] `tail ~/Library/Application\ Support/pureterm/feed.jsonl` 见审计行(choice/source)

## GAP-7.7 自定义恢复绑定

```bash
pureterm surface resume set --kind tmux --checkpoint work --auto --shell tmux attach -t work
pureterm surface resume run          # 经 Feed 面板审批,Always → mint HMAC token
```

- [ ] `resume run` 弹 Feed 面板;点 **Always** → 持久化 token(`<data_dir>/resume-tokens.json`)
- [ ] 杀掉该 tmux 会话后冷重启 GUI → 同 kind 的死 session 静默自动 spawn `tmux attach -t work`(令牌命中,不弹面板)
- [ ] 改 shell 再冷启 → 令牌失效 → 回落 `claude --resume`(令牌绑定精确 shell)

## GAP-7.5 智能体会话恢复

```toml
[[vault.agents]]
kind = "claude-code"
resume_command = ["claude", "--resume", "{session_id}"]
```

```bash
pureterm agents install claude-code   # SessionStart 默认已注册([[vault.agents]] 非空时 derive_entries 自动加)
```

- [ ] pane 里跑 `claude`(SessionStart hook 触发)→ `<data_dir>/agents/claude-code-sessions.json` 有记录
- [ ] **⌘Q** 退出 app(不 ⌘W —— 关 tab 会 kill session);重开 GUI → 死 session 自动 `claude --resume <id>` 回来

## GAP-7.6 Vault 智能体注册表 / detect-resume

在 PureTerm **外**(普通终端)跑一个 `claude`(产生可 resume 的 session),然后在 GUI:

- [ ] detect-resume 面板(启动同步扫一次 + 每 30s)列出检测到的 agent 进程 + 最近 session_id
- [ ] 点 **Resume** → 新 pane 里 `claude --resume <id>`
- [ ] 状态栏显示 `⚡ claude-code`(active);hibernate 后 `❄`;Feed pending 时 `⏳ N`
- [ ] `pureterm agents detect` / `pureterm agents registry`(CLI 等价)

## GAP-7.3 智能体休眠

```toml
[agent.hibernation]
enabled = true
pty_idle_secs = 300
cpu_idle_secs = 300
max_live_terminals = 2
confirm_secs = 60
```

开 ≥3 个 agent pane,留 ≥1 个 idle 5 分钟(PTY 无输出 且 CPU 近零):

- [ ] hibernate 确认面板弹出(60s 倒计时)+ 状态栏 `❄`
- [ ] 点 **确认** → SIGSTOP(child 不死,PTY 仍开);`pureterm agent list` 见 hibernated
- [ ] resume(面板 / `pureterm agent resume <id>` / 重启)→ SIGCONT,状态栏回 `⚡`
- [ ] `never_hibernate = true` 的 kind 不入选候选、不计入 live 计数

---

## 已知环境注意

- **clippy**:`cargo clippy --workspace -- -D warnings` 当前被 vendored `alacritty_terminal`
  的 `#![cfg_attr(clippy, deny(warnings))]` + 新 clippy lint 阻塞(预先存在,非本工作引入;
  CLAUDE.md 禁止无关格式化 vendored 源码)。本工作新增代码无 rustc 警告。
- **fmt**:repo 的 rustfmt 配置用 nightly-only 选项(`imports_granularity`/`group_imports`),
  stable rustfmt 会报整库 diff(预先存在)。
- **daemon `instance_lock_is_exclusive` 测试**:本机若有 `puretermd` 在跑会失败(环境性,
  非代码问题)。
