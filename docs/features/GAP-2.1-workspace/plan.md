# GAP-2.1 实施计划 — Surface / Pane / Workspace / Window 抽象层

> 对应 GAP-2.1（已闭环，见 [`done.md`](../../done.md)）。实施计划归档。

## Context（为什么做）

PureTerm 目前是**两层扁平结构**：`PureTermApp { tabs: Vec<Tab> }`，每个 `Tab { panes, split_tree, active_pane }`。没有 Workspace（命名分组），Window 也只是 `main_window + detached_windows`（每个 detached 窗口退化成只装 1 个 Tab）。

这卡住了一批 Tier-0 特性：GAP-3.1（双侧边栏，左侧栏要列 Workspace）、GAP-6.7（事件流 `Category::Workspace` 已预留但无生产者）、GAP-3.2/6.8（可编程 sidebar 解释器）、cli 的第 9 个 `workspace` 命令组（`cli.rs:11` 已留 deferred 注释）。

GAP-2.1 引入 4 层头等抽象（Window / Workspace / Pane / Surface），把命名工作区做成一等公民，让"切换不丢 PTY 状态"成为运行时不变量，并为后续 sidebar / 事件流 / 编程化铺路。

**DoD**（贯穿全程不变量）：用户可创建 2 个命名 Workspace，各含 1 个 Tab、每个 Tab 1 个 Pane；切换 Workspace 不丢 Surface（PTY）状态——非活跃 Workspace 的 Tab 留在内存、PTY 继续运行。

## 范围（用户已定，3 项全选最大档）

1. **完整 4 层重构**：Window/Workspace/Pane/Surface 全部头等类型；PTY 后端概念重命名为 `Surface`（今天即 `PaneContent` 枚举）；把退化的 `detached_windows` 统一进 Window 抽象。
2. **切换 UX 三件套**：命令面板 `PaletteMode::Workspaces` + 快捷键 + 侧边栏切换器。⚠️ 侧边栏依赖 GAP-3.1（未建），故**侧边栏切换器延后到 Phase 6**，与 GAP-3.1 一同落地；核心 DoD 仅靠面板+快捷键即可达成。
3. **持久化：独立 `workspaces.json`，全量搬迁**：把所有 tab/pane/surface 细节从 `gui-sessions.json` 迁到 `workspaces.json`，废弃旧文件；首次启动做 v1→v2 迁移。

## 目标架构

### 类型层级与归属

**归属模型（推荐）**：`PureTermApp` 持 `Vec<Window>`；每个 `Window` 持 `Vec<Workspace>`；**Workspace 属于单个 Window，不跨窗口**（跨窗口共享会引入"谁的 viewport 驱动 resize / 谁的 scrollback 权威"等不可化简复杂度，无 GAP-2.1 需求支撑）。

```rust
// split_tree.rs —— Surface（PaneContent 重命名，保持枚举形态）
pub enum Surface {
    Terminal(Arc<TerminalContext>),
    Web(Arc<BrowserContext>),
}
pub struct Pane {
    pub id: PaneId,
    pub surface: Surface,   // 原 content: PaneContent
    pub title: String,
}
// 临时兼容别名，一个 release 后移除：
// pub type PaneContent = Surface;

// iced_app.rs —— 新增 Workspace / Window 层
pub struct WorkspaceId(u64);
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub tabs: Vec<Tab>,
    pub active_tab: TabId,
    pub next_tab_id: u64,
}
pub struct Window {
    pub window_id: window::Id,
    pub workspaces: Vec<Workspace>,
    pub active_workspace: WorkspaceId,
    pub is_main: bool,
    pub show_tab_bar: bool,
}
pub struct PureTermApp {
    windows: Vec<Window>,             // 取代 main_window + detached_windows
    active_window: Option<window::Id>,
    // …其余非窗口字段不变…
    next_workspace_id: u64,
}
```

**关键：accessor 层把 372 KB 的改动约束成机械重写。** 现有所有 `self.tabs` / `self.active_tab` 站点改成走这些访问器，而访问器返回的仍是 `&Tab`/`&mut Tab`，故 split/close/swap/search 等 handler 的**函数体不变**，只改 lookup 前缀：

- `active_window_mut() -> Option<&mut Window>`
- `active_workspace() -> Option<&Workspace>` / `_mut`
- `active_tab_ref() -> Option<&Tab>` / `active_tab_mut`（现 `iced_app.rs:1611`，改为 workspace → tab）
- `window_of(id) / window_of_mut(id)`

> 已验证 `active_tab_ref/mut`（`iced_app.rs:1611-1617`）正是 `self.tabs.iter().find(...)` 返回 `Option<&Tab>`——重写为 `active_workspace()?.tabs.iter().find(...)` 是 1 行改动。

### 机制 1：切换不丢状态

PTY/Surface 不丢，**因为切换时什么都不 drop**：`TerminalContext` 在 `Arc` 里（`Pane.surface` → `Tab.panes` → `Workspace.tabs` → `Window`）。切换 Workspace **只改一个指针** `window.active_workspace = target`；旧 Workspace 的 `Vec<Tab>` 和所有 `Arc<TerminalContext>` 仍被 `Window` 持有。

- `active_tab` 天然 per-workspace（每个 `Workspace` 自带 `active_tab: TabId`）。
- Tab 栏（`view_top_bar` `iced_app.rs:6626`）只遍历**活跃 Workspace 的 `tabs`**；非活跃的不出现。
- 后台 Workspace 的 PTY 仍由既有 `iced::Subscription` 流驱动（daemon → channel → `TerminalWakeup` 对 app 持有的**所有** context 触发，不仅可见的——Phase 3 e2e 必须验证后台 Workspace 持续产出输出）。
- `title()`：`PureTerm — {ws_name} — {tab_title}`。
- **禁止** `mem::take`/`swap_remove` Workspace 的 tabs——切换唯一变更是 `active_workspace`。

### 机制 2：Window 层统一

用 `windows: Vec<Window>` 取代 `main_window + detached_windows`。

- **进程退出谓词**（`iced_app.rs:2877`）→ `self.windows.is_empty() && self.settings_window.is_none()`。
- `WindowClosed(id)`：先持久化（主窗，已做于 `:2858-2862`），再从 `self.windows` 移除该 Window。
- `view(id)`（`:7975`）：detached 特殊分支（`:7982-8010`）并入正常路径，每个 Window 都渲染 `view_top_bar + view_split_tree(active_tab) + view_bottom_bar`；`show_tab_bar=false` 保留旧 chrome-less 行为。
- `MovePaneToNewWindow`（`:3993`）：不再 push `DetachedWindow`，而是建一个新 `Window`（单 workspace + 单 tab）——同一 UX，一条代码路径。
- **顺带修一个 latent bug**：`capture_layout()`（`:1462`）今天只序列化 `self.tabs`，detached 的 tab 从不持久化；改为遍历 `windows → workspaces → tabs` 后免费修复。

## 持久化迁移（`workspaces.json` v2）

```rust
pub const SNAPSHOT_VERSION: u32 = 2;            // 原 1
const SNAPSHOT_FILE: &str = "workspaces.json";  // 原 gui-sessions.json

pub struct WorkspaceSnapshot { pub name: String, pub active_tab: usize, pub tabs: Vec<TabSnapshot> }
pub struct WindowSnapshot   { pub is_main: bool, pub active_workspace: usize,
                              pub workspaces: Vec<WorkspaceSnapshot>, pub viewport: Option<ViewportSnapshot> }
pub struct LayoutSnapshot   { pub version: u32, pub windows: Vec<WindowSnapshot> }
```

`TabSnapshot`/`PaneSnapshot` **原样复用**（已含 `session_id`/`kind`/`ssh_profile`/`agent_*`/`url`）。

**v1→v2 迁移**（`session_restore.rs` 新增 `migrate_v1_to_v2()`，首次启动一次性）：
1. `workspaces.json` 已存在 → 加载 v2，结束。
2. 否则 `gui-sessions.json` 存在 → 解析为 v1，把 `tabs` 包进**一个** `WorkspaceSnapshot { name: "Default", tabs, active_tab }`，再包进一个 `WindowSnapshot { is_main: true, active_workspace: 0 }`。
3. 写 `workspaces.json`，然后 `gui-sessions.json` → `gui-sessions.json.bak`（**绝不 unlink**，防数据丢失），记日志。
4. 之后启动永不碰 `.bak`。

`restore()` 返回类型从 `RestoreOutcome { tabs, active_tab_index }` 拓宽为 `{ windows: Vec<RestoredWindow { is_main, workspaces: Vec<RestoredWorkspace { name, tabs, active_tab_index }> } }`；per-tab 的 attach/prune/orphan 逻辑（`:209-373`）不变，只是把扁平结果重新分组。原子写 + 250ms-tick/500ms-idle 防抖 + flush-on-close 机制（`save_atomic`/`LayoutSaveTick`）**原样复用**，只改生产者 `capture_layout` 的形状。

## 切换 UX

- **命令面板**：`PaletteMode::Workspaces`（第三个变体，`iced_app.rs:740`），`refresh_palette_matches_inner`（`:2290`）加 workspaces 分支列 `WorkspaceId + name`，`run_palette_entry` 分发。
- **快捷键（避开已占用 ⌘1..9 / ⌘[ ] / ⌘{ }）**：`⌃1..9`（Control+数字）= `ActivateWorkspaceByIndex`；`⌘⌥←/→` = 上/下一个 workspace；新建 workspace 走面板或 `⌘⌃N`。
- **侧边栏切换器**：Phase 6，随 GAP-3.1 落地（现在只建 `WorkspaceSidebarState` 占位 stub，不渲染）。

## 分阶段实施

每阶段独立编译、过 `cargo test`、app 全程可用。遵循项目 TDD + `*_e2e.rs` 约定；闭合 🚧 GAP 时 runbook + auto-harness + 状态翻转同提交（见 memory [[closing-inprogress-gap-to-done]]）。

| Phase | 内容 | DoD 贡献 | 测试 |
|---|---|---|---|
| **1. Surface 重命名 + 结构骨架** | `PaneContent`→`Surface`（+临时别名）；`Pane.surface`；`terminal()/web()/is_web()` 改 match；新增 `WorkspaceId`/`Workspace`/`Window` 结构（**定义但不接线**）+ accessor stub | 解锁全部，零 UX 变化 | 现有快照 round-trip 仍绿；加 Surface match 穷尽测试 |
| **2. 单 workspace/单 window 接线（行为对等）** | 用 `windows: Vec<Window>`（启动 = 1 主窗 × 1 workspace）取代 `main_window`/`tabs`/`active_tab`/`detached_windows`；实现全部 accessor；重写所有 `self.tabs`/`self.active_tab`/`self.main_window`/`self.detached_windows` 站点；`MovePaneToNewWindow`→建新 Window；改退出谓词/`title`/`view`/`capture_layout` | Window 层落地，仍单 workspace | `join_pane_e2e.rs` + 新 `workspace_e2e.rs`（启动/detach/关窗/退出） |
| **3. 多 workspace + 切换（**🎯 DoD 在此交付**）** | `create/switch/rename/close_workspace`；`Action::WorkspaceNext/Prev/ActivateWorkspaceByIndex/NewWorkspace/RenameWorkspace`（`keymap.rs:28`）；默认绑定（`:200-302`）；`parse_action`（`:707`）；`PaletteMode::Workspaces` + 条目；Tab 栏只遍历活跃 workspace | **GAP-2.1 DoD** | **TDD DoD** `workspace_state_e2e.rs`：建 2 命名 workspace 各 1 tab 各 1 pane；各起长跑 PTY（`for i in seq 1 100; do sleep 1; echo $i; done`）；切 A 计数 N → 切 B 计数 M → 回 A 断言计数已超过 N（后台 PTY 未停）；debug-only 断言 `Arc<TerminalContext>` 强引用计数切换不降。runbook+harness+翻转同提交 |
| **4. 持久化迁移**（`workspaces.json` v2） | `WorkspaceSnapshot`/`WindowSnapshot`；版本 2；新文件名；`migrate_v1_to_v2()` + `.bak`；`capture_layout` 产 `windows→workspaces→tabs`；`restore` 分组回填 | 全量持久化 spec；废弃 `gui-sessions.json` | 迁移单测（v1→单 Default workspace）；2-workspace round-trip；重启重连保活（daemon session） |
| **5. CLI `workspace` 命令组**（`cli.rs:11`） | `pureterm workspace new\|switch\|list\|rename <id>` 经 IPC → `Message::*Workspace*`，填上第 9 个 deferred 组 | 编程化入口 | CLI 冒烟 |
| **6.（延后，依赖 GAP-3.1）侧边栏切换器** | `WorkspaceSidebarState` 占位 stub（Phase 1 已建）；GAP-3.1 双侧边栏框架就绪后插入真实 widget | 第三种切换 UX | 随 GAP-3.1 |

**里程碑**：Phase 1-3 闭合即达成 GAP-2.1 DoD（最小可交付）。Phase 4-5 完成"完整重构 + 全量迁移"承诺。Phase 6 随 GAP-3.1。

## 关键文件

- `crates/pureterm/src/split_tree.rs` — `PaneContent`→`Surface`（`:48-83`），`Pane` 字段 + accessor。
- `crates/pureterm/src/iced_app.rs` — 加 `Workspace`/`Window`/`WorkspaceId`（`~:47`）；替换 `PureTermApp` 上 `main_window`/`tabs`/`active_tab`/`detached_windows`（`:87-89,182`）；重写 accessor（`active_tab_ref` `:1611`/`active_tab_mut` `:1615`）、`capture_layout` `:1462`、`title` `:2492`、`view` `:7975`、`view_top_bar` `:6626`；`WindowClosed`/退出谓词（`:2853,2877`）；`MovePaneToNewWindow` `:3993`；`Action`→`Message` 桥 `:4977-5046`；面板 `:2290`/`:740`。
- `crates/pureterm/src/session_restore.rs` — `WorkspaceSnapshot`/`WindowSnapshot`、v2、`workspaces.json`、`migrate_v1_to_v2()`、拓宽 `restore()` 返回（`:153-165,172`）。
- `crates/pureterm/src/keymap.rs` — `Action::Workspace*`（`:28`）、默认绑定（`:200-302`）、`parse_action`（`:707`）。
- `crates/pureterm/src/cli.rs` — 实现 deferred `workspace` 组（`:11`）。
- 次要（`PaneContent`→`Surface` 机械更新）：`surface_snapshot.rs`、`agent_sessions.rs`、`browser/`。

## 风险与对策

1. **372 KB `iced_app.rs` 改动面** → accessor 层（返回相同 `&Tab`/`&mut Tab`）把约上百个 `self.tabs` 站点变成 1 行机械重写；Phase 1（重命名）/Phase 2（接线）拆开各自可审；每子步后 `cargo test`。
2. **侧边栏/GAP-3.1 依赖** → Phase 3 仅靠面板+快捷键即达 DoD；侧边栏明确 Phase 6/延后；占位 stub 防前向不兼容。
3. **快捷键冲突** → `⌘1..9`/`⌘[`/`⌘]`/`⌘{`/`⌘}` 全占用；workspace 走 `⌃1..9`（Control，不同修饰键）+ `⌘⌥←/→`。已核对 `keymap.rs:200-302`。
4. **迁移数据丢失** → v1 文件改名 `.bak` 永不删；迁移一次性，"workspaces.json 存在?" 守护；单测覆盖 v1→v2；解析失败回退全新启动（不劣于今天 `load()` 返回 `None`）。
5. **detached 语义变化** → detached 升格为完整 Window（可能多出 tab 栏）；需对等则 `show_tab_bar=false`。顺带修"detached tab 从不持久化"latent bug。
6. **PTY 保活不变量（DoD 核心）** → 切换只改 `active_workspace`；Phase 3 e2e 显式断言后台 PTY 持续产出；加 debug-only 强引用计数断言。
7. **`iced::daemon` 多窗口归属** → `view(id)`/`title(id)` 按 `window::Id` 调用；新 `window_of(id)` 查找 O(窗口数)，单 digit 量级可忽略。

## 验证（E2E）

1. **单测/`cargo test`**：每 Phase 后全绿；重点 `workspace_state_e2e.rs`（Phase 3 DoD）、`session_restore` 迁移单测（Phase 4）。
2. **手动 GUI E2E**（DoD 闭环门槛，按 memory [[app-runs-from-dist-bundle]] 用户跑 `dist/PureTerm.app`，需 `./build.sh release` 重打包）：
   - 建两个命名 workspace，各 1 tab 各 1 pane。
   - 各起 `for i in $(seq 1 100); do sleep 1; echo $i; done`。
   - `⌃1`/`⌃2` 切换；面板 `⌘⇧P` → Workspaces 模式切换。
   - 切回后断言长跑 PTY 计数已前进（后台未停）。
   - 关窗重开，断言两个 workspace + 各 tab/PTY 经 daemon 重连复活（scrollback 回放）。
3. **迁移**：保留旧 `gui-sessions.json` 的环境下首次启动 → 生成 `workspaces.json`（含一个 "Default" workspace）+ `gui-sessions.json.bak`；PTY 经 daemon 存活重连。
4. **闭合 GAP**：Phase 3 通过即移入 `done.md` 并同步 `FEATURES.md`。
