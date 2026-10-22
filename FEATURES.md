# PureTerm 已实现特性

> 本文件汇总 PureTerm 所有已闭环(✅)特性,按类别组织。路线图已全部闭合 → [`docs/done.md`](docs/done.md)；[`docs/todo.md`](docs/todo.md) 当前无待办项。
> 更新约定:新特性闭环后同步本文件 + `docs/todo.md` → `docs/done.md` 状态迁移。

---

## A. 核心架构

### GAP-2.2 嵌套水平+垂直分屏(2026-06-10)
- 水平/垂直递归分屏,拖拽调整比例(MIN_RATIO=0.1, MAX_RATIO=0.9)
- 补齐操作:等分所有分屏 `⌘=` / 交换 Pane `⌘⇧X` / 临时最大化 `⌘⇧Enter` / 焦点 Next/Prev `⌘]` `⌘[` / 移动到新 Tab `⌘⇧T` / 新窗口 `⌘⇧N`
- 键盘 + 右键菜单 + 命令面板三入口,命令面板搜 `split.*` 7 个 builtin
- 实现:`crates/pureterm/src/split_tree.rs`(equalize_all / swap_with_sibling / neighbor_in_order) + `iced_app.rs`(Tab.zoom_state + 6 Message + 5 ContextMenuItem + 7 命令面板 builtin)
- 单测:`split_tree::tests` 11 个(嵌套 3 层 / wrap / swap 二次回归 / 根 Leaf 边界)

### GAP-2.3 Socket API 协议 + 端口模型(2026-06-13)
- JSON-RPC 2.0 over Unix domain socket,31 个 method 跨 8 命名空间
- 命名空间:system(4) / window(4) / tab(4) / pane(6) / surface(4) / config(4) / ssh(3) / debug(2)
- 写操作通过 `IpcSideEffect` 枚举回写 iced 主循环,真实驱动 UI 状态变更
- 实现:`crates/pureterm/src/ipc/`(protocol / methods / listener / client)
- 单测:45 个(`ipc::protocol::tests` 6 + `ipc::methods::tests` 39)

### GAP-2.4 远程守护进程 puretermd 等价物(2026-06-22)
- 由 GAP-5.1 的 `puretermd` 实质满足:独立守护进程 + 持久 PTY + 关 GUI 不丢会话
- 本机 Unix-domain socket,非网络远程(网络远程属 GAP-5.2 SSH tunnel 范畴)

### GAP-2.5 多 UI 路径统一治理(2026-06-12)
- CLAUDE.md/AGENTS.md 一致描述"主路径=Iced";`pureterm-alacritty-runtime` 仅含 `glyph_atlas`(964 行)
- 新贡献者无歧义

### GAP-2.1 Surface/Pane/Workspace/Window 抽象层(2026-06-25)
- 4 层抽象:`Surface`(单 PTY/Web 内容载体)/ `Pane`(容器)/ `Workspace`(命名工作区)/ `Window`(顶级窗口);`PureTermApp.windows: Vec<Window>` 唯一真源(主窗 `is_main=true` + detached),每个 Window 持 `workspaces: Vec<Workspace>`,切 workspace = 改 `active_workspace` 指针(数据不移动 → Arc 平凡存活 = 后端 PTY/session 不丢)
- UX:键 ⌃1..9(按索引切 ws)/ ⌘⌃←→(prev/next)/ ⌘⌃N(新建)/ `⌘⇧N`(split pane 进 detached);命令面板 `workspace.{new,next,prev}`;多 ws 时标题带 `[name]` 前缀
- **detached 键盘路由**:获焦窗口(`iced::window::Event::Focused`)→ `active_window_id` → 按键/IME 落该窗活跃 terminal(此前 main-centric:detached 能渲染但收不到独立按键)
- **workspaces.json v2 持久化**:workspace/window 结构跨重启存活(PTY/session/scrollback 全保留);v1 `gui-sessions.json` 一次性迁移(`wrap_v1_into_v2`,旧文件重命名 `.bak` 永不 unlink);恢复时 detached 折叠进主窗(窗口拓扑不跨重启,会话全保留)
- 实现:`crates/pureterm/src/iced_app.rs`(`Window`/`Workspace` 模型 + `build_layout_snapshot` + 焦点路由)+ `session_restore.rs`(v2 快照树 + `restore_one_tab` + 折叠恢复)+ `keymap.rs`(Action/键位)
- 自动 E2E harness 全绿:430 pureterm 单测(`workspace_e2e` 切换不丢 Surface Arc 身份 + 多窗口模型 / `keyboard_routing_e2e` 焦点指针路由 / `session_restore` v2 round-trip + v1→v2 迁移 / `build_layout_snapshot` 捕获全窗口)
- pixel smoke:`docs/reviewed/GAP-2.1-workspace/test-plan.md`(非 CI gate,detached 真实焦点 + 跨重启恢复待人工执行)
- **未做(独立后续)**:CLI `workspace` 命令组(独立 phase);侧栏拖拽调宽(defer);close / rename workspace 操作(plan.md Phase 3 曾列但从未实现,DoD 不依赖,记 follow-up)
- **Review（2026-06-29）**:修 5 bug —— ① v1→v2 迁移改「先写盘、成功后才把旧 v1 改名 `.bak`」(写失败不丢 v1,`save_atomic_to()->bool` + `migrate_v1_to_v2_in` 路径参数化);② 关闭 detached 窗口补 `mark_layout_dirty`(此前 retain 后不排期持久化 → 重启复现);③ `flush_layout_if_dirty` 抽 `dirty_should_clear` 纯函数,仅在 capture 前后时间戳一致时清 dirty(防 debounce 丢最终态;iced 单线程故当前非 live bug,纯防御硬化);④ `WindowClosed` 焦点回退由「仅弹回主窗」改为「回退到任意存活窗口」,修「聚焦主窗被关 + detached 存活」时 `main_window` 已 None → snap-back 跳过 → `active_window_id` 悬空 → 下条按键撞 `tabs().expect` panic;⑤ `NewWorkspace` 加无活跃窗口防御早退(避免下方无条件 `create_tab` → `.expect` latent panic)。+ 后台 PTY 持续产字进 grid 的 DoD 浴(`gap21_background_grid_accumulates_without_event_polling`,补既有「仅 Arc 身份」较弱断言)+ 迁移先写后改名 tempdir 单测 + debounce re-arm 单测;wakeup loop 注释订正(只扫活跃窗口·活跃 workspace,后台 grid 字节靠 alacritty IO 线程独立喂养)。745 pureterm 单测全绿。

### 端到端测试体系 + 启动性能基线(2026-10-21)
- **Python E2E 套件**(`tests/e2e/`,16 个文件 / 226 例;Rust 全仓 1523 例):覆盖 Rust 单测够不到的**跨进程接缝** —— CLI ↔ GUI ↔ `puretermd` ↔ MCP ↔ 窗口服务器
  - `test_cli_local.py` 本地命令组(退出码 / JSON 形状 / 配置 CRUD / 插件 / 记事本 / hook 垫片)
  - `test_cli_ipc.py` + `test_rpc.py` Socket API(逐个已注册方法 + 错误路径 + 非法参数)
  - `test_daemon.py` 守护进程生命周期、会话增删查、**GUI 冷启动是否真的挂上守护进程**
  - `test_mcp.py` MCP stdio 协议(握手 / `tools/list` / `tools/call` 与 GUI 的桥接)
  - `test_display.py` + `test_background.py` **显示层像素检测**:抓真实窗口截图,断言非空白、字形已光栅化、背景色符合配置、无 panic 覆盖层、背景图通道真的画出来
  - `test_startup.py` 启动里程碑回归护栏 + 耗时上限
  - `test_session.py` 会话跨 GUI 重启存活
  - `test_graphics.py` **终端图形链路**:Kitty 图形 → 事件 → 存储 → 解码 → GPU 图集 → 屏幕(截图断言)
  - `test_browser.py` **内嵌浏览器**:本地 HTTP 服务器 → `pane split-web` → `browser.get-title` / `extract` / `navigate` / `wait`(含**页面脚本动态插入**的元素)
  - `test_ssh.py` **SSH 通道**:假 `ssh` 二进制 + `[[ssh.profiles]]` → `ssh.connect` → 事件日志确认「标签页建成 / 会话起来 / 连上了」
  - `test_triggers.py` **模式匹配触发器**:`[[triggers]]` → 输出匹配 → `trigger.matched` 事件 → notify / send_text / exec / set_variable 动作
  - `test_tmux_cc.py` **tmux 控制模式**:真 tmux 会话接进 GUI(标签页 / 当前屏幕 / 按键回传)
  - `test_agents.py` **agent 适配器**:安装 / 卸载 Claude Code 钩子(幂等 / 不动用户配置 / dry-run 不写盘)
  - `test_windows_config.py` **多窗口 / 工作区 / 配置读写 / 分割树手术**(`window.create|focus|close`、`workspace.switch`、`config.get|set`、`pane.detach|attach|swap|equalize|resize`)
- **守护进程泄漏(把机器拖垮)**:守护进程**故意**在 GUI 退出后继续存活(会话要扛过 GUI 重启),但**没有任何自停手段** —— 用户没法回收它,自动化测试更是每个用例泄漏一个。实测 E2E 套件跑到一半时机器上堆了 **401 个 `puretermd`**,套件耗时从 85s 涨到 **345s**,大批用例超时
  - 新增 `daemon.shutdown` RPC:回完应答后杀掉全部会话、清掉控制 socket、退出进程
  - 新增 `pureterm daemon stop` CLI:用户终于能回收它(此前只有 `status` / `install` / `uninstall`)
  - E2E fixture 在每个用例收尾时回收自己那个守护进程(沙箱给每个用例**专属**的 daemon socket,互不影响)
- **八个「空实现 / 谎报成功」的 API**(此前**完全没有 E2E 覆盖**,审计后逐个发现并修掉):
  - `window.focus` 回 `{"focused": true}` 却**没有任何副作用** —— 调用方以为窗口被聚焦了,实际什么都没发生 → 真的切窗口
  - `window.list` 的窗口数取自 `main_window`(恒 ≤1),且**每个**窗口都报**活动窗口**的标签页数 → 改为按 `self.windows` 报每窗口自己的标签页数 + 活动标志
  - `config.set` 回一句「not yet supported」却带**成功**状态 → 真的经 `mutate_config` 写盘(保留文件其余内容)+ 重载
  - `config.get` 只回 `key`、没有 `value` → 真的按点分键路径读值
  - `workspace.switch` 对**不存在**的工作区 id/name 回「switching: true」,主循环却什么也没做 → 改成报错
  - `pane.attach`(join-pane)拒绝移动「标签页里**唯一**的窗格」→ 刚被 `pane.detach` 拆出来的窗格**永远并不回去**(detach → attach 往返不成立),而 RPC 还回「attached: true」→ 允许移动,并把变空的源标签页一并删掉
  - **默认配置里 `font.size` 放在 `[font.normal]` 下,而加载器与设置 UI 都读 `[font].size`** → 默认配置中的字号**根本没被读到**;用户照着默认文件改 `size` 也毫无效果
  - `pureterm tmux-attach --socket` 与顶层**全局** `--socket`(IPC 路径)同名 → clap 在**解析时 panic**,命令行直接崩 → 改名 `--tmux-socket`
- **两个致命 panic(远程内容可触发)**:
  - 触发器模板按**字节**切片 → 模板含非 ASCII 字符(如中文标题「构建完成」)就切在字符边界之外 panic;触发器在 **GUI 主线程**求值,panic 直接把**整个应用 abort 掉**(终端消失,不只是这条规则失效)。改为按 `char` 扫描
  - `hover_preview` 的 favicon 解析用字节长度切 HTML → 远程页面里一个中文标题或 emoji 就足以 panic。切片前先把下标向下取整到字符边界
- **tmux 控制模式(`tmux -CC`)修复 + GUI 侧接线**(此前该后端**从未工作过**,且 GUI **从不消费**它):
  - `tmux -CC` 必须在 **pty** 上启动(它对 stdin 做 `tcgetattr` 进 raw 模式;用管道会直接报 `tcgetattr failed` 退出)
  - slave 端设 raw 模式(否则我们写进去的 `%…` 命令会被行规程**回显**回来污染信封流)
  - `%output` 载荷是 tmux 的 `\ooo` **八进制转义**(不是 base64)—— 早前按 base64 解码,导致**每一帧终端输出都被整条丢弃**
  - `%begin` 的真实格式是 `%begin <ts> <version> <flags>`(全局帧边界,**不带 pane id**)
  - 首行前的终端能力查询(`ESC P 1000 p`)不再吞掉第一条信封
  - 新增 `%window-add` / `%sessions-changed` / `%client-session-changed` / `%layout-change` 等通知类型
  - 3 个真实 tmux E2E 用例进入常规测试套件(缺 tmux 时优雅跳过)
  - **GUI 侧接线**:`TerminalContext` 新增 `Tmux` 变体 + `tmux_cc::pump` 把 `%output` 信封泵进网格 + `tmux.attach` IPC + `pureterm tmux-attach --session <name>`(此前该命令只**打印**一行 `tmux -CC …` 就退出 0,看起来「附加成功了」而实际什么都没发生)
  - tmux 3.6a 附加时**不回放屏幕**(只发 `%begin`/`%end`/`%session-changed`)→ 收到 `%session-changed` 后主动发 `capture-pane -p`,把命令输出块(`%begin flags=1`)里的原始行喂进网格
  - tmux 3.6a **不接受**客户端发来的 `%input`(回 `parse error: syntax error`,按键根本没送达)→ 改用 `send-keys -t %<pane> -H <逐字节十六进制>`;目标 pane id 由 `%output` 信封带出
  - **tmux socket 命名空间**(`tmux -L <name>`):测试用独立 socket,既不污染用户自己的 tmux 会话,也避开「多个控制模式客户端并发连接会让 tmux 服务器在启动期随机退出」的竞态;`tmux-attach --tmux-socket <name>` 也可指定
- **终端图形链路修复**(此前**大图一律不显示**):
  - 跨读取边界的图形序列不再被丢弃(PTY 一次 `read()` 不保证返回全部字节,2KB 以上的图像载荷几乎必然跨边界)→ 增加尾巴保留
  - Kitty 的 `c`/`r` 是**单元格**数而非像素数 → 像素尺寸改由载荷长度推导
  - 图像图集命中缓存时**重新上传像素**(同 id 重传原本永远显示旧图)
- **SSH 通道**:`ssh.connect` 经 IPC 打通「配置 → 守护进程 → `ssh` 二进制 → PTY → 会话」全链路,并有事件日志断言
- **内嵌浏览器(GAP-7.8)修复**:
  - `browser.wait` 原本让 JS 返回 **Promise**,而 WKWebView 的 `evaluateJavaScript` 默认**不 await Promise**(直接回 `undefined`)—— 该操作**永远**报 “no value”,形同虚设
  - 改法:早期用 JS 里 `while(true)` **同步忙等**——但那会**占住页面的 JS 线程**,页面靠 `setTimeout` 插入的元素永远出不来,`wait` 对动态元素必然超时。现改为 **Rust 侧轮询**(iced 定时器驱动,每次一次非阻塞 `querySelector` 探测),两次探测之间页面线程空闲
  - WKWebView 只在窗口**可见/在前台**时才加载页面并执行脚本求值 → E2E 需要先把应用置前台
- **启动埋点**:`PURETERM_TRACE_STARTUP=1` 把 `main_enter` / `first_frame` / `terminal_ready` 等里程碑打到 stderr(`crates/pureterm/src/startup_trace.rs`)
- **基准工具**:`scripts/bench_startup.py`(分阶段耗时)、`scripts/ab_startup.py`(两二进制交替对照,直接轮询 IPC socket 以剔除测量开销)、`cargo run --example wgpu_probe`(wgpu 各阶段成本探针)
- **启动优化**:背景图 / Sixel 两条 GPU 管线**按需创建**(`LazyPipeline`)—— 每次启动少编译 2 个 Metal shader library(实测约 80ms 的 Metal 编译);守护进程在应用构造阶段**预热**,与窗口创建并行;字体泛型名按平台解析(macOS/Windows 上 `monospace` → `Menlo`/`Consolas`)
- 详见 [tests/e2e/README.md](tests/e2e/README.md)

---

## B. UI/UX 界面


### GAP-3.3 命令面板 ⌘⇧P(2026-06-08)
- nucleo-matcher 模糊搜索,Actions / Profiles 双模式
- 实现:`crates/pureterm/src/iced_app.rs`

### GAP-3.4 状态栏(2026-06-12)
- 底部 24px:CWD(OSC 7) · Git 分支(30s 轮询) · SSH 连接指示 · 前台进程名 · 光标位置(row:col)
- 2026-06-22 GAP-7.6 加 agent 指示:`⚡ kind` / `❄ kind` / `⏳ N`
- 留后续迭代:顶部 title bar shortcut hint pills / 菜单栏 extra badge(CPU/内存已于 GAP-24 完成)

### GAP-24 增强状态栏(2026-06-26)
- 把原 monolithic `view_bottom_bar`(6 段 inline `text`)重构为 `StatusBarComponent` trait + `Vec<Box<dyn StatusBarComponent>>` 注册表;每段自渲染 `iced::Element<'static>`,`view_bottom_bar` 只组装 `BarData` 快照(SSH/Vi/agent 的 cfg-gated 格式化留在 in-module)
- 新增段:**CPU / Memory / Network(带 sparkline)+ Clock + Battery**;原有 Cwd/Git/Ssh/ViMode/Process/Agent/Cursor 全部移植上 trait
- **Sparkline**:`iced::widget::canvas` 自定义 `SparklineProgram`,移植业界 sparkline bezier 曲线数学(右锚定 / 自动缩放 ceiling floor 1.0 防 /0 / 60 点窗口;`path::Builder` 非 Clone 故 stroke/fill 两 builder 共享点集)
- **系统指标**:单一长生命周期 `iced::stream::channel` 发布者,持唯一 `sysinfo::System`/`Networks` + `battery::Manager`,1 Hz(可配)采样 + 120 点 ring buffer → `Message::StatusBarStats`;macOS `battery::Manager` 持 `Rc<IoKitManager>`(!Send),故每 tick 用 sync IIFE 重建并在 `.await` 前释放,保 future Send
- **Git 增强**:`git status --porcelain=v2 --branch` 一条子进程出 branch + dirty(●)+ ahead/behind(↑↓);`Task::perform` 跑在 GUI 线程外;`git` 缺失/非 repo 回退纯 FS `read_git_branch`(行为不回退)
- **配置**:`[statusbar]` 段(`components` 顺序列表 / `refresh_interval_secs` / `show_sparklines` / `bar_height`);sparkline 开时 `effective_height()` 自动 ≥28px;`terminal_content_height` 改为运行时高度参数(26 处调用点更新)
- 刷新模型:专用 `StatusBarTick`(进程名 sync + git 异步)+ `StatusBarStats`(指标扇出)+ `GitStatusResult`(异步 git 回流,仅当 cwd 仍属活跃 pane 才 apply);删除原 30s piggy-back 轮询
- 实现:`crates/pureterm/src/status_bar/`(mod/component/components/sparkline/publisher/stats/git)+ `app_config.rs`(`StatusBarConfig`/`load_statusbar`)+ `iced_app.rs`;新依赖 `sysinfo`/`battery`/`chrono` + tokio `process` feature
- 单测:`status_bar::git::tests` 10 个(porcelain v2:clean/dirty/untracked/ahead-behind/detached/no-upstream/non-git + `parse_head` 3);dev check 干净;release + `dist/PureTerm.app` 打包通过(30M arm64)
- **DoD 待人工**:GUI 真机 E2E(sparkline 动 / git dirty 标记 ~1s 出现 / 切 pane cwd/git 跟随 / 无电池优雅降级);MVP 范围 — 搜索框/snippet/composer/triggers/progress defer 后续

### GAP-19 Dual Sidebar（双侧边栏 MVP）(2026-06-29, Review ✅ 2026-07-01)
- 主布局 **left | terminal | right** 三列;⌘B 左侧(Workspace 列表 + Git 分支摘要),⌘⇧B 右侧(Find / Feed / Git / **Ports** 四 tab)
- **左侧**:点击 workspace → `ActivateWorkspaceById`;底部 Git 摘要复用 status-bar 异步状态
- **右侧 Find**:内嵌 ⌘F surface search + ⌘⇧F ripgrep(打开搜索时自动展开右侧栏);overlay 版在侧栏 Find tab 激活时隐藏
- **右侧 Feed**:审批面板从 pinned overlay 迁入 Feed tab;新请求自动打开右侧栏;`feed_panel_visible()` 与侧栏互斥
- **右侧 Git**:原 ⌘⇧B Git overlay 迁入 Git tab;`ToggleGitPanel` 保留为 legacy 别名
- **配置**:`[sidebar] left_width` / `right_width`(默认 240,120–480 clamp) / `left_default_open`
- **每窗口独立(Review)**:`SidebarState`(含运行时宽度)挂在 `Window` 上,A 窗 toggle/拖拽不影响 B 窗;视图按 `view(id)` 取窗(修了「B 窗渲染 A 窗 workspace 列表」latent bug)
- **拖拽调宽(Review)**:左/右分隔线 5px `mouse_area` → `SidebarDividerPress(Side)`;实时拖动复用 `GlobalCursorMoved`/`GlobalMouseRelease`(同 pane divider),clamp 120–480,逐帧 `resize_active_tab_panes`
- **事件对称(Review)**:`right_sidebar_event(now_open)` 纯函数锁「每次 toggle 恒发」;修 BUG-1(开 Git tab 早 return 漏发 `sidebar.right_opened`)+ BUG-2(关 Git tab 漏发 `git.panel_closed`)
- 终端列宽:`terminal_viewport_width()` 扣除 open 侧栏;PTY resize 全路径已接线
- 实现:`crates/pureterm/src/sidebar/`(mod/left/right) + `iced_app.rs` + `keymap.rs` + `app_config.rs`
- 单测:`sidebar::tests` 4 绿 + `app_config::load_sidebar` 2 绿;全量 772 绿 0 回归

### GAP-11 即时回放 MVP（2026-06-29）
- **数据源**:从 `events.jsonl` 加载 shell/surface/session 时间线(非 DVR delta 压缩)
- **命令行**:`pureterm replay list` / `pureterm replay export --output replay.jsonl`
- **GUI**:⌘⇧R 打开底部回放条,◀/▶  scrub,按当前 session 过滤
- 实现:`crates/pureterm/src/instant_replay.rs` + `iced_app.rs` + `cli.rs`
- **未做**:grid 快照回放 / run-length 压缩 / 独立回放窗口

### GAP-12 注释与标记 MVP（2026-06-29）
- **Marks**:OSC 133 shell marks → `PaneMarks` 网格行标记(Prompt/Command/Output/Error)
- **导航**:⌘⇧↑/↓ 跳上/下一个 mark,自动 scroll 到目标行
- **Annotations**:行级 `LineAnnotation` 存储(MVP 通过 API/OSC hex 扩展预留)
- 实现:`crates/pureterm/src/annotations.rs` + `iced_app.rs`(ShellMark 接线)
- **未做**:行内图标 UI / 右键添加注释 / Marks 侧栏列表

### GAP-25 多窗口 MVP（2026-06-29）
- **新窗口**:⌘⇧N → `Window::new_secondary`(tab bar + workspace,区别于 detached 单 pane)
- **IPC**:`window.create` → `NewWindow`;`workspace.switch` 支持 `all_windows:true` 全进程 + 跨实例 fan-out
- **注册表**:`~/.local/share/pureterm/window-sockets.txt` 记录各实例 GUI socket
- 实现:`crates/pureterm/src/multi_window_ipc.rs` + `iced_app.rs` + `ipc/methods.rs`
- **未做**:窗口间 drag-and-drop Tab / 布局快照多窗口恢复

### GAP-3.2 可编程 Sidebar DSL MVP（2026-06-29）
- **脚本**:`~/.config/pureterm/sidebars/*.toml` — `subscribe` + `template` + `icon`
- **解释器**:订阅 EventBus `Message::Event`,匹配 `shell.*` / `git.*` 等,渲染到左侧栏 Events 区
- 示例:`subscribe = ["shell.command_finished"]` + `template = "{command}"`
- 实现:`crates/pureterm/src/sidebar_dsl.rs` + `sidebar/left.rs` + `iced_app.rs`
- **未做**:Rhai 沙箱 / 渲染回调动作 / 权限隔离

### GAP-3.5 内置 Browser 面板 / WebView(2026-06-25, Review ✅ 2026-07-01 含 GAP-7.8)
- Pane 的第二种内容载体:自实现 wry 嵌入(macOS WKWebView 作 content NSView 子视图 `build_as_child`);几何真值=split_tree layout,reposition pass 按 `mark_layout_dirty` + tab 切换 + 拖拽 burst 触发;快照 `PaneSnapshot.kind="web"` + `#[serde(default)] url` 前向兼容
- **快捷键**:`⌘⇧L` 分出新 Web pane 并聚焦地址栏;`⌘L` 地址栏 overlay(导航活跃 Web pane,无则新建)+ `normalize_url_input`(裸域名→`https://` / scheme 原样 / 含空格→Google 搜索)
- Phase C per-view SOCKS5 proxy 绑定(wry `mac-proxy`,绑定 GAP-5.2 隧道);GAP-7.8 `browser.*` ops + MCP + screenshot 复用此 pane
- 命令行:`pureterm pane split-web --url <url>`;IPC `pane.split_web`
- 自动 E2E harness 全绿:`browser_address_bar_e2e`(spawn 序列 Arc 身份 + load_url 未 realize 即更新)+ `normalize_url_input` + keymap `⌘L`/`⌘⇧L`(双形),共 442 单测
- Linux 走 open-in-browser 兜底(webkit2gtk 原生嵌入未做);后退/前进导航 defer(`⌘[`/`⌘]` 与 pane 焦点冲突)
- 实现:`crates/pureterm/src/browser/` + `iced_app.rs`(`spawn_web_pane` / 地址栏 overlay);pixel smoke 见 `docs/reviewed/GAP-3.5-browser-panel/test-plan.md`
- **Review ✅（2026-07-01, GAP-7.8 Agent-Browser）**:执行核心稳健(per-op oneshot + Fn-Mutex 守护 + timeout backstop + 双层防双回复),缺陷集中在 GUI 粘合层与两个未归一化入口。修 3 处:① **[HIGH]** `bind_browser_tunnel` 收非 `connected` 隧道(stopped/failed/connecting/need_auth 仍报 `local_addr` 但无监听)→ 死代理重建 + `navigate` 假成功(同 GAP-02/3.5 fake-success 类);抽纯函数 `bind_tunnel_verdict(status)` 仅放行 `connected`,Reject 回错 + 发 `browser.bind_tunnel.rejected`。② **[MED-HIGH]** `browser navigate`/`pane.split_web` 绕过 `normalize_url_input`(裸 `localhost:8080` WKWebView 加载失败);抽 `browser::url::loadable_url()->Option` 共享核心,agent 路径**严格**(非 URL 报错,不静默搜索回退),地址栏行为不变。③ **[LOW]** 7 处 `reply_tx.lock().unwrap()`(泛 IPC + browser/surface resolver + 两 tunnel op)毒锁 panic 风险 → `ReplyTx` 别名 + `take_reply_sender`(`unwrap_or_else into_inner`)统一。+ screenshot stale doc 订正 + spec 计数订正(8 async+screenshot)。新增 7 单测(url 5 + navigate normalize + bind verdict);26 browser + 12 normalize + 55 ipc + 41 cli 全绿 0 回归,release + dist 重打包 37M。**Follow-up 未改**:多窗口父视图精确匹配(绑 GAP-25)/daemon 侧停隧道的逻辑代理泄漏(需状态监听)/IPC socket 级超时(跨切面)。

### GAP-3.7 完整快捷键自定义 + 模板(2026-06-12)
- Action 30 变体 + Keymap HashMap + TOML 解析 + CLI `pureterm shortcuts`
- 6 套预设模板:vim / tmux / iterm / vscode / agent-triage / custom
- 热重载:存盘即生效;Chord 多键序列数据结构已预留(`chord_bindings` + `ResolveResult::ChordPending`)
- 实现:`crates/pureterm/src/keymap.rs` + `iced_app.rs`(dispatch_action 统一派发) + CLI `shortcuts` + `resources/presets/`
- 单测:`keymap::tests` 22 个

### GAP-3.8 搜索体系(2026-06-21)
- `⌘F` 当前 surface 内搜索(正则 / case sensitive / whole word / 结果高亮跳转)
- `⌘⇧F` 在工作目录内 ripgrep 子进程搜索(tokio::process::Command)
- 全局跨 surface 搜索延后(需多 pane 扇出)
- 实现:搜索浮层 UI + flag(case/regex/whole-word)
- 196 测试绿 + dist .app 重打包;待手动 GUI E2E

### GAP-3.9 设置界面(2026-06-08)
- 基于 Iced 0.14 的图形化设置界面,作为独立进程运行
- **快捷键**:`Cmd+,`(macOS)打开设置窗口
- **架构**:独立进程 `pureterm-settings`,通过配置文件通信(写文件 → ConfigMonitor 自动热重载)
- **主题**:Halloy 风格暖色调暗色(Ferra 配色方向)
- **页面**:General / Appearance / Colors / Terminal / SSH Profiles / Keyboard Shortcuts
- **配置 I/O**:使用 `toml_edit::DocumentMut` 格式保留读写,`NamedTempFile::persist()` 原子写入
- 实现:`crates/pureterm-settings/`(独立 crate)

### GAP-32 Git 集成(2026-06-27)
- 在 GAP-24 Enhanced Status Bar 的 porcelain 管线上增量扩展,零新依赖:`git status --porcelain=v2 --branch` 一条子进程出 branch + dirty 分计数(staged/unstaged/untracked/unmerged/renamed)+ ahead/behind + HEAD oid + upstream;PR 走 `gh pr view --json` shell-out(复用用户 `gh auth`,不做内置 OAuth),挂 `GitStatus.pr` 单一源(状态栏 pill + 面板共用);`git` 缺失/非 repo 回退纯 FS `read_git_branch`(行为不回退)
- **状态栏 git 段**:`git_bar_label` 出 ` main ↑2↓1 ●2 ~1 +3`(零计数段省略)+ amber in-progress op 后缀 + 状态色 PR pill `⧉#N STATE (✓s · …p · ✗f)`
- **⌘⇧B Git 面板**(避让 ⌘⇧G=SearchPrev):只读 repo 视图(分支/upstream/ahead-behind/HEAD/分计数/recent commits/in-progress op/PR+checks)+ 快捷操作按钮 Stage all / Commit / Push / Pull / **Create PR** / Lazygit,命令写进活跃 pane PTY(`run_git_quick_action` → `write_to_pty`);Create-PR=`gh pr create`(in-pane 编辑器写 title/body),gh 缺失 toast;lazygit 本地起 TUI、SSH pane/未装 toast 降级;Toast 层(3.5s 自动消失 + 4s StatusBarTick 安全网)
- **Tab 标题可选显示分支**:`[git].tab_title_branch`(默认关,opt-in)开后在**派生标题**回退路径追加 ` [<branch>]`(OSC 程序设的标题 GAP-1.3 仍最高优先,不被覆盖);`tab_title_with_branch` 纯函数 + `read_git_branch` 纯 FS 取分支
- **EventBus `git.*` 发射**(GAP-6.7 集成):新增 `Category::Git`,在**有意义转移点**(非每 tick)发 `git.branch_changed`(按 pane 去重 `last_git_branch` + `git_branch_changed` 纯函数)/ `git.pr_updated` / `git.panel_opened`/`panel_closed` / `git.quick_action`;`pureterm events tail --type git` 可消费
- **配置**:`[git]` 段(`panel_enabled` / `show_pr` / `pr_refresh_ticks` 默认 30 / `lazygit_cmd` / `tab_title_branch`);PR 刷新每 `pr_refresh_ticks` tick 一次防抖
- 实现:`crates/pureterm/src/{status_bar/git.rs,gh_pr.rs,git_panel.rs,events.rs}` + `iced_app.rs`(面板 view + 动作执行 + emit 点)+ `app_config.rs`(`GitConfig`/`load_git`)+ `keymap.rs`(⌘⇧B)
- 单测:574 pureterm 全绿(porcelain v2 解析 19 + `parse_head` 3 + `gh_pr` 3 + `git_bar_label` 4 + `git_quick_action_command`/`as_slug`/`git_branch_changed` 6 + `tab_title_with_branch` 3 + events `Category::Git`/`type_filter_git` 2)
- pixel smoke:`docs/done/GAP-32-git-integration/test-plan.md`(非 CI gate,6 场景:状态栏 / 面板 / Create-PR / lazygit / tab 分支 / events tail --type git)
- **未做(独立后续)**:`git2` crate/直接 GitHub API(有意弃用,shell-out 零依赖复用 `gh auth`)/ `Cmd+Shift+G` lazygit 热键(已绑 SearchPrev)/ `events.subscribe` 流式(被 GAP-6.7 阻塞)

---

## C. 终端交互

### GAP-4.1 Vim 复制模式(2026-06-12)
- `Ctrl+Shift+Space` 进入,hjkl 导航,w/b/e 按词,0/$ 行首尾,g/G 屏顶底,Ctrl+u/d 半页
- v/V/Ctrl+v 选区(字符/行/矩形),y 复制,yy 复制行,/ ? 搜索,n/N 遍历
- GPU 渲染:青色 vi cursor + 搜索匹配 amber 高亮;状态栏 -- COPY -- / -- VISUAL -- 指示
- 实现:`crates/pureterm/src/vi_mode_handler.rs`(ViSubMode 状态机 + 18 ViMotion + visual 选区 + yank + 搜索) + `terminal_shader.rs`(vi cursor 渲染 + 搜索高亮) + `iced_app.rs`(键盘拦截 + 搜索栏 UI + 状态栏指示)
- 单测:`vi_mode_handler::tests` 3 + Alacritty vi_mode 19

### GAP-4.2 智能选择悬停预览(2026-06-29)
- 鼠标悬停 URL/SSH URL/文件路径(智能选择规则)200ms 后显示 pinned tooltip:URL 异步抓取 `<title>` + favicon;路径 `std::fs::metadata` + `ignore` gitignore 标记
- 右键语义菜单:打开 / 复制 / 在编辑器中打开(路径 + `path:line`);⌘⇧O 打开悬停或选区 URL/路径
- **GAP-36 Semantic History**:⌘-click `file.rs:42` / rustc `src/main.rs:42:5` → 编辑器(`$EDITOR` > `[semantic_history].editor` > `code`);`OpenSemanticTarget` 动作
- 实现:`crates/pureterm/src/hover_preview.rs`(纯逻辑 + reqwest 抓取)+ `iced_app.rs`(PaneCursorMoved 防抖 / tooltip+菜单 / ⌘-click / 动作)+ `keymap.rs`(⌘⇧O)
- 单测:`hover_preview::tests` 11

### GAP-33 Port Scanner（监听端口侧边栏）(2026-06-29)
- 右侧边栏 **Ports** 标签:`lsof -nP -iTCP -sTCP:LISTEN` 周期扫描(默认 5s),列表显示端口 / 进程名 / PID
- 点击行复制 `localhost:PORT`;↗ 在浏览器打开 `http://localhost:PORT`;`min_port` 过滤系统端口(默认 ≥1024)
- 实现:`crates/pureterm/src/port_scanner.rs` + `sidebar/right.rs` + `iced_app.rs` subscription
- 配置:`[port_scanner]` `enabled` / `interval` / `min_port`
- 单测:`port_scanner::tests` 4 + `app_config::port_scanner_parses_fields`

### GAP-4.3 Tail-Find 自动滚动搜索(2026-06-21)
- follow_tail:开启时若在底部自动 arm;新输出重锚(LayoutSaveTick 250ms 检测 bottommost 增长)
- 用户滚动取消(两 scroll handler);toggle 支持
- rotation-safe vi cursor 复用为锚
- 196 测试绿 + dist .app 重打包;待手动 GUI E2E

### GAP-1.3 标题栈 / OSC 动态标题(CSI 22t/23t + OSC 0/2)(2026-06-25)
- 程序设标题(OSC 0/2 `\e]0;title\a`)与压弹栈(CSI 22t push / 23t pop)真实反映到窗口标题 + Tab 标题(此前标题事件在事件桥被丢弃,程序设标题无任何可见效果)
- 显示策略:OSC 动态标题优先;空 / `None` / `ResetTitle` 回退 PureTerm 派生标题(`#1` / `🔒 SSH: <name>`),与主流终端行为一致
- 核心栈复用上游 alacritty_terminal(`Term::{push,pop,set}_title` + `title_stack`,深度上限 4096;vte 已接好 `22t→push_title` / `23t→pop_title` / `OSC 0,2→set_title`),不改 vendored 源码
- 实现:`terminal_context.rs::ChannelEventListener` 转发 `Event::Title`/`ResetTitle`(此前 `_ => return` 丢弃)+ `iced_app.rs` `pane_osc_title` side-map(仿 `pane_cwds`,不改 `Pane` 结构体)+ 事件循环两 arm + `resolve_display_title` 纯函数 + `tab_display_title` 接线(窗口标题 / Tab 栏 / Tab 切换器 / AppSnapshot)
- 单测:桥接转发 3 + `resolve_display_title` 3;全 461 绿

### GAP-15 Shell 集成（OSC 133 A/B/C/D）(2026-06-28)
- zsh + bash 自动注入 OSC 133 prompt/command/output 边界标记 + 复用既有 OSC 7 cwd;命令文本由 shell 经 B mark 的 `cmd=<hex>` 发送(非 grid 读取),GUI hex 解码进 `ShellMark.command`
- **注入**:`pureterm-daemon/src/shell_integration.rs` `prepare_shell_env(tag, opts)` — zsh `.zshenv` bootstrap(precmd/preexec/chpwd) + bash `PROMPT_COMMAND`(DEBUG trap);`PURETERM_SI_PID` 哨兵防同进程重复注入
- **解析**:`alacritty_terminal/event_loop.rs` `scan_osc133()` + `Event::ShellMark`;`terminal_context.rs` 桥接 `TerminalEvent::ShellMark`(此前 `_ => return` 丢弃)
- **状态机**:`shell_state.rs` per-pane A/B/C/D → `FinishedCommand`(exit/duration/text,64 条历史);`iced_app.rs` `pane_shell_state` + `shell.{prompt_started,command_started,output_started,command_finished}` EventBus emit
- **AI 上下文**:`shell_context.rs` + IPC `shell.context` + MCP `get_shell_context`(cwd / last_command / recent_commands;SSH pane → `integrated:false`)
- **配置**:`[shell_integration]`(`enabled`/`inject_zsh`/`inject_bash`/`include_command_text`);门控仅 GUI 直接 PTY 回退路径 — daemon 路径始终 `ShellIntegrationOpts::all_on()`(与 OSC 7 always-on 一致)
- 单测:shell_state/shell_context/events/app_config 21 绿 + daemon `shell_integration` 单测
- **DoD 待人工**:GUI 真机 E2E(zsh/bash `events tail --type shell` + MCP `get_shell_context`);**未做(独立后续)**:视觉 grid marks、fish/nu、click-to-SCP
- **Review ✅（2026-06-29）**:审计正确(scanner/事件桥/状态机/IPC/MCP 全对;IPC 每请求重建 live snapshot 无陈旧 bug)。修 4 处:① **[MED]** 两处 pane 关闭点漏清 `pane_shell_state`(GAP-15)+`pane_block_stores`(GAP-51) → pane id 单调不复用致无界内存泄漏,两处补齐(沿用既有 disjoint-field 借用,`tab` 借 `self.windows` 期间不能用 `&mut self` 方法);② **[LOW]** temp ZDOTDIR 默认 0755/0644 含真实 `$HOME` → dir `0700`/file `0600`(`DirBuilderExt`/`OpenOptionsExt`,0700 顺带堵 /tmp 可猜路径预填竞态;生命周期清理留后续);③ **[LOW/doc]** zsh 注释误称 `EPOCHREALTIME` 小数是纳秒(实为微秒 6 位),代码本对,订正注释;④ **[LOW/limit]** bash 集成覆写用户 `PROMPT_COMMAND`(OSC 7 旧实现会保留)→ 记为已知局限不改。+ 新增 `temp_zdotdir_is_owner_only` 单测;745 pureterm + daemon `shell_integration` 6 绿(排 instance_lock + 真机 PTY 环境性失败)。

### GAP-35 LSP 客户端（2026-06-29）
- **Crate**:`pureterm-lsp` JSON-RPC stdio;`hover` + `completion` + `publishDiagnostics` 方法
- **配置**:`[lsp] enabled` + `[lsp.servers]` rust/go/python/typescript/cpp 默认命令
- **集成**:状态栏 `lsp` 段 + AI Chat 诊断上下文 + semantic open `didOpen`
- 单测:`pureterm-lsp` 2 + `lsp` 2 + `app_config` lsp 2

### GAP-51 基于块的输出（2026-06-29）
- **配置**:`[block_output] enabled`(默认 `false`)
- **数据模型**:`BlockSumTree` + `output_text` grid 切片;每 pane 256 条
- **GUI**:右侧边栏 **Blocks** 标签 — 过滤/搜索/跳转
- 单测:`output_blocks` 3

### GAP-43 远程文件浏览（2026-06-29）
- **RPC 方法**:`remote.listdir` / `read` / **`write`** / **`find`**
- **GUI**:Files 侧栏 — 浏览/搜索/预览/保存
- 单测:daemon `remote_fs` 6 + `control` remote.* 6 + `remote_files` 7

### GAP-09 通知系统（OSC 9/99/777 + Bell）(2026-06-28)
- 解析 OSC 9/99/777 + Bell(0x07) →  per-pane 关注信号:amber attention ring + 状态栏 🔔 未读计数 + ⌘⇧M 通知历史面板 + macOS Dock 未读角标 + EventBus `notification.*`
- **解析**:`alacritty_terminal/event_loop.rs` `scan_osc_notify()` + `event.rs` `Notification`/`NotifySource`/`NotifyOccasion`;`terminal_context.rs` 桥接 `TerminalEvent::Notification`/`Bell`
- **Store**:`notification_store.rs` 内存真源(历史环/unread/attention/bell 节流);EventBus `events.jsonl` 仅审计(`notification.posted` / `panel_opened` / `panel_closed` / `all_dismissed`)
- **GUI**:`iced_app.rs` 处理 Notification/Bell → store + `refresh_dock_badge`;聚焦 pane 清 unread;`view_pane` amber ring;`view_notifications_panel` 历史列表 + 点击聚焦源 pane
- **配置**:`[notification]`(`enabled`/`desktop_notifications`/`dock_badge`/`ring`/`bell_passthrough`/`bell_throttle_ms`);`default-config.toml` 示例块
- **macOS**:`macos_dock_badge.rs` `NSDockTile.setBadgeLabel`;`macos_notifications.rs` UNUserNotificationCenter shim(request `.alert|.sound|.badge` + `post()` 调 `UNNotificationRequest`);启动 + 热重载时 `request_authorization()`

### GAP-21 Dock 角标收尾（macOS）(2026-06-28)
- Dock 角标 = 通知 unread + Feed 待审批(`feed_panel.pending_count()`);`dock_badge_total()` + `refresh_dock_badge_from_state()`;Feed resolve/timeout/inbound 后同步刷新
- **Dock 菜单**:`macos_dock_menu.rs` `applicationDockMenu:` 显示 MRU 最近 10 个 workspace;选择 → `ActivateWorkspaceById`;`recent_workspace_ids` 在 workspace 切换/创建时更新
- 单测:`dock_badge_total` 3 + `touch_recent_workspace_mru_order` 1
- **桌面 toast 门控**:`should_toast(occasion, pane_visible, app_focused)` 纯函数 — `Always`/`Invisible`/Bell 仅在 pane 不可见时弹;`Unfocused` 仅在 app 失焦时弹;`WindowFocused`/`WindowUnfocused` 翻 `app_focused`
- **CLI/IPC**:`pureterm notify "msg" [-t title] [--pane-id N]` → `notification.post` RPC → `IpcSideEffect::ShowNotification`(source `Cli`)→ store + 角标 + 门控 toast
- 单测:scanner 17 + notification_store 9 + app_config 3 + bridge 2 + `should_toast` 2;GUI E2E 见 `docs/reviewed/GAP-09-notification/test-plan.md`(非阻塞,待人工)
- **已知局限**:Linux dbus 通知未做(用户决定 macOS-only);`FocusNotificationPane` v1 仅清 unread,跨 tab 聚焦待补

### GAP-10 Triggers（模式匹配自动化 v1+v2）(2026-06-28)
- `[[triggers]]` 正则匹配终端**输出**文本 → 七动作:`highlight`(GPU overlay) / `notify`(toast + EventBus) / `send_text`(写 PTY) / `exec`(argv 子进程,无 shell) / `set_variable` / `set_mark` / `fold`(conceal 折叠渲染)
- **模板**:`{match}`、`$1`–`$9`、`${var:NAME}`、`\n\r\t\xNN` — notify/send_text/exec/set_variable 派发前 expand
- **配置**:`app_config.rs` `load_triggers()` — 容错跳过坏正则/缺字段;热重载 `ConfigReloaded`
- **扫描**:`triggers.rs` `scan_lines`(captures) + `next_scan_window` per-pane delta;`iced_app.rs` `TerminalEvent::Wakeup` 内联(首次 wake 只 seed 不回放历史);列号用字符数非字节
- **Highlight/Fold**:`pane_trigger_state` → 共享 `Arc<Mutex>` → `terminal_shader.rs` overlay + fold conceal
- **EventBus**:`Category::Trigger` + `trigger.matched`(含 captures) + `trigger.variable_set`(脱敏)
- **状态栏**:`trigger_count` 组件 id（`⚡ N`，需手动加入 components）
- **MCP/IPC**:`trigger.list` / MCP `trigger_list`(只读;**不**返回密码/变量值)
- 单测:triggers + load_triggers + `trigger_list` 脱敏;全量绿;GUI E2E 待人工

---

## D. SSH 远程

### GAP-5.1 持久 PTY 守护进程(2026-06-17)
- 全里程碑 M1-M5 闭环:本地 + SSH PTY 均由独立 `puretermd` 守护进程拥有
- 本地 `tty::new`,SSH 用共享 `pureterm-ssh` crate 的 `SshPty::spawn` / `SshCommandBuilder` / `SshSessionManager`
- daemon `PumpablePty` trait 统一 pump;GUI `DaemonPty: EventedPty` 包 socketpair,`PtyEventLoop` 不动
- `pureterm ssh connect [user@]host` 真 interactive client
- 滚动回放 1 MiB ring → SCROLLBACK;登录自启动(macOS LaunchAgent / Linux systemd)
- 多 attach 扇出(Vec<Subscriber> 广播) + 有界背压(drop >2MiB 滞后客户端) + control/data socket 0600
- 实现:`crates/pureterm-daemon/`(daemon,含 ring_buffer / PumpablePty / SessionManager / autostart / discovery) + `crates/pureterm-ssh/`(共享 SSH spawn) + `crates/pureterm/src/{daemon_pty.rs,daemon_client.rs,daemon_cli.rs,ssh_cli.rs}` + `terminal_context.rs` + `build.sh`

### GAP-5.7 SSH Tab UI 串通(2026-06-08)
- `⌘⇧S` 弹 SSH Profile 选择对话框 → 异步 spawn → PtyEventLoop 集成 → 标题显示 `🔒 SSH: <name>`
- graceful_close:发送 `exit\r` → 等 500ms → SIGHUP 兜底
- 实现:`crates/pureterm/src/ssh/` + iced UI 串通

### GAP-5.8 SSH 主 UI 设置 UI(2026-06-08)
- 右侧栏 `SshSettingsPanelState` + `CredentialStore` 主密码端到端
- Profile 列表 + 实时编辑 + 保存到 toml
- 加密凭证存储:AES-256-GCM + Argon2id,`Zeroizing` 包裹 master_key

### GAP-5.9 SSH 命令面板集成(2026-06-08)
- `Mode::Profiles` 一跳到底:命令面板搜 `ssh` → 切到 Profiles 模式 → 选 Profile 直接连接

### GAP-5.6 SSH reconnect/disconnect 状态机(2026-06-14/2026-06-17)
- **进程内子集(2026-06-14)**:`SshState` 状态机 + `ReconnectHub` std::thread worker + tokio mpsc + 持久 iced Subscription + 按 `ConnectionKey` 防重连风暴
- `terminal_context.rs::SshCtx::reattach()`:同 terminal `Arc` 重建 `PtyEventLoop`,保留滚动历史
- 指数退避 3→60s × 8,Condvar 可唤醒;退出按**用户意图**分类(`user_initiated_close` + `>5s` grace)
- 状态栏指示:`SSH` / `SSH ↻ n` / `SSH ✗` / `SSH ⏻`
- `ssh.disconnect` IPC 真实落地:置 `Disconnected` 终态 + emit `ssh.state_changed → disconnected`(Vanish:随后 teardown 移除 pane);`pureterm ssh disconnect [--pane-id N]`(无 `--pane-id` 断开活跃 pane)
- **daemon 化跨重启恢复(2026-06-17)**:GUI 重启后自动重连 daemon 存活会话 + 重建 tabs/分屏布局
- `session_restore.rs`:`LayoutSnapshot` 持久化 → `data_dir()/gui-sessions.json`,`NamedTempFile::persist` 原子写,变更去抖 `time::every(250ms)` + ≥500ms 空闲落盘 + 关窗 flush
- `iced_app::new()` 启动 `load → session.list 校验存活 → attach_existing 重连 → scrollback 回放重建 grid`
- 孤儿会话(daemon 有/快照无)作额外 tab 接回;窗口尺寸按上次快照重开
- `SplitNode` serde + `remap_pane_ids`/`regenerate_split_ids`(进程本地 id 重映射);daemon 零改动
- SSH 重连到存活 `ssh -tt` child 无需重新认证
- **关闭语义**:`⌘W` 关 tab/pane → `kill_pane_session` 结束该 daemon 会话(下次不再恢复);退出 app → 不 kill,保留存活会话供下次恢复
- **4 项收尾代码闭环(2026-06-25 ✅，GUI 真机 E2E 后补)**:
  - `Authenticating` 独立状态:利用 `spawn_pty` fork 后即返回、认证在 ssh 子进程异步的事实 —— `ReconnectUpdate::Spawned`(替代 `Success`)→ reattach → `SshState::Authenticating`,首个远端字节(`TerminalEvent::Wakeup`)→ `Connected`(修正原先 spawn Ok 即 `Connected` 的过早判定)。`SshFailureKind`(Auth/Network/HostKey/Unknown)+ `classify_ssh_failure(stderr)` → 带类型 `Failed{kind}` + 分类后对话框
  - Socket API:`ssh.state_changed` 事件(复用 `Category::Session`,5 切换点 emit)+ 只读 `ssh.state` RPC(`AppSnapshot::ssh_pane_states: Vec<SshPaneState>`)+ `pureterm ssh state` CLI
  - 多 pane tab SSH 退出重连:撤销 `pane_count<=1` 门控,遍历本 tick 全部 `exited_panes`,SSH 非用户发起退出任意 pane_count 都 `trigger_ssh_reconnect`(保留 pane 在 `split_tree`),不再被 `PtyExitedPane` 移除/取消
  - 重连成功分隔线:`TerminalContext::inject_reconnect_marker`(`ansi::Processor::advance` 向 alacritty `Term` 网格注入持久 `── Reconnected to <host> ──`,scrollback 内旧/新会话分界;同 `surface_snapshot.rs:121` 的 VT 注入模式)
- **disconnect 半边 + 初始连接收尾(2026-06-25 第二轮 ✅)**:
  - disconnect 真状态机:`Fx::SshDisconnect` 置 `SshState::Disconnected` + emit `ssh.state_changed → disconnected`(Vanish 语义:状态机/事件流忠实反映,随后走现有 teardown 移除 pane;重连被 `user_initiated_close` 标志抑制)。此前 `Disconnected` 是运行时死代码(定义/快照/slug/颜色/单测齐全但从未赋值)、`ssh disconnect` 无 `--pane-id`、断开不发事件且不可见于 `ssh.state`
  - 初始连接走 `Connecting`(此前直跳 `Connected`):`push_ssh_tab` emit `→ connecting`,Wakeup handler 经纯函数 `on_first_remote_byte(prior)→(flip,inject_separator)` 决策——`Authenticating`(重连)盖分隔线、`Connecting`(初始连接)不盖(变体本身区分,无需新字段)
  - 455 harness 绿(+4 新单测:`on_first_remote_byte`×3 + `ssh_disconnect_with_pane_id`)
- 实现:`crates/pureterm/src/ssh/reconnect.rs` + `session_restore.rs` + `terminal_context.rs` + `iced_app.rs`
- **Review ✅（2026-06-30）**:状态机本身正确(六态转移 + `on_first_remote_byte` 纯函数决策 + flap 检测器逐行核实无误;既 flag 的「per-spawn backoff 不升级 → hammer host」已在 5d319b4 用窗口式 flap 检测器修掉)。审计出 **3 个重连 worker 生命周期 bug**(都是「worker 该停没停/停晚了」):① **[MED-HIGH]** `Fx::SshDisconnect` 不 cancel 在飞 worker → 重连中用户断开会被 worker 成功后 reattach「复活」,或 worker 跑完发 `Failed` 覆盖 `Disconnected`(语义错);worker 只认 `cancel`、不读 `user_initiated_close`/状态 → 补 `reconnect_hub.cancel(pid)`。② **[MED]** `close_active_pane`(⌘W 非末 pane)缺 `mark_user_initiated_close`+`cancel` → 关掉重连中的 pane 后 worker 残留最长 ~225s 反复 spawn/abort ssh + 泄漏 slot/flap_history;补齐(对齐 `PtyExitedPane` 8104-8117 路径)。`detach_active_pane` 经核实是 **move** 非 destroy(pane 续活新 tab/window,doc 明示不 release pool)→ 正确地不 cancel。③ **[MED-LOW]** worker 末次失败仍睡满 60s 才报 `Failed`,且 `Progress` 谎报「将在 60s 后重试」→ 抽 `final_attempt()` 纯缝 + `Err` 分支末次 `break`(跳过 sleep + 误导性 Progress)。+ 新增 `final_attempt_is_last_backoff_index_and_gives_up_immediately` 单测(TDD red→green);746 pureterm 单测全绿 0 回归,release 编译过。**更广 follow-up(未改)**:`CloseOtherTabs`/`CloseTabsToLeft` 整段绕过 SSH 清理(无 cancel/release/kill),建议后续抽 `teardown_tab_ssh_panes(tab_id)` helper 统一所有销毁路径。

### GAP-5.4 SSH Agent 转发(2026-06-25)
- `ssh-add` 后远程免密:`SshProfile.forward_agent = true` / CLI `-A` → daemon `ssh -tt … -o ForwardAgent=yes` 子进程 + 显式注入 `SSH_AUTH_SOCK`
- **agent 套接字解析器**(`pureterm-ssh::agent_sock`,两轨共用):优先级 配置覆盖 > 继承 `$SSH_AUTH_SOCK` > macOS `launchctl getenv` 回退(GUI 启动常缺 shell env);`resolve_pure` 纯函数 7 单测
- **双轨**:Track A(PTY `ssh`,闭 DoD)+ Track B(daemon russh 隧道:`Channel::agent_forward` + 重写 `server_channel_open_agent_forward` 做 Unix socket 字节泵 + `authenticate_publickey_with` agent 免密认证)
- 默认关(不可信主机 agent 被盗用风险,与 OpenSSH 一致),profile / `-A` 显式开
- **DoD 真机 E2E**(本地 sshd,可复现):跑 PureTerm `build()` 原样 argv(`-o ExitOnForwardFailure=yes -o ForwardAgent=yes`)→ 远端 `ssh-add -l` 见转发 key(指纹一致)、ssh exit 0(`ExitOnForwardFailure` 覆盖 agent 转发,0=服务器已接受);对照 `ForwardAgent=no` 远端无 `SSH_AUTH_SOCK`。442(harness)+ 21(pureterm-ssh)绿
- 实现:`crates/pureterm-ssh/src/{agent_sock,command,pty,session}.rs` + `crates/pureterm-daemon/src/{tunnel,session,control}.rs` + `crates/pureterm/src/{ssh/config,daemon_client,ssh_cli,tunnel_cli,cli}.rs`
- Track B(daemon russh)真机 E2E 待补(需重启 puretermd 验新 daemon 的 `-A`;非 DoD,与 GAP-5.2 同属待真机网关验证)

### GAP-40 SSH 端口转发 -L/-R/-X MVP(2026-06-29)
- **`SshForwards`**(`pureterm-ssh::command`):`-L`/`-R` 字符串列表 + `-X`/`-Y` → `SshCommandBuilder` 拼进 `ssh -tt` argv（动态 `-D` 仍走 GAP-5.2 `ssh tunnel`）
- **Profile**:`[[ssh.profiles]]` 增 `local_forwards` / `remote_forwards` / `forward_x11` / `forward_x11_trusted`;GUI 连接对话框列表行显示 `[L2 R1 X]` 摘要;状态栏 `SSH L1` 等
- **CLI**:`pureterm ssh connect … -L 8080:localhost:80 -R … -X/-Y`（daemon-direct,与 `-A` 同路径）
- **Daemon**: `session.create` 透传 forward 字段 → `create_ssh` → 共享 `SshCommandBuilder`
- MVP 范围:无独立「转发管理面板」/运行时 `-O cancel` UI;SOCKS 仍 GAP-5.2

### GAP-5.3 最小屏幕优先 PTY 尺寸调整(2026-06-29)
- 多 pane 共享同一 daemon session 时,PTY `TIOCSWINSZ` = **min(cols)×min(rows)** across 全部 subscriber(GUI attach)
- **`[ssh] smallest_screen_wins = true`**(默认开);GUI resize 路径同步按 session_id 聚合 min 再 `TerminalContext::resize`
- 实现:`pureterm-daemon/src/session.rs` pump 跟踪 per-subscriber 尺寸;`iced_app.rs` `compute_session_min_sizes` + `resize_workspace_panes`

### GAP-5.2 SOCKS5 隧道(2026-06-27)
- daemon `puretermd` 持有 russh 0.60 SSH 会话 + 手写 SOCKS5 listener（`atyp=3` 域名透传 → 远端解析 DNS，`curl --socks5-hostname` 即用），三路认证（identity/agent/password→need_auth），30s keepalive（R6 死网关检测）
- `tunnel.{start,list,stop,provide_auth}` JSON-RPC + `ssh tunnel/tunnel-stop/tunnel-list` CLI（daemon-direct）；与 PTY SSH 双轨（不复用 ControlMaster socket）
- **TOFU host-key**（`known_hosts.rs`）：隔离文件 `data_dir()/known_hosts`（**不**动用户 `~/.ssh/known_hosts`），首连记录 / 复连 Match / 换 key 严格拒绝。`ClientHandler::check_server_key` 经纯函数 `decide_host_key` 决策；Mismatch 经既有 `Failed` 状态 + `tunnel.list` 浮现（无需新 RPC）。OpenSSH 行格式 `[host]:port algo blob`，仅字面 host 匹配（wildcard/hashed 非 MVP）
- **持久化**（`persist.rs`）：daemon 自有 `data_dir()/tunnels.json`（不进 GUI 的 `workspaces.json` 全量覆写、不进 `pureterm.toml` 配置）。`TunnelConfig` **不存密码**（对齐 SSH-profile）；`start`/`stop` 写入点 + 启动**仅恢复配置**（注册为 `stopped`，用户手动重连；不自动重连以避免 hammering + 不泄漏密码）。原子写 `NamedTempFile::persist`
- agent 转发复用 GAP-5.4 Track B（隧道路径本身），无需新增
- flip 标准：代码闭环 + `cargo test -p pureterm-daemon` 单测绿（known_hosts 7 + persist 6 + tunnel host_key 4 + 既有，共 69 绿；`instance_lock_is_exclusive` 在 puretermd 运行时环境性失败、2 PTY 集成测试需真机）；真实网关 E2E 见 `docs/reviewed/GAP-5.2-socks5-tunnel/test-plan.md`，**非阻塞**人工 follow-up（GAP-5.6/5.4 先例）
- 实现:`crates/pureterm-daemon/src/{tunnel,known_hosts,persist,control,runtime,discovery}.rs` + `crates/pureterm/src/tunnel_cli.rs`

### SSH Profile 配置示例(alacritty.toml)

```toml
[[ssh.profiles]]
name = "生产服务器"
host = "192.168.1.100"
user = "admin"
port = 22

[[ssh.profiles]]
name = "跳板机"
host = "bastion"
key_path = "~/.ssh/bastion_key"
forward_agent = true   # GAP-5.4: 远程免密 git push(默认 false,仅在可信主机开)
```

### SSH Action 键盘绑定示例

```toml
[[keyboard.bindings]]
key = "F5"
mods = "Control|Shift"
action = { Ssh = "OpenSettings" }

[[keyboard.bindings]]
key = "F6"
mods = "Control|Shift"
action = { Ssh = "ListProfiles" }
```

可用的 SSH Action:`Ssh = "Connect"` / `Ssh = "OpenSettings"` / `Ssh = "ListProfiles"`

---

## E. 集成与可编程

### GAP-6.1 Socket API v2 完整协议(2026-06-13)
- 31 method / 8 命名空间 + `IpcSideEffect` 写操作副作用机制
- 实现:`crates/pureterm/src/ipc/methods.rs`
- 单测:45 个

### GAP-6.2 CLI 协议(2026-06-13)
- clap derive 8 命令组(system / window / tab / pane / surface / config / ssh / debug) + Msg/Shortcuts
- 全局 `--socket` / `--json` flag;leaf → method 映射复用 send_rpc
- 实现:`cli.rs` + `main.rs`(clap dispatch) + `ipc/client.rs`(`send_rpc` 返回 Value 供 `--json`)
- 单测:14 个

### GAP-6.4 自定义命令注册(2026-06-15)
- `[[commands]]` 声明式配置 → keymap 绑定 + `⌘⇧P` 面板可搜
- 执行模型:触发写 `command+\r` 进当前 pane PTY(复用 `write_to_pty`),零新进程
- 键修饰符:`+` 和 `|` 都接受;shift + 字母自动注册 macOS 双形态
- 实现:`keymap.rs`(`CustomCommand` + `Action::CustomCommand` + `split_mods_and_key` + `register_custom_commands`) + `app_config.rs`(`load_custom_commands` 容错 + id 去重) + `iced_app.rs`(dispatch + `CommandEntry::Custom` 面板)
- 单测:13 个

### GAP-6.6 tmux 兼容子集(2026-06-24)
- `pureterm tmux <command>` —— tmux 兼容命令子集,映射到 Socket API v2(GAP-6.1)+ daemon
- 14 子命令 + target 解析器(`%N` / `:+` / `:-` / active / `%0` 回退到首 pane)
- **阶段 1(只读)**:capture-pane(复用 `surface.snapshot` 真实实现,`-p` 默认最近 50 行)/ list-panes / send-keys / split-window / kill-pane / select-pane
- **阶段 2(布局,RPC + IpcSideEffect)**:resize-pane(`resize_ratio`)/ swap-pane(`swap_by_ids`)/ select-pane 几何(`find_adjacent`)/ break-pane(`pane.detach` → MovePaneToNewTab)/ select-layout(→ equalize)
- **阶段 3(跨 tab)**:join-pane(`pane.attach`;`remove`+`split`+TerminalContext `Arc` 移动,PTY/daemon session 跟随、不重连)
- **阶段 4(daemon)**:pipe-pane(`pipe_pane.rs` pump→mpsc→tokio 子进程 `kill_on_drop`;GUI `surface.pipe` 转 daemon)/ wait-for(`wait_for.rs` channel/lock store;CLI 直连 daemon socket 绕 5s 超时)
- 实现:`tmux_cli.rs` + `split_tree.rs`(`resize_ratio`/`find_adjacent`/`swap_by_ids`/`detach_leaf`)+ `surface_snapshot.rs`(`capture`)+ `crates/pureterm-daemon/src/{pipe_pane,wait_for}.rs` + `ipc/methods.rs`(`pane.*` / `surface.*` RPC + `IpcSideEffect`)
- 自动 E2E harness 全绿:419 pureterm 单测 + 54 daemon 单测(含本次补的 join-pane Arc-follows 不变量[`join_pane_e2e`]/ capture() 真实栅格读 / `resize_ratio`+`find_adjacent` / pipe-pane pump→子进程字节落地 / CLI↔`SocketRouter` round-trip 三条)
- pixel smoke:`docs/reviewed/test-plan/tmux-features-smoke.md`(非 CI gate,12 命令逐条)
- 已知 phase-2 stub:`kill-pane -t` 定向、`select-pane -L/-R/-U/-D`/`-l`

### GAP-6.7 Event Stream(事件流 + 持久 JSONL 日志)(2026-06-26)
- `EventBus` 双写:每条 `AppEvent` 落 `events.jsonl`(16MiB rotation + `.1` 归档)+ broadcast(4096);12 类 category(Window/Workspace/Surface/Pane/Session/Notification/Feed/Agent/Sidebar/Browser/App/Config),~42 个 emit 点集中在 `iced_app.rs`
- CLI:`pureterm events list [--limit N] [--type cat|cat.kind]`(newest-first 回读 + dotted-kind 过滤)/ `pureterm events tail [--type ...]`(follow,rotation inode 检测重读 + 半行跨次缓冲)
- 收尾(本次 flip):① 修 workspace producer 误分类(`workspace.created`/`workspace.switched` 从 `Surface` 修正为 `Workspace`);② `tail_follow` 读路径抽出可单测的 `drain_once` + 轮询 75ms→150ms(≈ 空闲唤醒减半);③ `events.rs` doc 注释更新(Workspace/Browser 已由 GAP-2.1/3.5 产出)
- 实现:`crates/pureterm/src/{events.rs,events_bus.rs,events_log.rs}` + `iced_app.rs`(emit 点)+ `cli.rs`(`EventsSub`)
- 单测:495 pureterm 全绿(events 模块 11 个:EventBus 双写 / AppEvent JSONL round-trip / TypeFilter / read_recent / drain_once 完整行+半行+rotation / tail_follow 端到端送达 + rotation 存活)
- pixel smoke:`docs/reviewed/GAP-6.7-event-stream/test-plan.md`(非 CI gate,实时跟随 + workspace 过滤 + list 回读 + tail 延迟)
- **未做(独立后续)**:`events.subscribe` 流式 MCP 工具(需独立流式传输设计)/ sidebar producer(被 GAP-6.8 阻塞);`notify`(kqueue/inotify)替轮询经实测评估弃用 —— macOS `RecommendedWatcher` 走 FSEvents,事件 ~1s 才到(比 150ms 轮询还慢)且其 runloop 会拖住同进程其它线程的 sleep,主平台无收益反有害(详见 `events_log.rs::TAIL_FALLBACK_MS` 注释)
- **GAP-09 集成(2026-06-28)**: `Category::Notification` producer 已落地 — `notification.posted` / `panel_opened` / `panel_closed` / `all_dismissed`;desktop toast / `pureterm notify` CLI defer 至 GAP-09 收尾

---

## F. AI Agent 智能体

### GAP-01 AI Chat Panel（内置聊天面板）(2026-06-28)
- **⌘⇧L** 右侧面板（`⌘L` 保留 Browser 地址栏）→ 消息列表 + 输入框 + Send；流式 assistant 回复；Stop 取消 in-flight HTTP
- 发送时注入活跃 pane：`shell_context::build_for_pane` + `surface_snapshot::capture_visible_tail`（80 行）拼入 user turn（UI 历史仅存用户原文）
- **核心**：`ai_chat/`（`panel`/`context`/`provider`/`anthropic`/`openai`/`sse`/`types`）；`spawn_ai_chat_stream`（`Task::stream` + `ChatProvider`，同 GAP-02 模式）
- **配置**：`[ai_chat]`（`provider`/`model`/`max_tokens`/`api_key_env`/`openai_base_url`）；API key 仅 env
- **Review（2026-06-30）✅**：修 9 类 bug + 安全加固 + Markdown 接线 —— 关闭面板 inflight 残留（`abort_inflight_chat` 统一三路径）/ prompt 注入加固（`<terminal_context>` 标签 + 不可信数据 system 条款）/ 提前 EOF 合成终止 / 共享 `reqwest::Client`（GAP-01/02/08 复用）/ 历史上限 20 + SOUL 32KiB cap / `validate_base_url` 仅 https+本地 / `classify_reqwest_error` 错误脱敏 / 未知 provider 拒绝 / Stop 部分文本标记；`markdown::parse` 接 `rich_text`/`span` 渲染代码块 + 行内样式 + 自动滚动。88 ai_chat + 3 soul 绿 0 回归。GUI E2E 待人工
- **未做（defer）**：Settings UI、session 持久化 GUI、MCP function calling；Markdown 富文本进阶（语法高亮/列表/表格）

### GAP-02 Error Recovery（命令失败 LLM 修复建议）(2026-06-28)
- Shell Integration 非零 exit 自动触发（可配 `[error_recovery].auto_analyze`）→ 捕获命令 + 输出 tail → 复用 `ai_chat` Anthropic/OpenAI `ChatProvider` 流式分析 → 底部 inline 卡片展示建议 + **Apply** 写入 PTY
- **核心**:`error_recovery.rs`(`FailureContext`/`ErrorRecoveryState`/`build_prompt`/`extract_fix_command`/`should_auto_analyze`);`surface_snapshot.rs` `capture_visible_tail`/`capture_row_range`
- **GUI**:`iced_app.rs` ShellMark OutputEnd 门控 + `spawn_error_recovery_stream` + `view_error_recovery_card`;`⌘⇧E` 手动重分析最近失败命令;Dismiss/Stop 取消 in-flight 请求
- **配置**:`[error_recovery]`(`enabled`/`auto_analyze`/`provider`/`model`/`max_output_lines`/`max_tokens`/`api_key_env`/`openai_base_url`);API key 仅 env（`ANTHROPIC_API_KEY`/`OPENAI_API_KEY`）
- **EventBus**:`Category::ErrorRecovery` — 事件 `error_recovery.triggered` / `suggested` / `applied`
- 单测:error_recovery 7 + app_config 2 + events filter 1;637 pureterm 绿（`triggers::scan_multi_trigger_independent` 预存失败 unrelated）
- pixel smoke:`docs/reviewed/GAP-02-error-recovery/test-plan.md`（非 CI gate,需真实 API key）
- **DoD 待人工**:GUI E2E + Apply 验证;**未做(独立后续)**:Settings UI、SSH pane 自动分析

### GAP-04 Computer Use MVP（桌面截图 + 输入 stub）(2026-06-29, Review ✅ 2026-07-01)
- **MCP tools**：`desktop_screenshot` / `desktop_click` / `desktop_type` / `desktop_key`（`spawn_blocking` 直调 crate，不经 Socket API）
- **截图**：macOS `screencapture -x -t png` → PNG `data:image/png;base64,...`；非 macOS 返回错误
- **输入**：`click`/`type_text`/`key_press` MVP stub（返回 not-implemented 错误，占位后续 CGEvent）
- **核心**：`crates/pureterm-computer-use/` + `computer_use.rs`（base64 data URL 封装）
- **未做（defer）**：Linux/Windows 截图、真实鼠标/键盘注入（落地后须接审批门）
- **Review ✅（2026-07-01）**：审计干净 MVP，修 1 安全缺口 + 3 小 bug。① **[HIGH 安全]** 截图缺审批门（spec §安全）→ 复用 `feed_client::request_approval`，capture 前弹 Feed 面板，抽纯函数 `approval_allows_screenshot(&FeedOutcome)->bool` fail-closed（NoGui/Timeout/Error 拒绝）。② **[MED]** 临时文件泄漏（`read` 失败跳过 cleanup）→ 抽 `read_and_cleanup(path)` 全路径无条件清理。③ **[LOW-MED]** `temp_png_path` PID-only 可预测 + 非 UTF8 回退共享路径 → 报错不回退 + `PID-nanos-counter` 后缀。④ **[LOW]** `rpc_method` 误导（标 `computer_use.*` 但进程内直调无派发器）→ sentinel `"(in-process)"` + roster 测试。⑤ **[LOW doc]** 优先级 P1→P2。+ `png_bytes_to_data_url` 编码测试。**设计边界**：输入 stub / macOS-only / 无图像脱敏（门控为 MVP 缓解，follow-up）。792 pureterm + 5 computer-use 绿 0 回归。

### GAP-07 Soul System（AI 人格配置）(2026-06-29)
- **`[soul]`** 配置（`enabled`/`directory`）；默认目录 `~/.config/pureterm/ai/`
- 启动/重载时加载 `SOUL.md` / `STYLE.md` / `SKILL.md` / `MEMORY.md` → 拼入 AI Chat `build_request` system prompt
- **核心**：`soul.rs` + `app_config::SoulConfig` + `ai_chat/context.rs` soul 扩展参数
- **未做（defer）**：MEMORY.md AI 自动策展、Settings UI 编辑

### GAP-08 Natural Language Commands（`# 描述` → shell 命令）(2026-06-29)
- 终端输入行 `# find large files` + **Enter** → 清除当前行（Ctrl+A Ctrl+K）→ 复用 `[ai_chat]` LLM provider 流式生成 → 底部 inline 卡片 + **Apply** 写入 PTY（不自动执行）
- **核心**：`natural_language.rs`（同 GAP-02 状态机/`extract_fix_command`）；`surface_snapshot::capture_cursor_line`
- **配置**：`[natural_language]`（`enabled`/`max_tokens`）；provider/model 与 `[ai_chat]` 共享
- **未做（defer）**：`Cmd+#` 快捷键插入 `# `、Settings UI

### GAP-7.1 Agent Hooks 通用框架(2026-06-18)
- PureTerm 充当 Claude Code / 其它 agent 的 hook 中转:agent 把 hook 事件 JSON 经 stdin 喂给 `pureterm hook <event>`,PureTerm 按 `[[agent.hook]]` 策略评估
- Standalone:不依赖 daemon / GUI,直接读 `pureterm.toml` + 本地评估 + 起子进程;零 IPC 往返
- 双规则模型:内置 `deny_pattern`(正则,无需写脚本) + 外部 `command`(经 `sh -c`,stdin 喂 JSON,stdout JSON/exit=决策,默认 60s 超时)
- 事件对齐官方协议:PreToolUse / PostToolUse / Notification / UserPromptSubmit / Stop / SubagentStop / PreCompact / SessionStart / SessionEnd
- 决策输出:PreToolUse 走 `hookSpecificOutput.permissionDecision`,其它阻断型走 `{"decision":"block"}`;exit 2=block
- 实现:`agent_hooks.rs`(框架) + `app_config.rs`(`load_agent_hooks`) + `cli.rs`(`Command::Hook`)
- 单测:171 全绿(含真实子进程 exit-2 / stdout JSON / 超时 kill 验证)

### GAP-7.2 Claude Code Agent 适配器 + 安装框架(2026-06-18)
- `AgentAdapter` trait + `ClaudeCodeAdapter`;框架化使 Codex / OpenCode / Grok / Pi 成为机械式后续
- CLI: `pureterm agents install claude-code` / `list` / `uninstall`;默认仅注册已配规则事件,`--all-events` / `--dry-run`
- 安全写配置:对 `~/.claude/settings.json` 走加锁读-改-写(fs2 排他锁 + `NamedTempFile::persist` 原子写),保留既有内容
- 幂等安装(命令路径哨兵去重) + 精准卸载(只删自己的组,清空事件数组与空 hooks 键)
- 实现:`agent_adapters.rs` + `cli.rs`(`Command::Agents` / `AgentsSub`) + `main.rs`
- 单测:181 全绿

### GAP-7.3 Agent 休眠(2026-06-23)
- daemon idle 采集:PTY 无输出 + CPU 近零 → 双条件判定
- hibernate:SIGSTOP 挂起;resume:SIGCONT 恢复
- GUI 决策面板:HibernationScanTick 双条件 + max_live_terminals 阈值 + 60s 确认倒计时 + pinned overlay
- 命令行:`pureterm agent hibernate/resume/list`
- 状态栏指示:`❄ kind`(hibernated) / `⚡ kind`(active) / `⏳ N`(feed pending)
- 实现:`idle_probe.rs`(CPU 采样) + `session.rs`(hibernate/resume) + `hibernate_panel.rs` + `iced_app.rs` + `cli.rs`
- 自动 E2E harness 全绿

### GAP-7.4 Feed 审批面板(2026-06-23)
- 延迟回复(承重墙):shim 与 GUI 单连接单请求,`feed.request_approval` 不立刻回 reply,暂存 `IpcRequest` 到 `FeedPanelState::pending`,用户点按钮时 resolve → 解锁 listener
- `ask_pattern` / `ask_reason` 配置;evaluate 在 deny 后聚合 Ask(deny 永远赢过 ask)
- 5 按钮面板:Once / Always / Deny / Bypass 5min / 等待;120s 软超时自动 Deny
- 作用域记忆:Always(session_id,tool_name) / All(session_id) / Bypass(session_id+5min 过期)
- SessionEnd 清记忆;feed.jsonl 审计
- 实现:`feed_client.rs`(std-only) + `feed_panel.rs` + `agent_hooks.rs`(ask_pattern) + `ipc/methods.rs` + `iced_app.rs`
- 自动 E2E harness 全绿:220 单测

### GAP-7.5 Agent 会话恢复(2026-06-23)
- session id 持久化:SessionStart hook → `AgentSessionStore`(FIFO 16 条) → `data_dir()/agents/<kind>-sessions.json`
- 冷恢复:GUI 重启 → `restore()` → daemon session 死 + pane 有 agent_session_id → 查 `[[vault.agents]]` resume_command → `claude --resume <id>` via `create_local(argv)`
- PaneSnapshot 加 `agent_session_id` / `agent_kind`
- 实现:`agent_sessions.rs` + `feed_client.rs::notify_session_started` + `session_restore.rs::cold_resume_agent` + `session.rs::create_local`
- 自动 E2E harness 全绿

### GAP-7.6 Vault agent 注册表(2026-06-23)
- `[[vault.agents]]` 声明式注册:`kind` / `name` / `detect`(子串) / `resume_command` / `never_hibernate`
- 进程扫描:`agent_detect.rs`(macOS `ps` / Linux `/proc`,detect 子串匹配,排除 grep/rg/ps/shell)
- CLI:`pureterm agents detect`(扫描打表) / `pureterm agents registry`(打印解析结果)
- SessionStart 默认注册修复:`derive_entries` 在 `[[vault.agents]]` 非空时自动追加,打通 GAP-7.5 冷恢复
- detect-resume GUI 面板:`detect_panel.rs` + `DetectScanTick`(30s) + vault 模板插值 → `create_resume_tab`
- 状态栏 `⚡ kind` / `❄ kind` / `⏳ N` 指示(`agent_indicator` 纯函数 + `last_session_list` 缓存,零新 IPC)
- 实现:`agent_detect.rs` + `feed_log.rs` + `agent_adapters.rs` + `iced_app.rs` + `cli.rs`
- 自动 E2E harness 全绿:305 单测

### GAP-7.7 自定义恢复绑定(2026-06-23)
- `[[resume.bindings]]` 增删改查:`pureterm surface resume set/list/remove`
- 轻量占位符插值:`{session_id}` / `{checkpoint}` / `{kind}` / `{cwd}`
- HMAC-SHA256 持久令牌:secret `<data_dir>/resume.key`(0600) + 摘要 `resume-tokens.json`;绑定精确 shell
- 冷恢复不弹面板:auto 绑定 + 有效令牌 → 静默 spawn;无令牌 → 跳过回落 `claude --resume`
- 复用 Feed 面板:`FeedRequestPayload.resume`;Always → `approve_token`
- 实现:`resume_bindings.rs` + `app_config.rs` + `cli.rs` + `feed_client.rs` + `feed_panel.rs` + `iced_app.rs`
- 自动 E2E harness 全绿:293 单测

### GAP-7.9 PTY 读写原语(2026-06-24)
- 补齐 Socket API 两个承重空壳,让任何 agent / 脚本能「读屏 + 注命令」(免 shell-out、免 JSON 解析)
- `surface.send_text`(`ipc/methods.rs`):`text` 必填 + `pane_id` 可选 → `IpcSideEffect::SendText`(unix);`iced_app::execute_ipc_side_effect` 镜像 `SshDisconnect` 解析 pane → `write_to_pty`(fire-and-forget)
- `surface.snapshot`:**deferred reply**(仿 `browser.*`,全 scrollback 序列化持 `FairMutex<Term>` 锁跨千行 off-main 避免掉帧);`Message::Ipc` 拦截 → clone `Arc<FairMutex<Term>>` → `Task::perform` 内 `spawn_blocking` 复用 `Term::bounds_to_string`(**不重写 cell 迭代**)+ tail 截尾 + `tokio::time::timeout`;oneshot first-wins
- 命令行:`pureterm surface send-text --pane-id N --text 'echo hi\r'` / `snapshot --visible-only --lines N`
- 独立可用:CLI / 脚本 / MCP 都消费它;零依赖
- 实现:`surface_snapshot.rs` + `ipc/methods.rs` + `iced_app.rs` + `cli.rs`
- 378 单测绿

### GAP-7.10 MCP server + agent 客户端安装(2026-06-24)
- PureTerm 成为 agent 可驱动的终端宿主:`pureterm mcp serve` 起 **stdio MCP server**,Claude Code / Codex 装一次即获 `capture_pane` / `send_text` / `list_surfaces` / `new_tab` / `split_pane` / `focus_pane` + **全 9 个 browser 工具**(`browser_navigate` / `browser_extract` / `browser_click` / `browser_type` / `browser_scroll` / `browser_wait` / `browser_get_url` / `browser_get_title` / `browser_screenshot`)原生结构化工具
- **MCP↔IPC 桥**(`mcp/server.rs`,feature-gated `mcp-server` 默认开):rmcp 1.7.0,**精确双 pin `rmcp`+`rmcp-macros` =1.7.0**(rmcp 1.7.0 只声明 `rmcp-macros ^1.7.0` 会解析到 1.8.0 宏,`#[tool]` 展开引用 1.7.0 runtime 不存在的 `schema_for_input`);每工具 body = `Parameters<T>` → `rpc()` 助手把**阻塞** `send_rpc` 包 `spawn_blocking` 透传 GUI Socket API,`Result<String,String>`(Err → MCP `isError`)
- **安装适配器**(`mcp/adapter.rs`):`McpClientAdapter` trait + ClaudeCodeAdapter(`.mcp.json`)/ CodexAdapter(`~/.codex/config.toml`,TOML `locked_toml_rmw`)/ ClaudeDesktopAdapter;复用 `agent_adapters::locked_json_rmw` + `pureterm_bin`;install 幂等(remove-then-insert,保留他人 server);CLI `mcp install/list/uninstall <client>`
- `[mcp]` 配置(`McpConfig`):`default_capture_lines`(5000)+ `enabled_tools` 白名单(空=全部)
- 安全 stance:同用户所有权是主边界(GUI socket `0600`;`serve` 仅因 agent 住 pane 内才继承 socket);v1 不接 Feed 审批,用户主动 `mcp install` 即知情授权
- 实现:`mcp/{mod,server,adapter}.rs` + `app_config.rs` + `cli.rs` + `Cargo.toml`(feature `mcp-server`)
- E2E:stdio handshake / `tools/list` schema 生成 / 无 GUI 时 `tools/call` 优雅 `isError` 均验证;378 单测绿

### GAP-03 代码库索引 MVP(2026-06-29)
- 新 crate **`pureterm-codeindex`**:Tree-sitter 语义 chunk(Rust/Python/JS 函数·类·模块级) + SQLite 持久化 + **FTS5 关键词搜索**;按内容 hash 跳过未变文件
- **MCP 工具**(`mcp/server.rs`,本地索引,不经 GUI socket):`search_code(query)` / `get_file_context(path)` / `find_symbol(name)`;工作区根=`PURETERM_CODEINDEX_ROOT` 或进程 cwd;索引库缓存于 `~/Library/Caches/pureterm/codeindex/`(或 `$XDG_CACHE_HOME`)
- **状态栏 stub**:`[statusbar].components` 可选 `"codeindex"` 段,索引进行时显示 `idx N%`(GUI 与 MCP 分进程时通常隐藏)
- **TODO(defer)**:嵌入向量(`embeddings` 表已预留) / Merkle tree 增量同步 / 语义向量检索
- 实现:`crates/pureterm-codeindex/` + `pureterm/src/codeindex.rs` + `mcp/{mod,server}.rs` + `status_bar/components.rs`
- 单测:`cargo test -p pureterm-codeindex` 5/5 绿

### GAP-35 LSP 客户端 MVP(2026-06-29)
- 新 crate **`pureterm-lsp`**:JSON-RPC stdio 客户端;MVP 仅 **rust-analyzer**(`initialize`/`initialized`/`textDocument/didOpen`/`textDocument/publishDiagnostics`)
- **触发路径**:Hover Preview 右键「在编辑器中打开」→ `didOpen` 读盘并推送文档;诊断汇总进 **AI Chat 上下文** + 可选状态栏 **`lsp`** 段(`NE NW`)
- **配置**:`[lsp] enabled` + `[lsp.servers] rust = "rust-analyzer"`(默认 `enabled = false`,缺省不阻塞启动)
- 实现:`crates/pureterm-lsp/` + `pureterm/src/lsp.rs` + `app_config.rs` + `hover_preview.rs` + `status_bar/` + `ai_chat/context.rs`
- 单测:`cargo test -p pureterm-lsp` + `app_config`/`lsp`/`context` 绿;补全/hover/definition UI 留后续迭代

---

## G. 平台

### GAP-47 macOS AppleScript 自动化 MVP（2026-06-29）
- **CLI 桥**：`pureterm script …` — `new-tab` / `switch-tab` / `switch-workspace` / `send-text` / `get-cwd` / `ssh` / `open <pureterm://…>`；AppleScript 经 `do shell script "pureterm script …"` 调用
- **MCP 工具**：`script_get_cwd`（`shell.context` 别名）、与 GAP-48 共享的 `open_url`
- **配置**：`[applescript] enabled = true`（完整 NSAppleEventManager 字典 deferred）
- 实现：`automation.rs` + `script_cli.rs` + `cli.rs` + `mcp/server.rs`
- 676 单测绿

### GAP-48 `pureterm://` URL 方案（2026-06-29）
- **注册**：`Info.plist` `CFBundleURLTypes` + `pureterm url register`（`LSSetDefaultHandlerForURLScheme`）
- **macOS 处理**：`application:openURLs:` → 队列 → iced `TerminalWakeup` 执行
- **URL 动作**：`tab/new` · `tab/switch/N` · `workspace/switch/Name` · `ssh/user@host:port` · `send?text=` · `open?cwd=`
- **Socket API**：`workspace.list` / `workspace.switch` + `ssh.open`（临时 SSH 标签页）
- **配置**：`[url_scheme] enabled` / `register_on_launch`
- 实现：`macos_url_scheme.rs` + `automation.rs` + `ipc/methods.rs` + `iced_app.rs`
- 676 单测绿

### GAP-8.5 跨平台优势(持续维护)
- macOS / Linux / Windows 三平台持续维护

---

## P3 GAP MVP 清单（2026-06-29）

### GAP-06 环境感知 Agent MVP
- 本地任务队列 + `[ambient_agents]` 配置门控；云端 SSE/webhook 延后
- 实现：`ambient_agents.rs`

### GAP-13 广播输入
- ⌘⌃B 切换广播；当前 tab 内 sibling pane 同步 PTY 输入
- 实现：`broadcast_input.rs` + `iced_app` PTY 扇出

### GAP-14 增强 Vim 模式 MVP
- operators `d`/`c`、`p` 粘贴、寄存器骨架；`dd`/`dw` 最小路径
- 实现：`vi_mode_handler.rs` 扩展

### GAP-18 双向文本支持 MVP
- `[terminal].bidi_mode` + `unicode-bidi` 行重排 helper
- 实现：`bidi.rs`

### GAP-26 工作区分组 MVP
- `[[workspace_groups]]` 配置 + 分组数据模型
- 实现：`workspace_groups.rs`

### GAP-30 工具带 MVP
- 右侧栏 **Tools** 标签
- 实现：`toolbelt.rs` + `sidebar/right.rs`

### GAP-31 Quake 停靠 MVP
- `[quake].enabled` + F12 顶部停靠切换
- 实现：`quake_dock.rs`

### GAP-37 密码管理器 MVP
- `op` / `lpass` / `security` CLI + SSH 密码链
- 实现：`password_manager.rs`

### GAP-39 串口与 Telnet MVP
- Profile `session_type` + 外部 CLI argv（daemon 路由延后）
- 实现：`serial_telnet.rs`

### GAP-41 跳板机 MVP
- `jump_hosts` → `ssh -J` 参数
- 实现：`pureterm-ssh` `with_jump_hosts`

### GAP-42 登录脚本 MVP
- `login_commands` 首条 PTY 写入
- 实现：`login_scripts.rs`

### GAP-46 国际化 MVP
- 主应用 + Settings `I18n` en/zh；`[pureterm].locale`
- 实现：既有 `pureterm-settings::i18n`

### GAP-1.2 DECSCL 一致性级别 MVP
- `CSI Ps " p` 转义序列 → `Term.conformance_level` 字段
- 实现：`alacritty_terminal/src/decscl.rs`

---

## P4 GAP MVP 清单（2026-06-29）

> 配置见 `default-config.toml` P4 段；运行时 bundle：`p4_runtime.rs`；CLI：`p4_cli.rs`。

### GAP-05 语音输入 MVP
- `[voice_input].enabled` + `api_url` + 按键说话状态
- 实现：`voice_input.rs`（Whisper/cpal 未接）

### GAP-17 Sixel / 分块图形 MVP
- `[terminal_graphics]` + 协议检测 + LRU cache scaffold
- 实现：`terminal_graphics.rs`

### GAP-20 玻璃效果 MVP
- `[window.glass]` + 启动/重载 apply hook
- 实现：`macos_glass_effect.rs`

### GAP-22 触控栏 MVP
- `[touch_bar]` + 刷新桩
- 实现：`macos_touch_bar.rs`

### GAP-23 可视标签栏 MVP
- `[tab_bar].position` / `should_show()` 显示判定
- 实现：`visual_tab_bar.rs`

### GAP-28 窗口边框 MVP
- `[window.border]` 激活/非激活颜色
- 实现：`window_border.rs`

### GAP-29 角标覆盖层 MVP
- `[badge]` 文本/颜色/位置 + 按配置覆盖
- 实现：`badge_overlay.rs`

### GAP-34 文件预览 MVP
- `[file_preview]` + 按扩展名 `open`/`xdg-open`
- 实现：`file_preview.rs`

### GAP-38 文件传输 MVP
- `[file_transfer]` + `scp` 外部调用(Unix)
- 实现：`file_transfer.rs`

### GAP-44 共享会话 **桩**
- `[shared_session]` 配置 + `pureterm share invite|serve` 命令
- 实现：`shared_sessions.rs`（WebSocket/WebRTC 未接）

### GAP-45 自动更新
- `[update]` 配置（`enabled` / `auto_check` / `channel` / `feed_url`）
- 同步 fast-path：`pureterm update check` — 比对 `CARGO_PKG_VERSION` 与 `PURETERM_LATEST_VERSION` env var，自带 semver 比较
- 异步 remote check：`pureterm update check --remote` — reqwest 拉取 GitHub Releases API（默认 `https://api.github.com/repos/CNB/PureTerm/releases/latest`），`stable` channel 跳过 prerelease，UA 标 `pureterm/<ver>`
- 实现：`crates/pureterm/src/auto_update.rs`（6 单测：disabled / semver 基础 / `v` 前缀 / 不齐段 / 默认 feed / 空版本）
- macOS Sparkle 仍延后（`feed_url` 字段已就位可指向 AppCast）

### GAP-49 插件系统
- `[plugins]` 配置 + 默认目录 `~/.config/pureterm/plugins/<name>/`
- `plugin.toml` 清单解析：`name` / `version` / `description` / `author`，folder name 必须与 `name` 一致
- CLI：`pureterm plugin list|install <name>|uninstall <name>` — `list` 报告 manifest 错误而不是静默跳过；`uninstall` 拒绝非 plugin 文件夹（防御性）
- 实现：`crates/pureterm/src/plugin_system.rs`（9 单测：disabled / 缺目录 / install 创建 / 拒绝空名 / disabled install / 混合目录下的错误报告 / name mismatch / uninstall 成功 / 拒绝非 plugin 文件夹）
- WASM / dylib 运行时延后到 Phase D

### GAP-50 云同步
- `[cloud_sync]` 配置（`enabled` / `gist_id` / `token_env`，默认 `PURETERM_GIST_TOKEN`）
- 本地：`pureterm sync push|pull` → `~/.config/pureterm/sync/latest.json`（带 `schema = 1` 版本字段，schema 不匹配时 pull 拒绝）
- 远程：`pureterm sync push|pull --remote` — reqwest 调用 GitHub Gist API（`POST` 创建新 Gist 或 `PATCH` 更新现有 Gist），token 走 `token_env` 读取
- 实现：`crates/pureterm/src/cloud_sync.rs`（8 单测：disabled / local round trip / token 拒绝 disabled / token 拒绝缺 env / token 拒绝空 / schema mismatch / schema match / disabled 无 IO）

### GAP-52 工作流
- `[[workflows]]` 段：每个 entry 含 `name` / `description` / `steps` / `params` / `category`（palette 分组）/ `alias`（短命令）
- `{{key}}` 占位符展开；`missing_params` 检查（声明顺序）
- `find` 解析时 name 优先于 alias（避免 name=`d` 被 `alias: d` 的 workflow 误吞）
- `group_by_category` 给 palette 提供按类别分组
- CLI：`pureterm workflow list`（按 category 分组打印，显示 alias） / `pureterm workflow run <name|alias> --param k=v`（缺参则报错退出）
- 实现：`crates/pureterm/src/workflows.rs`（7 单测：参数展开 / 缺失参数 / name+alias 匹配 / name 优先 / category 分组 / 畸形 TOML 跳过 / alias 双向 round trip）

### GAP-53 笔记本
- `[notebooks]` 配置 + `.pureterm` JSON 格式
- 三种 cell：`markdown` / `code`（`language` + `source`）/ `output`（执行后追加）
- Cell 执行引擎：`Notebook::execute_cell(idx)` 调起 `$SHELL -c <source>`（缺省 `/bin/sh`），捕获 stdout + stderr（`-- stderr --` 分隔），写入 `output` cell
- CLI：`pureterm notebook show|new|run <path> <idx>` — `new` 写空 notebook；`show` 打印 cell 类型 + 摘要；`run` 执行单个 cell 并打印输出
- 实现：`crates/pureterm/src/notebooks.rs`（9 单测：round trip / cell kind / append 索引 / 执行 echo / 捕获 stderr / 拒绝非 code / 越界拒绝 / 畸形 JSON 拒绝 / 磁盘 round trip）
- Markdown 渲染、cell 编辑器、AI 集成留待后续

### GAP-54 背景图 MVP
- `[background].image/mode/opacity` + 路径解析
- 实现：`background_image.rs`（GPU 纹理未接）

### GAP-55 透明/模糊 MVP
- `[window.effects]` 不透明度 + 模糊应用钩子
- 实现：`window_effects.rs`

### GAP-56 焦点跟随鼠标 MVP
- `[focus_follows_mouse]` + 悬停延迟状态机
- 实现：`focus_follows_mouse.rs`

### GAP-57 触感反馈 MVP
- `[haptic]` + 事件门控桩
- 实现：`macos_haptic.rs`

### GAP-58 自定义主题 MVP
- `[theme]` 侧栏/标签栏/状态栏覆盖
- 实现：`custom_theme.rs`

### GAP-59 Emoji 渲染 MVP
- `[emoji].dedicated_atlas` + ZWJ 启发式
- 实现：`emoji_rendering.rs`

### GAP-60 分屏捕获 MVP
- `[pane_capture]` + 文本导出 + `pureterm capture export` 命令
- 实现：`pane_capture.rs`（PNG GPU 未接）

### GAP-61 打印支持 MVP
- `[print]` + `lpr` + 页眉/页脚格式化器
- 实现：`print_support.rs`

### GAP-4.6 无障碍 MVP
- `[accessibility]` + 播报状态 + 标签页名称
- 实现：`accessibility.rs`

---

## 统计

| 类别 | 已实现数 |
|---|---|
| A. 核心架构 | 5 |
| B. UI/UX 界面 | 8 |
| C. 终端交互 | 4 |
| D. SSH 远程 | 6 |
| E. 集成与可编程 | 5 |
| F. AI Agent 智能体 | 9 |
| G. 平台 | 5 |
| **总计** | **63** |

---
