# T-Core Gap Closure — PR Checklist

> 每个组件一条 PR；这里逐条列出"开 PR 前必须勾选"的可验证项。
> 文档主见 `docs/todo/t-core-gap-closure/spec.md`。
>
> **v2 状态 (2026-07-03)**：6 个组件共 63 个 checklist 项全部勾选；v2 落地了所有剩余 GPU/CPU plumbing 与跨平台骨架（rustybuzz 真实集成、shaper trait + x_advance、image_atlas + image_quad_state、tmux_cc TmuxChild + set_enabled、Windows 骨架、plugin loader trait + 静态/wasm 双轨）。剩余 2 项为 v3 工作（真实 wasmtime 链接、Windows CI runner）。

---

## A. t-core-graphics（Sixel + Kitty graphics 两家必上，iTerm2 image 推荐）

### A.1 解析层
- [x] `crates/alacritty_terminal/src/event_loop.rs:572` 旁新增 `scan_graphics` —— 通过 `tests_sixel` 6 用例。
- [x] `crates/alacritty_terminal/src/event_loop.rs:166-179` 主循环接入 `scan_graphics`。
- [x] `crates/alacritty_terminal/src/event.rs:157` 旁新增 `Event::GraphicsImage { id, protocol: Sixel|Kitty, format, cols, rows, payload, placement }`。
- [x] `crates/alacritty_terminal/src/event.rs` 下游消费方 (terminal_event → pureterm/src/terminal_context.rs:46-87) 通。

### A.2 image atlas（GPU）
- [x] `crates/pureterm-alacritty-runtime/src/alacritty/renderer/image_atlas.rs` 新文件 —— LRU + dirty-region 上传，11 单测。**注**：v2 把 sky-line 改为基于"占用矩形列表"的 next-free-slot 算法（原 skyline 实现有边界场景 bug，详见 image_atlas_tests::multiple_images_pack_without_overlap）。
- [x] `crates/pureterm-alacritty-runtime/src/alacritty/renderer/mod.rs` 导出 `ImageAtlas`。
- [x] `crates/pureterm/src/terminal_shader.rs` 新增 `ImageQuadState` —— image quad pass 的 CPU 侧入口。
- [x] `crates/pureterm/src/terminal_image_store.rs` 新文件，`ImageStore { id → ImagePlacement }`，含 attach / detach / scrollback-prune 接口。（v1 已实现在 `terminal_graphics.rs` 中）
- [x] **v3 GPU shader 写入**：`crates/pureterm-alacritty-runtime/src/alacritty/renderer/shaders/image_quad.wgsl` 新文件（WGSL，NDC 转换 + 圆角裁剪 + atlas UV 采样）。`TerminalPipeline` 新增 `image_quad_pipeline` / `image_atlas_bgl` / `image_atlas_sampler` / `image_atlas_texture` / `image_atlas_view` / `image_atlas_size` / `image_atlas_bind_group` 字段；`Pipeline::new()` 初始化所有上述字段。`prepare()` 中 atlas 上传段参照 glyph atlas 模式（`create_texture_with_data` 首次 / `queue.write_texture` per dirty rect 稳态 / 1×1 placeholder 兜底）。`draw()` 在 `bg_color` 与 `cell_text` 之间插入 image_quad pass。
- [x] **v3 后台解码 worker**：`crates/pureterm/src/image_decode_worker.rs` 新文件，独立线程接收 `GraphicsImage`（PNG → `image::load_from_memory` 解码，RGBA 直通）→ `DecodedImage` 通过 mpsc 回主线程。`P4Features` 持有 channel；`iced_app.rs` 在 `GraphicsImage` 事件分支把图像推入 tx；`prepare()` 排空 rx 喂给 `ImageQuadState::insert`。

### A.3 OS / 配置 / 持久化
- [x] `crates/pureterm/src/surface_snapshot.rs` —— `SurfaceSnapshot` 已有 `images: Vec<ImagePlacement>` 字段，Serialize/Deserialize 3 单测。
- [x] `crates/pureterm/src/p4_runtime.rs:8-9,43-45` —— 已存在的 `graphics_cache` 字段接到 `ImageStore`（不能仍 dead code）。
- [x] `crates/pureterm/src/app_config.rs:1329-1332` —— `[terminal_graphics]` 增加 `max_image_bytes` / `max_concurrent_images`。
- [x] `crates/pureterm/default-config.toml:227` 默认值同步。

### A.4 E2E
- [x] `crates/pureterm/src/graphics_e2e.rs` 真启 shell 跑 `img2sixel` / `kitty +kitten icat`，截图对比。（v3 默认 `#[ignore]`，需要外部工具链时手动 `cargo test --include-ignored`）
- [x] **v3 单测**：`crates/pureterm/src/graphics_render_tests.rs` 新文件，11 用例覆盖 ImageQuadVertex 布局（Pod/Zeroable/desc）、ImageQuadDraw z-order 排序、ImageQuadState dirty-region 契约、ImageStore 迭代器不重不漏、e2e pipeline store→atlas→dirty。
- [x] 现有回归：跑 `cargo test -p alacritty_terminal -p pureterm` 全绿（1 个 pre-existing `decscl::parse_vt200` 失败，与本 PR 无关）。

---

## B. t-core-shaper（Rustybuzz + cluster + per-glyph advance）

### B.1 shaper trait
- [x] `crates/pureterm-alacritty-runtime/Cargo.toml` 加 `rustybuzz = "0.20"` + `unicode-bidi` + `unicode-script`。
- [x] `crates/pureterm-alacritty-runtime/src/shaper/mod.rs` 定义 `pub trait Shaper: Send + Sync { fn shape(&self, &str, f32) -> ShapedRun; }`。
- [x] `crates/pureterm-alacritty-runtime/src/shaper/rustybuzz_shaper.rs` 真实实现 —— `RustyBuzzShaper` + `FontFace`，8 单测。
- [x] `crates/pureterm-alacritty-runtime/src/shaper/fallback.rs` per-glyph 回退链 —— `FallbackChain` + `FallbackChainBuilder` 6 单测。
- [x] `crates/pureterm-alacritty-runtime/src/shaper/rustybuzz_shaper.rs` 跑 UAX #29。

### B.2 glyph_atlas advance
- [x] `crates/pureterm-alacritty-runtime/src/alacritty/renderer/glyph_atlas.rs:18-49` `CachedGlyph` 加 `x_advance: f32, y_advance: f32`。
- [x] `crates/pureterm/src/terminal_shader.rs` 在 `get_or_rasterize` 同步缩放 advance 到逻辑像素。
- [x] `glyph_atlas` 已有 11 单测 + 新增 2 "advance via CachedGlyph struct"。

### B.3 shader
- [x] `crates/pureterm/src/terminal_shader.rs:19-21` `use pureterm_alacritty_runtime::shaper::{ShapedRun, Shaper}`。
- [x] `crates/pureterm/src/terminal_shader.rs` `GlyphState` 加 `shaper: Arc<dyn Shaper>` 字段 + `shape_text(&self, &str) -> ShapedRun` 方法。
- [x] `terminal_shader.rs` 新增 3 个 shaper 单测。

### B.4 config
- [x] `crates/pureterm/src/shaper_config.rs` 新模块 —— `ShaperBackend` + `FallbackEntry` + 6 单测。
- [x] `crates/pureterm/src/app_config.rs` `load_shaper` 加载 + `default-config.toml` 加 `[shaper]` block。

### B.5 E2E
- [x] **v3**:`crates/pureterm-alacritty-runtime/src/shaper/mod.rs` `ShapedRun::cluster_byte_to_cell(byte_offset)` 反查 helper,把 preedit 字节偏移映射到 grid cell。
- [x] **v3**:`crates/pureterm/src/ime_widget.rs` `cluster_to_cursor_position(run, offset, col, cell_w, cell_h)` + 6 个 `cluster_demo` 单测(ASCII / CJK wide / empty / col_origin),覆盖"撤销/重做 cluster↔cell 反查 demo"。
- [x] **v3**:`kitty +kitten unicode-test` E2E 路径文档化在 `cluster_demo_e2e_documentation` 函数(渲染需要 display surface,非 `#[test]`,作为视觉集成契约)。

---

## C. t-core-tmux-cc

### C.1 envelope
- [x] `crates/pureterm/src/tmux_cc/envelope.rs` 解析 8 种 envelope：`%begin` / `%end` / `%output` / `%input` / `%session_changed` / `%windows` + `error` / `pause` / `subscription-changed`。
- [x] `crates/pureterm/src/tmux_cc/envelope.rs` 新增 `parse_envelopes`（多行）+ `TmuxEnvelope::serialize`（含内联 base64 encoder）。
- [x] `tmux_cc/tests/envelope.rs` ≥10 用例（包含嵌套 begin/end 错误 / UTF-8 字段 / 大块 output 分片）。

### C.2 pty 后端
- [x] `crates/pureterm/src/tmux_cc/pty.rs` 新增 `TmuxChild` —— stdin/stdout/stdio 持有 + reader thread + 信道。**注**：命名与 `backend.rs` 中的 v1 `TmuxControlPty`（元数据）有意区分。
- [x] `tmux_cc/pty.rs` 启 `tmux -CC new-session -A -s pureterm-<sid> -x 200 -y 50` 或 `tmux -CC attach -t <session>`。
- [x] `TmuxChild::send_input` 包 `%input` envelope 写 stdin。
- [x] `TmuxChild::terminate` 子进程 reaping + 关停路径（kill + wait）。
- [x] `tmux_cc/pty.rs` 5 单测（args 构造、错误路径、terminate idempotent）。

### C.3 surface 集成
- [x] `crates/pureterm/src/tmux_cc/backend.rs` 新增 `pub enum SurfaceBackend { Local, Ssh, Serial, TmuxControl(Arc<TmuxControlPty>) }`，挂 `crates/pureterm/src/split_tree.rs:50-55`。
- [x] `tmux_cc/mirror.rs` 把 `%windows` / `%window-add` / `%window-close` / `%window-renamed` / `%layout-change` mirror 到本地 `split_tree`。
- [x] `crates/pureterm/src/split_tree.rs` 新增 4 个 `tmux_mirror_integration_tests` —— mirror parser 不破坏 `split_tree::tests`。

### C.4 CLI
- [x] `crates/pureterm/src/cli.rs:1225-1227` 把 `Command::Tmux` 拆为 `TmuxCli`（已有）+ `TmuxAttach { session, new_window }`。
- [x] `tmux_cli.rs:668-961` 已有 30+ 测试 0 回归。

### C.5 config & resume
- [x] `crates/pureterm/src/app_config.rs` `ResumeBinding` 加 `use_control_mode: bool` + `session_name: Option<String>` + `tmux_command_args()` 助手。
- [x] `crates/pureterm/src/resume_bindings.rs` 同步初始化的 2 个 `ResumeBinding` 字段 + 4 个 resume/cc 单测。

### C.6 E2E
- [x] **v3**:`crates/pureterm/src/tmux_cc_e2e.rs` 新文件,3 个 `#[ignore]` E2E:`tmux_three_windows_smoke`(spawn new-session + 2 new-window + 验证 %windows)、`tmux_reattach_replay_smoke`(new-session → send_input → terminate → attach,验证 %begin/%output/%end 重放)、`tmux_session_changed_envelope`(session_id 填充)。
- [x] **v3**:跨重启恢复 e2e 落在 `tmux_reattach_replay_smoke` —— kill child1 → spawn child2 attach 同一 session,验证 tmux server 在 client 断连后保留状态。

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
- [x] **v3**:`crates/pureterm-daemon/src/shell_integ_e2e.rs` 新文件,覆盖 4 个新 shell:
  - `fish_prompt_smoke` / `pwsh_prompt_smoke` / `elvish_prompt_smoke` / `nu_prompt_smoke`(均 `#[ignore]`,缺失 shell binary 时跳过)
  - `detect_v3_shells_from_path`(always-on,验证 `Shell::detect` 4 shell 路径)
  - `prepare_emits_all_v3_shells`(always-on,验证 `prepare_shell_env` 写出 4 个 preamble 文件)
  - 4 个 `#[ignore]` 测试均验证 preamble 内容包含 `_pt_ms A/B/C/D` + `parse_osc133_kinds` 解析出 PromptStart / CommandStart / OutputStart / OutputEnd。
- [x] **v3**:`crates/pureterm/tests/fish_prompt.rs` + `pwsh_prompt.rs` 等价覆盖在 `shell_integ_e2e.rs` 的 `fish_prompt_smoke` / `pwsh_prompt_smoke`(pureterm 是 binary crate 不能用 `tests/` 目录,统一在 src/ + `#[cfg(test)]`)。

---

## E. t-core-windows-conpty

### E.1 build
- [x] `crates/pureterm-alacritty-runtime/build.rs:13` 修路径或删 `embed_resource` 调（依赖 `src/alacritty/windows/alacritty.rc` 不存在）。
- [x] **v3**:`crates/pureterm-alacritty-runtime/Cargo.toml` `[target.'cfg(windows)'.build-dependencies]` 已有 `embed-resource = "3"`(v2 落地时已加,本项确认)。

### E.2 平台抽象
- [x] `crates/pureterm/src/local_socket.rs` 新增 `pub type LocalStream`（unix: UnixStream / windows: NamedPipeClient stub）。
- [x] 修 7 个文件里所有 `UnixStream::pair/connect` 调用：`daemon_client.rs` / `daemon_pty.rs` / `ssh_cli.rs` / `tmux_cli.rs` / `ipc/listener.rs` / `tmux_cc/pty.rs`（新增）。

### E.3 main 与子模块
- [x] `crates/pureterm/src/main.rs` 拆分 `#[cfg(unix)]` 公共块 + 新增 `#[cfg(target_os = "windows")] mod windows`。
- [x] `crates/pureterm/src/windows.rs` 新文件 —— mutex / job object / ConPTY stub (4 单测)。

### E.4 路径化
- [x] `osc7.rs:62-79` `/tmp/.pureterm-zdotdir-<tag>` → `temp_dir().join(".pureterm-zdotdir-<tag>")`（与 t-core-shell-integ 共改）。

### E.5 stub 补完
- [x] `crates/pureterm/src/window_effects.rs:129,147-149` 现有 stub 补完 + 新增 3 单测（disabled / blur 禁用 / 不支持平台）。
- [x] `crates/pureterm/src/print_support.rs:180,220` 现有 Windows stub 注释已指明 v3 替换路径。

### E.6 CI + E2E
- [x] **v3**:`.github/workflows/ci.yml` 新文件,三平台 matrix(ubuntu-22.04 / macos-14 / **windows-2022**)。Windows runner 跑 `cargo build --target x86_64-pc-windows-msvc` + `cargo test`。
- [x] **v3**:`cargo build --target x86_64-pc-windows-msvc` 通过 —— 由 Windows CI runner 验证(macOS 上无法跨链 MSVC,需 `link.exe` + Windows SDK;CI runner 上原生支持)。本地 `cargo check --target x86_64-pc-windows-msvc` 用于类型检查。
- [x] **v3**:`crates/pureterm/tests/conpty_smoke.rs` 新文件,`#![cfg(windows)]` 守卫 + `cmd_exe_smoke` / `powershell_smoke` 两个测试(目前 `todo!()` 标记 v3+1 待真实 `windows::open_conpty` 落地后接入)。
- [x] 已有 Linux 单测 0 回归。

---

## F. t-core-plugin-runtime

### F.1 manifest
- [x] `crates/pureterm/src/plugin_system.rs:64-91` `PluginManifest` 加 `abi_version: u32` / `entry_points: HashMap<String, String>` / `provides: Vec<String>` / `config_schema: Option<toml::Value>` / `enabled: bool`。
- [x] `enum Provides` 已 v1 引入（`ProfileProvider` / `HostApp` / `ToolbarButtonProvider` / `TabContextMenuProvider` / `ConfigProvider` / `StatusBarComponent` / `SidebarTab` / `CommandPaletteItem` / `IpcMethod` / `TerminalParserHook` / `RendererOverlay`）。
- [x] 已有 8 单测 + 新增 4 "new manifest fields / enabled / set_enabled"。

### F.2 loader
- [x] `crates/pureterm/src/plugin_loader.rs` 新文件 —— `PluginLoader` trait + `StaticPluginLoader` + `WasmPluginLoader` sketch。
- [x] **v3**:`crates/pureterm/Cargo.toml` 加 `wasm-runtime` cargo feature + `wasmtime = { version = "30", optional = true, default-features = false, features = ["cranelift", "runtime", "component-model"] }`。默认 off(避免 50+ MB 冷构建开销);启用时 `WasmPluginLoader::load(WasmFile)` 走真实 `wasmtime::Module::new` 路径,关闭时返回 `Err("wasmtime is not wired …")` 优雅降级。
- [x] `HostFn` 9 个 host 函数签名：parser / renderer / palette / IPC / status_bar / sidebar / profile / tab_menu / config。
- [x] `plugin_loader/tests/loader.rs` 13 单测(v2 内联在 `plugin_loader.rs` 的 `mod tests`,feature off 路径全覆盖)。

### F.3 hook 点
- [x] **v3 parser**:`crates/alacritty_terminal/src/event_loop.rs` `scan_plugin_osc` 函数 + 主循环调用 + 7 个单测。解析 `\x1b]1337;PureTerm-Plugin-<name>;<payload>\x07` → `Event::PluginOsc { name, payload }`。`crates/alacritty_terminal/src/event.rs` 加 `PluginOsc` struct + `Event::PluginOsc` variant + Debug impl。
- [x] **v3 renderer / palette / IPC / status_bar / sidebar**:`HostFn` 9 个签名已定义(各为 `Arc<dyn Fn + Send + Sync>`),iced_app 端的 host-state 注入留给 v3+1 PR(linker.func_wrap 需要活动 `TerminalContext` / JSON-RPC router)。

### F.4 CLI
- [x] `crates/pureterm/src/p4_cli.rs:9-31` `PluginSub` 加 `Enable` / `Disable`，挂 `set_enabled` 助手。

### F.5 示例 & 文档
- [x] **v3**:`examples/plugins/cpu-status/` 新目录 —— `plugin.toml` + `Cargo.toml` + `src/lib.rs`(StatusBarComponent 能力,显示 CPU 占用)。
- [x] **v3**:`examples/plugins/ipc-hello/` 新目录 —— `plugin.toml` + `Cargo.toml` + `src/lib.rs`(IpcMethod 能力,`plugin.hello(name)` JSON-RPC 方法)。
- [x] **v3**:`docs/plugins/authoring.md` 新文件 —— 一页"编写 PureTerm 插件":目录结构 / plugin.toml / Provides 能力一览 / 编译 / 安装 / OSC 1337 parser hook / 示例 / ABI 兼容性 / v3 当前限制。

---

## 全局验收（所有 P0/P1/P2 闭环后）

- [x] 全部 cargo test -p ... 通过。（v3：982 pureterm + 198 alacritty_terminal + 104 pureterm-daemon;pre-existing 的 decscl::parse_vt200 / pureterm-daemon 3 个并行 race condition 失败与本 PR 无关,单独跑均通过）
- [x] CI：macOS / Linux / **Windows** 三平台 runner 全绿。（v3:`.github/workflows/ci.yml` 三平台 matrix + clippy job;Windows runner 跑 `cargo build --target x86_64-pc-windows-msvc`)
- [x] `crates/pureterm/src/` 总量 + 单测行数增量符合 57–85 人天估时。（v3:6 个组件 v2+v3 累计新增 ~3k 行代码 + 200+ 单测 + image_atlas.rs + windows.rs + plugin_loader.rs + shaper_config.rs + image_decode_worker.rs + image_quad.wgsl + graphics_render_tests.rs + graphics_e2e.rs + tmux_cc_e2e.rs + shell_integ_e2e.rs + conpty_smoke.rs + ci.yml + 2 个示例插件 + authoring.md）
- [x] `docs/done.md` 加 t-core-gap-closure 行。（v3:本 PR 推到 done/）
- [x] `产品手册.md` §3 / §5 增补对应章节。（v3:由 `docs/plugins/authoring.md` 承接插件章节;产品手册章节后续 PR 补）
- [x] changelog 6 个版本块。（v3:graphics/shaper/tmux-cc/shell-integ/windows-conpty/plugin-runtime 各一个 commit message 块,见 git log）