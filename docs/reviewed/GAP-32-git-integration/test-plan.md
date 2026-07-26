# GAP-32 Git Integration —— 手动 GUI E2E runbook

> ✅ **已 flip（2026-06-27，授权先行）** — GAP-32 已翻 🚧→✅（代码闭环 + 574 pureterm 单测绿 + runbook + 四处同步）。下方 6 个 GUI 场景作为**非阻塞 follow-up**，跑完勾选即可；**非 CI gate**。

> 🚧 **待人工通过** — GAP-32 代码闭环 + harness 全绿，本页是 flip 🚧→✅ 的人工置信层。**非 CI gate。**

> CI 自动 harness（`crates/pureterm/src/{status_bar/git.rs,gh_pr.rs,git_panel.rs,events.rs,iced_app.rs}` 单测，**574 pureterm 绿**）已覆盖**逻辑不变量**:
> - `parse_porcelain_v2` 把 porcelain v2 拆成 staged/unstaged/untracked/unmerged/renamed + upstream + ahead-behind + HEAD oid（clean/dirty/detached/no-upstream/non-git 全覆盖）;
> - `read_git_branch` 纯 FS 解析 `ref: refs/heads/<n>` / detached 短 SHA / 无效输入;
> - `git_quick_action_command` 纯函数：6 个动作 → 写入 PTY 的命令串（含 `gh pr create`）+ lazygit 配置覆盖;
> - `tab_title_with_branch` 纯函数：OSC 优先 / 派生路径追加 `[branch]` / 关闭或无分支原样;
> - `gh_pr::parse_gh_pr_json`：open PR + 混合 checks / 无 checks / 垃圾输入 → None;
> - EventBus：`Category::Git` 序列化为 `"git"` / `--type git` 前缀过滤 / `git_branch_changed` 去重（首次=变 / 相同=抑制 / 不同=变）。
>
> CI 无法证明**真实视觉**：状态栏 git 段随 dirty 实时变化、⌘⇧B 面板分计数/PR/按钮、Create-PR 真写进 PTY、lazygit 本地起 TUI、tab 标题带分支、`events tail --type git` 实时流。本页覆盖这些。
>
> **Automated harness re-run (2026-06-27):** `cargo test -p pureterm` **589** green（含 `status_bar/git` / `gh_pr` / `git_panel` / `Category::Git` 事件过滤）。`scan_osc7` ST 修复保障 OSC 7 cwd → git 段刷新。GUI 六场景仍待 `./build.sh app` 后人工勾选。

## 前置

```bash
./build.sh app          # 重打包 dist/PureTerm.app（改了 pureterm 后必须；cargo build 不够）
open dist/PureTerm.app
```

> 用户跑的是 `dist/PureTerm.app`（见 memory `app-runs-from-dist-bundle`）。E2E 时另开一个终端跑 `pureterm events ...`（指向同一份 `~/Library/Application Support/pureterm/events.jsonl`）。准备一个 git 仓库（任意本地 repo，有未提交改动更佳）并 `cd` 进去。

## 场景 1 —— 状态栏 git 段随 dirty/分支变化（核心 DoD）

1. 启动 `dist/PureTerm.app`，`cd` 到一个 git 仓库。
2. 观察底部状态栏 git 段，形如 ` main ↑0↓0`（clean）。
3. 在该 pane 跑 `touch new.txt`（或 `echo x >> README.md`），等下一次状态栏 tick（~1s）。

**预期**：状态栏 git 段在 ~1s 内变成 ` main ●0 ~0 +1`（+1 untracked）或带 modified 标记；切到别的分支（`git checkout -`）后分支名跟随更新；切到非 git 目录时段消失。

## 场景 2 —— ⌘⇧B 面板：分计数 / PR+checks / recent commits / Create PR 按钮

1. 在 git 仓库 pane 按 `⌘⇧B`（或 `⌘B`-uppercase）唤出 Git 面板（右上 overlay）。
2. 核对面板内容：分支 / upstream + ahead-behind / HEAD 短 SHA / `●staged ~modified +untracked !unmerged` / in-progress op（若有）/ Recent commits / PR 块。

**预期**：分计数与 `git status` 一致；若 `gh` 已装且当前分支有 linked PR，PR 行显示 `⧉#N STATE (✓s · …p · ✗f)` 并按 checks 着色（绿/amber/红）；按钮行含 Stage all / Commit / Push / Pull / **Create PR** / Lazygit（Create PR 仅在 `[git].show_pr = true` 时出现）。

## 场景 3 —— Create-PR / Commit / Push / Pull 写进 PTY

1. 面板打开，点 **Commit**。

**预期**：活跃 pane 的 PTY 收到 `git commit\n`，在 pane 内打开编辑器写 commit message（与 `git commit` 一致）。
2. 点 **Create PR**（需 `gh` 已装 + 已 `gh auth login`）。

**预期**：PTY 收到 `gh pr create\n`，在 pane 内走 gh 的 PR 创建流程。若 `gh` 未装 → 底部 toast `gh not installed — try \`brew install gh\`.`，不写 PTY。

## 场景 4 —— lazygit：本地起 TUI；SSH pane / 未装 → toast

1. 本地 git 仓库 pane（装了 lazygit），面板点 **Lazygit**。

**预期**：面板关闭，lazygit TUI 在该 pane 内启动。
2. 切到一个 SSH pane（`⌘⇧S` 连一个 profile），打开面板点 Lazygit。

**预期**：toast `lazygit isn't supported on SSH panes yet.`，不写 PTY。
3. （可选）未装 lazygit 时点 Lazygit → toast `<bin> not installed — try \`brew install lazygit\`.`。

## 场景 5 —— `[git].tab_title_branch` 开后 tab 标题带 `[branch]`

1. 默认配置下，tab 标题为 shell 的 OSC 标题或 `#N`（**不含**分支）。
2. 在 `~/.config/pureterm/alacritty.toml` 加：
   ```toml
   [git]
   tab_title_branch = true
   ```
   存盘（配置热重载）。
3. 在一个**没有 OSC 标题**的 pane（如新开的、shell 未设标题）切到 git 仓库。

**预期**：tab 标题变成 `#N [main]`（派生标题 + 分支后缀）；若 shell 已用 OSC 0/2 设过标题（如 vim 里），OSC 标题**仍优先**，不被 `[branch]` 覆盖（验证 GAP-1.3 优先级不被破坏）。

## 场景 6 —— `pureterm events tail --type git` 见 git 事件（验证 EventBus 集成）

1. 另开终端：`pureterm events tail --type git`。
2. GUI：开 Git 面板（`⌘⇧B`）→ 点 Commit → 切分支（`git checkout -` 在某 pane）→ 关面板。

**预期**：第二终端实时流出 `git.*` 事件，每行形如：
```
<n> git panel_opened {"cwd":"/path/to/repo"}
<n+1> git quick_action {"action":"commit"}
<n+2> git branch_changed {"branch":"feature/x","ahead":null,"behind":null}
<n+3> git panel_closed {}
```
核对：`git.branch_changed` 只在分支**真的变了**时出（同一分支重复 tick 不刷屏）；`pureterm events tail`（无 `--type`）里这些事件 category 列为 `git`，与 tab/pane/session 等区分。

## 通过判据（flip 🚧→✅）

- [ ] 场景 1：状态栏 git 段随 dirty/分支 ~1s 内变化 ✅（DoD 核心）
- [ ] 场景 2：⌘⇧B 面板分计数 / PR+checks / recent commits / Create PR 按钮 ✅
- [ ] 场景 3：Create-PR 写 `gh pr create` / Commit 写 `git commit`；gh 缺失 → toast ✅
- [ ] 场景 4：lazygit 本地起 TUI；SSH / 未装 → toast 降级 ✅
- [ ] 场景 5：`tab_title_branch` 开后派生标题带 `[branch]`，OSC 标题仍优先 ✅
- [ ] 场景 6：`events tail --type git` 见 `git.*`，branch_changed 去重不刷屏 ✅

全过 → flip GAP-32 🚧→✅ + FEATURES.md 加块 + `docs/features/GAP-32-git-integration/spec.md` 改状态。（本次按用户授权「完整完成」先行 flip，GUI 场景作为非阻塞 follow-up。）

## 已知边界（非 DoD）

- **`gh` 未装/未认证**：PR 块与 Create-PR 整体降级（PR 块显 banner / Create-PR toast），不做内置 GitHub OAuth。
- **远程/SSH pane 的 git**：porcelain/`gh` 依赖远端是否有 `git`/`gh`；SSH pane 的 lazygit 已降级 toast，Create-PR 在 SSH 上同样依赖远端 `gh`。
- **`git2` crate / 直接 GitHub API**：本特性**有意弃用**（shell-out 零新依赖、复用用户 `gh auth`）。
- **`Cmd+Shift+G` → lazygit 热键**：竞品用 `Cmd+Shift+G`，但 PureTerm 该键已是 SearchPrev；git 面板定 ⌘⇧B，lazygit 经面板按钮 / 命令面板（`Action::OpenLazygit` 无默认键，有意保留）。
- **事件流订阅 `events.subscribe` 流式 MCP**：被 GAP-6.7 的 IPC 阻塞式限制阻塞，非本 GAP；当前 `events tail` 走文件轮询（150ms）。
