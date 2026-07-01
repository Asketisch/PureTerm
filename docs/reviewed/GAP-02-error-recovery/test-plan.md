# GAP-02 Error Recovery — 手动 GUI E2E runbook

> 🚧 **待人工通过** — GAP-02 代码闭环，本页是 flip 🚧→✅ 的人工置信层。**非 CI gate。**
>
> **Automated harness:** `cargo test -p pureterm error_recovery` + `app_config::tests::error_recovery_*` + `events::tests::type_filter_error_recovery_*` 绿。

## 前置

```bash
export ANTHROPIC_API_KEY=sk-...   # 或 OPENAI_API_KEY + provider = "openai"
./build.sh app
open dist/PureTerm.app
```

确认 `~/.config/pureterm/alacritty.toml` 含：

```toml
[shell_integration]
enabled = true
include_command_text = true

[error_recovery]
enabled = true
auto_analyze = true
provider = "anthropic"
```

## 场景 1 —— 自动分析失败命令

1. 在 zsh/bash pane 执行：`ls /this-path-does-not-exist-xyz`
2. 预期：底部弹出 **Error Recovery** 卡片，显示 exit code + 流式 LLM 建议。
3. 建议完成后出现 **Suggested: `...`** 与 **Apply** 按钮。

## 场景 2 —— Apply 写入 PTY

1. 在场景 1 卡片上点击 **Apply**。
2. 预期：建议命令写入 pane（带换行执行），卡片关闭。

## 场景 3 —— ⌘⇧E 手动触发

1. 执行 `false`（或任意非零 exit 命令）。
2. 关闭卡片后按 `⌘⇧E`。
3. 预期：对最近一次失败命令重新分析并打开卡片。

## 场景 4 —— EventBus

1. 终端：`pureterm events tail --type error_recovery`
2. 触发场景 1。
3. 预期：见 `error_recovery.triggered`，完成后见 `error_recovery.suggested`；Apply 后见 `error_recovery.applied`。

## 通过判据（flip 🚧→✅）

- [ ] 场景 1：非零 exit → 自动卡片 + 流式建议
- [ ] 场景 2：Apply 写入 PTY
- [ ] 场景 3：⌘⇧E 手动重分析
- [ ] 场景 4：`events tail --type error_recovery`

全过 → 更新 [`FEATURES.md`](../../FEATURES.md) / [`features/GAP-02-error-recovery/spec.md`](../features/GAP-02-error-recovery/spec.md) / [`todo.md`](../todo.md)。

## 已知边界（非 DoD）

- SSH/fish pane（无 OSC 133）不自动分析；⌘⇧E toast 提示需 Shell Integration
- 无 API key 时卡片显示 `[error: missing API key …]`
- Settings UI 未做（仅 TOML + env）
