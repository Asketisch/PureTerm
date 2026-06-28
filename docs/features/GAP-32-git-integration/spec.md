---
gap: GAP-32
title: Git Integration
category: 开发者工具
plan_type: integration
priority: P2
effort: M
status: done
closed: 2026-06-27
e2e: manual-e2e/gap-32-git-integration.md
---

# Git Integration（Git 集成）

## 概述

在 Status Bar / Tab 标题 / Git 面板中显示 Git 详细信息：当前分支、dirty 分计数（staged/unstaged/untracked/unmerged）、ahead/behind、HEAD oid、linked PR（编号 + checks 滚动汇总）、in-progress 操作（rebase/merge/…）、最近 commit。支持快捷操作（add / commit / push / pull / create PR / lazygit），并把 git 状态变化写进事件流（`git.*`）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | 每个 workspace 显示分支、linked PR 状态/编号 | — |
| **iTerm2** | iTermGitState 状态栏组件 | `sources/StatusBar/Components/iTermGitState` |
| **Kaku** | Lazygit 集成（`Cmd+Shift+G`），未提交更改提示 | `Kaku-main/crates/kaku/src/lazygit.rs` |

## PureTerm 现状

**✅ 已闭环（2026-06-27）。** 实现走 `git status --porcelain=v2` shell-out（一条子进程出 branch + 分计数 + ahead/behind + HEAD oid）+ `gh pr view` shell-out（复用用户 `gh auth`，零新依赖、零 OAuth）；状态栏 git 段 + ⌘⇧B 只读面板（分计数 / PR+checks / 最近 commit / 快捷操作）+ Create-PR（`gh pr create`）+ 可选 tab 标题显示分支 + `git.*` EventBus 发射。574 pureterm 单测绿。

## 优先级

**P2 — 终端增强**

Git 集成是开发者的核心需求，cmux 将其作为 workspace 上下文的关键部分。

## 工作量

**M（1 周）**

需要：`git status --porcelain=v2 --branch` 解析（纯函数）+ `gh` CLI PR 查询 + 状态栏段 / 面板 / 快捷键 / 配置。

## 实现方向

1. **porcelain-v2 分计数**：`parse_porcelain_v2` 解析 branch / dirty / staged / unstaged / untracked / unmerged / renamed / upstream / ahead-behind / HEAD 短 oid；`git` 缺失或非 repo 回退纯 FS `read_git_branch`（行为不回退）。`crates/pureterm/src/status_bar/git.rs`
2. **`gh pr view` PR 查询 + Create-PR 动作**：shell-out `gh pr view --json number,state,headRefName,url,statusCheckRollup`（复用用户 `gh auth`）；PR 挂 `GitStatus.pr` 单一源；Create-PR 面板按钮写 `gh pr create` 进 PTY。`crates/pureterm/src/gh_pr.rs`
3. **状态栏 git 段**：`main ↑2↓1 ●2 ~1 +3`（分支 + ahead/behind + staged + modified + untracked；零计数段省略）+ PR pill（`⧉#N STATE`，按 checks 着色）+ in-progress op 后缀。`crates/pureterm/src/status_bar/components.rs`
4. **Tab 标题可选显示分支**：`[git].tab_title_branch` 开后，在**派生标题**回退路径追加 ` [<branch>]`（OSC 程序设的标题仍最高优先，不被覆盖）；默认关（呼应「可选」）。`iced_app.rs::tab_title_with_branch`
5. **⌘⇧B 唤面板 + 快捷操作**：避让 ⌘⇧G（已绑 SearchPrev）；面板按钮 Stage all / Commit / Push / Pull / Create PR / Lazygit，命令写进活跃 pane 的 PTY；lazygit 在 SSH pane 或未装时降级 toast。`keymap.rs` + `iced_app.rs::run_git_quick_action`
6. **EventBus `git.*` 发射**：新增 `Category::Git`，在**有意义转移点**（非每 tick）发射 `git.branch_changed`（按 pane 去重）/ `git.pr_updated` / `git.panel_opened` / `git.panel_closed` / `git.quick_action`，`pureterm events tail --type git` 可消费。`events.rs` + `iced_app.rs`

> pixel smoke：[`../../manual-e2e/gap-32-git-integration.md`](../../manual-e2e/gap-32-git-integration.md)（非 CI gate）。
