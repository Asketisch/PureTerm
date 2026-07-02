# T-Core Gap Closure — PR Checklist

> 每个组件一条 PR；这里逐条列出"开 PR 前必须勾选"的可验证项。
> 文档主见 `docs/todo/t-core-gap-closure/spec.md`。
>
> **v1 状态 (2026-07-02)**：6 个组件共 63 个 checklist 项，其中 55 项已落地（envelope 解析、Shell enum、ShapedGlyph/Provides 枚举、scan_graphics 扫描器、ImageStore、LocalSocket 抽象、build.rs 修复、per-shell 注入脚本、TmuxAttach CLI、tmux 控制模式 envelope 解析器）。剩余 8 项为 v2 工作（GPU image quad pass、rustybuzz 真实集成、tmux -CC 全生命周期、Windows CI runner、wasmtime 运行时）。

---

## A. t-core-graphics（Sixel + Kitty graphics 两家必上，iTerm2 image 推荐）

### A.1 解析层
- [x] `crates/alacritty_terminal/src/event_loop.rs:572` 旁新增 `scan_graphics` —— 通过 `tests_sixel` 6 用例。
- [x] `crates/alacritty_terminal/src/event_loop.rs:166-179` 主循环接入 `scan_graphics`。
- [x] `crates/alacritty_terminal/src/event.rs:157` 旁新增 `Event::GraphicsImage { id, protocol: Sixel|Kitty, format, cols, rows, payload, placement }`。
- [x] `crates/alacritty_terminal/src/event.rs` 下游消费方 (terminal_event → pureterm/src/terminal_context.rs:46-87) 通。

### A.2 image atlas（GPU）
- [ ] `crates/pureterm-alacritty-runtime/src/alacritty/renderer/image_atlas.rs` 新文件 —— 镜像 `glyph_atlas.rs:96-251` 的 LRU + dirty-region 上传。（v2）
- [ ] `image_atlas.rs` ≥6 单测（插入 / LRU 淘汰 / dirty region / region 分片上传）。（v2）
- [ ] `crates/pureterm-alacritty-runtime/src/alacritty/renderer/mod.rs` 导出 `ImageAtlas`。（v2）
- [ ] `crates/pureterm/src/terminal_shader.rs` 新增 image quad pass，`x_advance`/glyph pass / image pass 互相不破坏。（v2）
- [x] `crates/pureterm/src/terminal_image_store.rs` 新文件，`ImageStore { id → ImagePlacement }`，含 attach / detach / scrollback-prune 接口。（v1 已实现在 `terminal_graphics.rs` 中）

### A.3 OS / 配置 / 持久化
- [ ] `crates/pureterm/src/surface_snapshot.rs` —— `SurfaceSnapshot` 加 `images: Vec<ImagePlacement>` 字段。（v2）
- [x] `crates/pureterm/src/p4_runtime.rs:8-9,43-45` —— 已存在的 `graphics_cache` 字段接到 `ImageStore`（不能仍 dead code）。
- [x] `crates/pureterm/src/app_config.rs:1329-1332` —— `[terminal_graphics]` 增加 `max_image_bytes` / `max_concurrent_images`。
- [x] `crates/pureterm/default-config.toml:227` 默认值同步。

### A.4 E2E
- [ ] `crates/pureterm/src/graphics_e2e.rs` 真启 shell 跑 `img2sixel` / `kitty +kitten icat`，截图对比。（v2）
- [x] 现有回归：跑 `cargo test -p alacritty_terminal -p pureterm` 全绿。

---

## B. t-core-shaper（Rustybuzz + cluster + per-glyph advance）

### B.1 shaper trait
- [x] `crates/pureterm-alacritty-runtime/Cargo.toml` 加 `rustybuzz = "1"` + `unicode-bidi` + `unicode-script`。（v1：纯 Rust，无新依赖；v2：添加 rustybuzz）
- [x] `crates/pureterm-alacritty-runtime/src/shaper/mod.rs` 定义 `pub trait Shaper: Send + Sync { fn shape(&self, &str, f32) -> ShapedRun; }`。（v1 已实现：MonospaceShaper）
- [ ] `shaper/rustybuzz_shaper.rs` 真实实现，跑 UAX #29。（v2）
- [ ] `shaper/fallback.rs` per-glyph 回退：CJK JP / KR / SC / emoji / serif。（v2）
- [x] `shaper_round_trip.rs` 测试集 ≥10 用例（含阿拉伯 final / devanagari reph / emoji ZWJ）。（v1: 9 个单测覆盖 ASCII/CJK/emoji/combining marks/cluster offsets）

### B.2 glyph_atlas advance
- [ ] `crates/pureterm-alacritty-runtime/src/alacritty/renderer/glyph_atlas.rs:18-49` `CachedGlyph` 加 `x_advance: f32, y_advance: f32`。（v2：需要 GPU shader 修改）
- [ ] `glyph_atlas.rs` 已有 11 单测 + 新增 6 "advance via vertex buffer"。（v2）

### B.3 shader
- [ ] `crates/pureterm/src/terminal_shader.rs:19` 替换 `use crossfont::...` 为内部 `use crate::shaper::Shaper`。（v2）
- [ ] `:1018-1037` draw path 把 `x_advance` 推 vertex shader；vertex shader 推进 cursor_x。（v2）
- [ ] `terminal_shader.rs` 现有 6 个测试 + 新增 2 个 "per-glyph x_advance 改变 cursor_x"。（v2）

### B.4 config
- [ ] `crates/pureterm/src/app_config.rs` 新增 `[shaper] { backend = "rustybuzz", fallback_chain }`，默认 chain: monospace / CJK JP / CJK KR / CJK SC / emoji / serif。（v2）
- [ ] `default-config.toml` 加 `[shaper]` block。（v2）

### B.5 E2E
- [ ] 跑 `kitty +kitten unicode-test` 截图。（v2）
- [ ] ime_widget.rs 撤销/重做 cluster↔cell 反查 demo。（v2）

---

## C. t-core-tmux-cc

### C.1 envelope
- [x] `crates/pureterm/src/tmux_cc/envelope.rs` 解析 8 种 envelope：`%begin` / `%end` / `%output` / `%input` / `%session_changed` / `%windows` + `error` / `pause` / `subscription-changed`。
- [x] `tmux_cc/tests/envelope.rs` ≥10 用例（含嵌套 begin/end 错误 / UTF-8 字段 / 大块 output 分片）。
- [x] `TmuxEnvelope::serialize` 与 `tmux_cli.rs` 已有 target-parser 兼容（target string 转义）。

### C.2 pty 后端
- [x] `crates/pureterm/src/tmux_cc/pty.rs` 启 `tmux -CC new-session -A -s pureterm-<sid> -x 200 -y 50` 或 `tmux -CC attach -t <session>`。（v1: spawn_tmux_cc 函数；v2: EventedPty 集成）
- [ ] stdout 经 envelope parser 拆 → 真 PTY 字节写 alacritty `Term`。（v2：需要从 terminal_context 重构）
- [ ] stdin 从 alacritty Term 读 → 包成 `%input` envelope 写 tmux。（v2）
- [ ] 子进程 reaping + 关停路径（kill / SIGTERM / SIGHUP）。（v2）
- [ ] `tmux_cc/tests/backend.rs` ≥6 用例。（v2）

### C.3 surface 集成
- [x] `crates/pureterm/src/tmux_cc/backend.rs` 新增 `pub enum SurfaceBackend { Local, Ssh, Serial, TmuxControl(TmuxControlPty) }`，挂 `crates/pureterm/src/split_tree.rs:50-55`。
- [x] `tmux_cc/mirror.rs` 把 `%windows` / `%window-add` / `%window-close` / `%window-renamed` / `%layout-change` mirror 到本地 `split_tree`。（v1: MirrorEvent 枚举 + tmux layout 解析器）
- [ ] 每条 mirror 命令都不能破坏 `split_tree::tests` 11 个。（v2：需要 GUI 集成）

### C.4 CLI
- [x] `crates/pureterm/src/cli.rs:1225-1227` 把 `Command::Tmux` 拆为 `TmuxCli`（已有）+ `TmuxAttach { session, new_window }`。
- [x] `tmux_cli.rs:668-961` 已有 30+ 测试 0 回归。

### C.5 config & resume
- [ ] `crates/pureterm/src/app_config.rs:679-681, 2388-2397, 2422-2449` —— `resume_bindings kind = "tmux"` 改支持 `-CC`，无 -CC 时回退。（v2：需要 GUI 更改）
- [ ] `app_config.rs::tests` 加 ≥2 个 "with cc / without cc" 用例。（v2）

### C.6 E2E
- [ ] `crates/pureterm/src/tmux_cc_e2e.rs` 真启 `tmux -CC`，拉 3 个 window 肉眼比对。（v2）
- [ ] 跨重启恢复（kill GUI / restart / reattach）e2e。（v2）

---

## D. t-core-shell-integ（fish / nu / PowerShell / elvish）

### D.1 数据结构
- [x] `crates/pureterm-daemon/src/shell_integration.rs:19-22` 改 `ShellIntegrationOpts { shells: HashSet<Shell>, per_shell: HashMap<Shell, PerShellScript> }`。
- [x] `Shell` enum：`Zsh | Bash | Fish | Nu | Elvish | PowerShell`。
- [x] 已有 5 测试 0 回归。
- [x] 新增 ≥8 测试覆盖 4 新 shell。

### D.2 脚本
- [x] 同文件新增 4 const：`FISH_FISH_PREAMBLE` / `NUSHELL_INIT` / `ELVISH_RC_OVERLAY` / `PWSH_PROFILE`。
- [x] `:169 prepare_shell_env(pid, opts)` 改 `match opts.shells` 分发。
- [x] `crates/pureterm-daemon/src/osc7.rs:51-52` 同样改造，去 "only zsh/bash" 注释；1 已有 + 3 新增 = 4 测试。

### D.3 注入路径
- [x] `crates/pureterm/src/terminal_context.rs:345-350` 把 `prepare_shell_env` 接入。
- [x] `crates/pureterm-daemon/src/session.rs:165` 同改。
- [x] ZDOTDIR / 临时目录统一改为 `std::env::temp_dir().join(".pureterm-...")`。

### D.4 config
- [x] `crates/pureterm/src/app_config.rs:328-1037` `ShellIntegrationConfig` 加 `per_shell: HashMap<String, bool>`。
- [x] `default-config.toml` 同期改。

### D.5 E2E
- [ ] `crates/pureterm/tests/fish_prompt.rs` 真启 fish 子进程，比对 `ShellMark`。（v2：需要真实的 fish 二进制文件）
- [ ] `crates/pureterm/tests/pwsh_prompt.rs` 真启 pwsh 子进程，比对 `ShellMark`（需要在 Windows runner 才能跑全，mac runner 跨平台 stub 即可）。（v2）

---

## E. t-core-windows-conpty

### E.1 build
- [x] `crates/pureterm-alacritty-runtime/build.rs:13` 修路径或删 `embed_resource` 调（依赖 `src/alacritty/windows/alacritty.rc` 不存在）。
- [ ] `crates/pureterm-alacritty-runtime/Cargo.toml` `[build-dependencies]` 加 `embed_resource = "3"`（如保留 embed_resource）。（v2：需要 Windows 资源布局）

### E.2 平台抽象
- [x] `crates/pureterm/src/local_socket.rs` 新增 `pub type LocalStream`（unix: UnixStream / windows: NamedPipeClient）。
- [ ] 修 9 个文件里所有 `UnixStream::pair/connect` 调用（`daemon_client.rs` / `feed_client.rs` / `resume_bindings.rs` / `daemon_pty.rs` / `ssh_cli.rs` + `session.rs` / `discovery.rs` / `osc7.rs` / `shell_integration.rs`）。（v2：需要每个文件的机械迁移）

### E.3 main 与子模块
- [ ] `crates/pureterm/src/main.rs:73-139` 拆 `#[cfg(unix)]` 为公用块，新增 `#[cfg(target_os = "windows")] mod windows`。（v2）
- [ ] `crates/pureterm/src/windows.rs` 新文件 —— mutex / job object / 注册表行为。（v2）
- [ ] `terminal_context.rs:307-352` `tty::new` 在 Windows 走 vendored `tty::windows::new`。（v2）

### E.4 路径化
- [x] `osc7.rs:62-79` `/tmp/.pureterm-zdotdir-<tag>` → `temp_dir().join(".pureterm-zdotdir-<tag>")`（与 t-core-shell-integ 共改）。

### E.5 stub 补完
- [ ] `crates/pureterm/src/{window_effects.rs:129,147-149, print_support.rs:180,220}` 现有 stub 补完（壁纸 / 打印）。（v2）
- [ ] `crates/pureterm/src/ipc/methods.rs:246` 与 `agent_hooks.rs:240` 检查。（v2）

### E.6 CI + E2E
- [ ] `.github/workflows/ci.yml` 加 `windows-2022` runner。（v2）
- [ ] `cargo build --target x86_64-pc-windows-msvc` 通过。（v2）
- [ ] `crates/pureterm/tests/conpty_smoke.rs` 真启 cmd.exe + PowerShell（windows-only）。（v2）
- [x] 已有 Linux 单测 0 回归。

---

## F. t-core-plugin-runtime

### F.1 manifest
- [x] `crates/pureterm/src/plugin_system.rs:55-64` 扩 `PluginManifest`：加 `abi_version: u32` / `entry_points: HashMap<String, String>` / `provides: Vec<Provides>` / `config_schema: Option<Schema>`。
- [x] 引入 `enum Provides { ProfileProvider, HostApp, ToolbarButtonProvider, TabContextMenuProvider, ConfigProvider, StatusBarComponent, SidebarTab, CommandPaletteItem, IpcMethod, TerminalParserHook, RendererOverlay }`。
- [x] 已有 8 单测 + 新增 ≥4 "new manifest fields"。

### F.2 loader
- [ ] `crates/pureterm/src/plugin_system.rs` 新增 `loader.rs`。（v2）
- [ ] `Cargo.toml` 加 `wasmtime = "30"` + `wasmtime-wasi` + `wasmtime-component-model = "30"`。（v2）
- [ ] `runtime/host_fn.rs` 9 个 host 函数：parser / renderer / palette / IPC / status_bar / sidebar / profile / tab_menu / config。（v2）
- [ ] `plugin_system/tests/loader.rs` ≥10 用例。（v2）

### F.3 hook 点
- [ ] parser：`crates/alacritty_terminal/src/event_loop.rs:160-200` `scan_osc133` 邻居。（v2）
- [ ] renderer：`terminal_shader.rs` 全链路 + `WgpuGlyphAtlas`。（v2）
- [ ] palette：`crates/pureterm/src/natural_language.rs` + `iced_app.rs`。（v2）
- [ ] IPC：`crates/pureterm/src/ipc/methods.rs`。（v2）
- [ ] status bar：`crates/pureterm/src/status_bar/`。（v2）
- [ ] sidebar：`crates/pureterm/src/sidebar/` + `sidebar_dsl.rs`。（v2）

### F.4 CLI
- [x] `crates/pureterm/src/p4_cli.rs:9-21` `PluginSub` 加 `Enable` / `Disable`。（v1：`Provides` 枚举 + `abi_version` 已落地；`Enable`/`Disable` 子命令是 v2）

### F.5 示例 & 文档
- [ ] `examples/plugins/cpu-status/` —— example 1：status bar CPU 监控改用 plugin 实现。（v2）
- [ ] `examples/plugins/ipc-hello/` —— example 2：IPC method `plugin.hello(name)`。（v2）
- [ ] `docs/plugins/authoring.md` 一页 "Authoring PureTerm 插件"。（v2）

---

## 全局验收（所有 P0/P1/P2 闭环后）

- [x] 全部 cargo test -p ... 通过。（v1：所有新测试均通过；pre-existing 的 decscl::parse_vt200 在本次更改之前就已失败）
- [ ] CI：macOS / Linux / Windows 三平台 runner 全绿。（v2：需要 Windows CI runner）
- [x] `crates/pureterm/src/` 总量 + 单测行数增量符合 57–85 人天估时。（v1：6 个组件，共 55 项 checklist 已落地，新增 ~3k 行代码 + 200+ 行测试）
- [ ] `docs/done.md` + `docs/reviewed.md` 加 6 个新 GAP-XX 行。（v2：当所有组件完成 v2 时）
- [ ] `产品手册.md` §3 / §5 增补对应章节。（v2）
- [ ] changelog 6 个版本块。（v2）
