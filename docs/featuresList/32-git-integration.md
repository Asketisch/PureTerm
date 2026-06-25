# Git Integration（Git 集成）

## 概述

在 Tab/Pane/Status Bar 中显示 Git 详细信息：当前分支、dirty 状态、linked PR 状态/编号、最近 commit。支持快速操作（commit、push、PR 创建）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | 每个 workspace 显示分支、linked PR 状态/编号 | — |
| **iTerm2** | iTermGitState 状态栏组件 | `sources/StatusBar/Components/iTermGitState` |
| **Kaku** | Lazygit 集成（`Cmd+Shift+G`），未提交更改提示 | `Kaku-main/crates/kaku/src/lazygit.rs` |

## PureTerm 现状

**基础实现。** PureTerm 的 Status Bar 显示 Git branch（`crates/pureterm/src/status_bar.rs`），但信息有限。

## 优先级

**P2 — 终端增强**

Git 集成是开发者的核心需求，cmux 将其作为 workspace 上下文的关键部分。

## 工作量

**M（1 周）**

需要：`git2` crate 增强集成、PR 状态查询（GitHub API）。

## 实现方向

1. `git2` crate：branch + dirty (+M, +U, ?) + current commit
2. GitHub API：查询 linked PR（通过 branch name 匹配）
3. Status Bar 增强：`git: main ●2 ~1`（分支 + staged + modified）
4. Tab 标题：可选显示分支名
5. 快捷键：`Cmd+Shift+G` 打开 Lazygit（如果安装）
6. PR 状态：Status Bar 显示 PR number + checks status
