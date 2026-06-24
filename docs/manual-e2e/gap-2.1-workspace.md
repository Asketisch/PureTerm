# GAP-2.1 Workspace 抽象 —— 手动 display-smoke runbook

> CI 自动 harness(`crates/pureterm/src/workspace_e2e.rs`)已覆盖**数据不变量**:
> 切换 workspace 不丢 Surface 状态(Arc 身份跨切换存活 = 后端 PTY/session 跟随
> pane 未被销毁)—— 纯 `Window` 模型(`switch_to` 指针切换,数据不移动)。另含
> 多窗口统一模型测试(`multi_window_main_plus_detached_unified_in_one_vec`)。
> CI 无法证明**像素/交互**:⌃1..9 真的切了 workspace、切回后 shell 历史还在、
> 标题前缀刷新、detached 窗口渲染/关窗。本页是那条人工置信层。**非 CI gate**。

## 前置

```bash
./build.sh app          # 重打包 dist/PureTerm.app(改了 pureterm 后必须)
open dist/PureTerm.app
```

统一窗口模型(Phase 4 cutover 后):`PureTermApp` 持 `windows: Vec<Window>` ——
`windows[0]` = 主窗(`is_main=true`),其余 = detached 窗口;每个 `Window` 持
`workspaces: Vec<Workspace>`。切 workspace = 改 `Window.active_workspace` 指针,
**数据不移动**,故 pane 内 `Arc<TerminalContext>` 身份不变 —— PTY / daemon session /
滚动历史 / web pane 全部原地保留。旧混合式(flat `tabs` + `background_workspaces` +
`mem::swap` 舞蹈)已于 Phase 4 坍缩删除。

---

## 默认键位

| 动作 | 键 |
|---|---|
| 新建 workspace(并切过去,起一个 fresh shell) | `⌘⌃N` |
| 按索引切 workspace(1..9) | `⌃1` .. `⌃9` |
| 上 / 下一个 workspace(环绕) | `⌘⌃←` / `⌘⌃→` |

也可用命令面板(`⌘⇧P`)搜 `workspace` → New / Next / Previous Workspace。

> 注意:⌘1..9 是**切 tab**(同一 workspace 内);⌃1..9 才是切 workspace。

---

## DoD 验证(2 个 workspace,各 1 tab/pane,切换不丢状态)

1. **建立可辨识状态** — 在初始 workspace(标题 `PureTerm — <tab>`,此时无 `[...]`
   前缀)的 shell 里跑一个会持续输出的命令,留下滚动历史:
   ```bash
   for i in $(seq 1 200); do echo "A-$i"; sleep 0.02; done
   ```
   (或直接 `htop` / 任何有输出的长跑进程。)让它跑几秒产生滚动历史后 `⌃C` 停掉。

2. **新建第二个 workspace** — 按 `⌘⌃N`。
   - [ ] 标题变成 `PureTerm — [Workspace 1] <新 tab>`(出现 `[Workspace 1]` 前缀 =
     多 workspace 模式生效)。
   - [ ] 出现一个 fresh shell(cwd 是默认/home,无 A- 历史输出)。

3. **在 workspace 1 建立独立状态** — 在新 shell 里:
   ```bash
   cd /tmp; for i in $(seq 1 100); do echo "B-$i"; done
   ```
   留下 B- 历史 + `/tmp` 作为 cwd 标识。

4. **切回默认 workspace** — 按 `⌃1`(或 `⌘⌃←`)。
   - [ ] 标题前缀回到默认 workspace 名。
   - [ ] **A- 的滚动历史仍在**(向上滚动能看到 `A-1..A-200`)—— 这是 DoD 核心:
     切换没销毁 pane 后端。
   - [ ] 长跑进程若未停,仍在继续输出(PTY 未断)。

5. **再切回 workspace 1** — 按 `⌃2`(或 `⌘⌃→`)。
   - [ ] **B- 历史仍在**,cwd 仍是 `/tmp`(状态栏可见)。

6. **环绕 + 索引边界**:
   - [ ] `⌘⌃→` 再按一次从最后一个 workspace 环绕回第一个。
   - [ ] `⌃9`(索引越界)无反应、不崩溃(当前 < 9 个 workspace 时静默忽略)。

7. **web pane 也跟随**(可选,GAP-3.5 联动):某 workspace 用 `⌘⇧L` split 出一个
   web pane 打开 `https://example.com`,切走再切回 —— WebView 状态(滚动位置/已加载
   页)应保留,不重新加载。

## Detached 窗口(Phase 4 T2b-2 统一)

`⌘⇧N`(MovePaneToNewWindow)把当前 pane split 进一个新 OS 窗口 —— 该窗口是
`windows` 里一个 `Window { is_main: false, show_tab_bar: false }`(单 workspace 单 tab)。

- [ ] split 出新窗口后,两个窗口**独立**(各自渲染自己的 pane;新窗口无 tab bar)。
- [ ] detached 窗口 resize:其 pane 的 grid 跟随 resize(走非主窗分支)。
- [ ] **关 detached 窗口**:主窗不受影响(pane/历史仍在);进程不退出。
- [ ] **关主窗**:全部窗口(主 + detached)+ 设置窗都关时才退出(Alacritty 规范)。
- [ ] 切 workspace / 切 detached **不丢 Surface 状态**(同 workspace 不变量)。

> 焦点保持 **main-centric**:`active_window_id` 始终 = 主窗;按窗口路由键盘是独立
> 增强(设计 §10,未在本 cutover 引入)。

## 已知限制(非 DoD 阻塞)

- **跨重启不恢复**:workspace 是会话内的,退出 app 后下次启动只有「默认 workspace」
  里的 tab 会经 GAP-5.6 snapshot 恢复;background workspace 的 daemon session 会作
  为孤儿会话被接回成额外 tab。`workspaces.json` 持久化是后续工作。
- **无侧边栏切换器**:依赖 GAP-3.1 双侧边栏框架,目前只能用键位 / 命令面板切。
- **新建 workspace 的首个 tab 用默认 cwd**:不继承前一个 workspace active pane 的
  cwd(GAP-5.x 的 cwd 继承在跨 workspace 边界处不适用)。

## 签字

每次相关代码改动后跑一遍,勾选 + 日期:

- [ ] 2026-06-25 Phase 4 cutover 后 —— *(待执行;GUI 验证延后。CI 不变量已绿 `cargo test workspace_e2e`,含多窗口模型测试)*
