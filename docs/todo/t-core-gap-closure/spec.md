---
gap: t-core-gap-closure
title: 终端模拟核心层差距补齐（graphics / shaper / tmux-CC / 多 shell / ConPTY / plugin runtime）
category: 终端交互
plan_type: arch
priority: P0
effort: XL
status: v2-closed
v1_closed: 2026-07-02
v1_summary: 全部 6 个组件的 v1 骨架 / 协议解析 / 枚举 / 测试已落地。
v2_closed: 2026-07-03
v2_summary: 全部 6 个组件的 v2 落地（rustybuzz 真实集成 + shaper trait + CachedGlyph x_advance；image_atlas + ImageQuadState + surface_snapshot ImagePlacement；tmux_cc TmuxChild + pty 通道；ResumeBinding use_control_mode；local_socket 跨平台 + windows.rs 骨架；plugin_loader trait + StaticPluginLoader + WasmPluginLoader sketch + set_enabled）。剩余 wasmtime 实际链接、Windows CI runner、GPU quad shader 写入为 v3。逐组件详情见 checklist.md。
umbrella: true
components:
  - gap: t-core-graphics
    title: 图形协议 (Sixel + Kitty graphics + iTerm2 inline image 三选二)
    priority: P0
    effort: M (10–14 d)
  - gap: t-core-shaper
    title: Harfbuzz 字形 shaper (rustybuzz) + cluster + per-glyph advance
    priority: P0
    effort: M (8–12 d)
  - gap: t-core-tmux-cc
    title: tmux 控制模式 (attach 真 tmux server)
    priority: P0
    effort: L (12–18 d)
  - gap: t-core-shell-integ
    title: Shell integration 覆盖 fish / nu / PowerShell / elvish
    priority: P1
    effort: S (5–8 d)
  - gap: t-core-windows-conpty
    title: Windows / ConPTY 上线 (LocalSocket 抽象 + ZDOTDIR 路径化 + build.rs 修复)
    priority: P1
    effort: L (10–15 d)
  - gap: t-core-plugin-runtime
    title: Plugin runtime (wasmtime + ABI + provides)
    priority: P2
    effort: L (12–18 d)
moved_to_todo: 2026-07-02
---

# 终端模拟核心层差距补齐（T-Core Gap Closure）

## 概述

对标 Kaku / cmux / Ghostty / iTerm2 / Nezha / Orca / Tabby / Warp 后，识别出 PureTerm 在终端模拟核心层的 6 块结构性短板。这是一份 umbrella spec，落地时会拆成 6 个独立 GAP-XX 分别走 done → reviewed 流程。

| # | P | 主题 | 估时 | 阻塞 |
|---|---|---|---|---|
| 1 | P0 | 图形协议 (Sixel + Kitty + iTerm2 inline 中至少两家) | 10–14 d | Agent 富媒体输出 |
| 2 | P0 | Harfbuzz shaper (rustybuzz) + cluster + advance | 8–12 d | 所有复杂脚本 |
| 3 | P0 | tmux 控制模式 | 12–18 d | 会话迁移 |
| 4 | P1 | Shell integ 覆盖 fish / nu / PowerShell / elvish | 5–8 d | Win/fish 用户 |
| 5 | P1 | ConPTY 打穿 Windows | 10–15 d | Win 用户 |
| 6 | P2 | Plugin runtime (wasmtime + provides + ABI) | 12–18 d | 第三方扩展 |

合计 6 个闭环，P0 + P1 + P2 估 57–85 人天。

---

## 1. 组件 t-core-graphics —— 图形协议 (Sixel + Kitty)

**优先级 P0，估时 M（10–14 d）。**

### PureTerm 现状（审计证据）

**已存在（seam 可接入）**
- `crates/pureterm/src/terminal_graphics.rs:1-78` —— `GraphicsPlacement { id, cols, rows, payload_len }` + FIFO `GraphicsCache` + `detect_protocol` 仅嗅探 `\x1b_G` (Kitty) / `\x1bPq` (Sixel)。
- `crates/pureterm/src/p4_runtime.rs:8-9,43-45` —— `P4Runtime` 已带 `terminal_graphics_cfg` + `graphics_cache` 字段。
- `crates/pureterm/src/app_config.rs:1329-1332` —— `AppConfig::load_terminal_graphics` 读 `[terminal_graphics]` 表。
- `crates/pureterm/default-config.toml:227` —— 默认 `[terminal_graphics]` block 已就绪。
- `crates/pureterm/Cargo.toml:67-69` —— `image = "0.25"` 已为 `background_image` 引入，PNG 解码可直接复用。
- `crates/alacritty_terminal/src/event_loop.rs:572` —— `scan_osc133` 邻居就是天然注入点。
- `crates/pureterm-alacritty-runtime/src/alacritty/renderer/glyph_atlas.rs:96-251` —— 图集机制可镜像。

**核心缺口**
- 无字节级 parse、无 base64 解码、无 PNG/RGBA 解码管线。
- `Term::grid` 无 image cell，placement 是 cache 里的元数据，没锚定到 row/col/scrollback。
- 无 GPU overlay pass —— `WgpuGlyphAtlas` 只画 glyph，shader 不会画 image quad。
- scrollback 无 image bookkeeping：`Surface::Terminal` → `TerminalContext` → `surface_snapshot.rs` 全是纯文本。
- 零调用方 —— `detect_protocol` 在 workspace 无任何调用。

### 对标实现 (直接抄)

| 来源 | 协议 | 路径 |
|------|------|------|
| **Kaku** | Sixel | `/Users/admin/Code/Github/Terminal/Kaku-main/term/src/terminalstate/sixel.rs:7-126` (DCS 参数 + palette + 像素) |
| **Kaku** | Sixel image-store | `/Users/admin/Code/Github/Terminal/Kaku-main/term/src/terminalstate/image.rs:55-260` |
| **Kaku** | iTerm2 inline | `/Users/admin/Code/Github/Terminal/Kaku-main/term/src/terminalstate/iterm.rs` |
| **iTerm2** | Sixel parser | `/Users/admin/Code/Github/Terminal/iTerm2-master/sources/VT100/VT100SixelParser.{h,m}` + `VT100Output.h:159` |
| **iTerm2** | Inline image | `/Users/admin/Code/Github/Terminal/iTerm2-master/sources/VT100/VT100InlineImageHelper.h:2` |
| **Ghostty** | Kitty API | `/Users/admin/Code/Github/Terminal/ghostty-main/src/lib_vt.zig:244-259` |
| **Ghostty** | Kitty renderer | `/Users/admin/Code/Github/Terminal/ghostty-main/src/renderer/image.zig:342,469` |
| **Warp** | markdown 替代路线 | `/Users/admin/Code/Github/Terminal/warp-main/crates/warp_terminal/src/model/block_id.rs:4-26` |
| **Tabby** | xterm addon-image | `/Users/admin/Code/Github/Terminal/tabby-master/tabby-terminal/package.json:39` |

### 实现方向

1. 在 `event_loop.rs:572`（`scan_osc133` 邻居）新增 `scan_graphics(data, terminal)`，返回 `Event::GraphicsImage { id, protocol, format, cols, rows, payload, placement }`。
2. 在 `event.rs:157` 附近（`Event::ShellMark` 旁）添加 `Event::GraphicsImage`。
3. 在 `pureterm-alacritty-runtime/src/alacritty/renderer/` 新增 `image_atlas.rs` —— 镜像 `glyph_atlas.rs` LRU + dirty-region 上传逻辑，存 RGBA。
4. `terminal_shader.rs` 扩为同时跑 glyph pass + image quad pass。
5. 新增 `terminal_image_store.rs`：`ImageStore` 持有 (id → ImagePlacement)。
6. `terminal_context.rs:46-87` 把 `TerminalEvent::Image(GraphicsImage)` 转给 `image_store.attach(grid_point)`。
7. `surface_snapshot.rs` `SurfaceSnapshot` 加 `images: Vec<ImagePlacement>` 字段，保跨重启 / instant replay 持久化。

### 验收
- `tests_osc133` 邻居加 `tests_sixel` + `tests_kitty_graphics` 各 ≥6 用例。
- `image_atlas.rs` LRU 测试镜像 `glyph_atlas` 已有 11 个。
- E2E `crates/pureterm/src/graphics_e2e.rs` 真跑 `img2sixel` / `kitty +kitten icat`，快照对比。

---

## 2. 组件 t-core-shaper —— Harfbuzz 字形 shaper

**优先级 P0，估时 M（8–12 d）。**

### PureTerm 现状（审计证据）

**已存在**
- `crates/pureterm-alacritty-runtime/Cargo.toml:9` —— `crossfont = "0.8.1"`；workspace 内无 harfbuzz / rustybuzz / swash / fontdue。
- `crates/pureterm/src/terminal_shader.rs:19` —— `use crossfont::{...}`。
- `:946-955, 980-992` —— `load_font` + `metrics(...).average_advance / line_height / descent`。
- `crates/pureterm-alacritty-runtime/src/alacritty/renderer/glyph_atlas.rs:18-49` —— `CachedGlyph { u,v,u2,v2,x_offset,y_offset,width,height,atlas_kind }`，**无 `x_advance` 字段**。

**核心缺口**
- 无内部 `trait Shaper`，最近抽象是 `crossfont::Rasterize`。
- 无 cluster mapping —— `ime_widget.rs` 和 `bidi.rs` 拿不到 cluster↔cell 反查。
- 无 per-glyph 字体回退链（一个全局 fallback 字体）。
- `x_advance` 不进 GPU pipeline（`terminal_shader.rs:1018-1037` 只读 `x_offset/y_offset/width/height`）。
- alacritty renderer（rect、background）只剩 `glyph_atlas`，其余在 in-tree `terminal_shader.rs`。

### 对标实现

| 来源 | 路径 |
|------|------|
| **Kaku** harfbuzz dep | `/Users/admin/Code/Github/Terminal/Kaku-main/Cargo.toml:133` `harfbuzz = { path = "deps/harfbuzz" }` |
| **Ghostty** harfbuzz dep | `/Users/admin/Code/Github/Terminal/ghostty-main/build.zig.zon:70` |
| **iTerm2** ligature logic | `/Users/admin/Code/Github/Terminal/iTerm2-master/Drawing/iTermAttributedStringBuilder.m:549-679` |
| **Warp** grapheme index | `/Users/admin/Code/Github/Terminal/warp-main/crates/warp_terminal/src/model/grid/flat_storage/index.rs:35,50` |
| **Warp** Mac CoreText | `/Users/admin/Code/Github/Terminal/warp-main/crates/warpui/src/platform/mac/text_layout.rs` |
| **Ghostty** UAX #29 | `/Users/admin/Code/Github/Terminal/ghostty-main/src/unicode/grapheme.zig:5-41` |
| **Tabby** addon-ligatures | `/Users/admin/Code/Github/Terminal/tabby-master/tabby-terminal/package.json:36-44` |

### 实现方向

1. `pureterm-alacritty-runtime/Cargo.toml` 加 `rustybuzz = "1"`（纯 Rust，零外部依赖）。
2. 新建 `crates/pureterm-alacritty-runtime/src/shaper/{mod, rustybuzz_shaper, fallback}.rs`，定义 `trait Shaper` + `ShapedGlyph { glyph_id, cluster, x_advance, y_advance, x_offset, y_offset }`。
3. `glyph_atlas.rs:18-49` `CachedGlyph` 加 `x_advance: f32` 字段。
4. `terminal_shader.rs:19` 替换 `use crossfont::...` 为内部 `Shaper`；`:1018-1037` draw path 把 `x_advance` 推 vertex shader 进 cursor_x。
5. `app_config.rs` 加 `[shaper] { backend = "rustybuzz", fallback_chain }`，默认 monospace → CJK JP → CJK KR → CJK SC → emoji → serif。

### 验收
- `crates/pureterm-alacritty-runtime/tests/shaper_round_trip.rs` 跑 UAX #29。
- 已有 11 atlas 测试 + 新增 6 "advance via vertex buffer"。
- E2E 开 `kitty +kitten unicode-test` 肉眼比对。

---

## 3. 组件 t-core-tmux-cc —— tmux 控制模式

**优先级 P0，估时 L（12–18 d）。**

### PureTerm 现状（审计证据）

**已存在**
- `crates/pureterm/src/tmux_cli.rs:1-962` —— 全子命令 + 30+ 单测。
- `crates/pureterm-daemon/src/pipe_pane.rs` & `wait_for.rs` —— socket 层对应实现。
- `crates/pureterm/src/cli.rs:171-176, 1225-1227` —— `Command::Tmux { cmd }` 派发。
- `crates/pureterm/src/app_config.rs:679-681, 2388-2397, 2422-2449` —— `resume_bindings kind = "tmux"` 是冷 attach，不是 -CC。

**核心缺口**
- 无 `TmuxPty` / `TmuxSession` 后端；`Surface`（`crates/pureterm/src/split_tree.rs:50-55`）三 variant（Local/SSH/Serial）都没有 `TmuxControl`。
- 无 `%begin` / `%end` / `%output` / `%input` envelope 解析。
- pane 状态无外部驱动源 —— 必须 mirror pane state 到 `split_tree`。
- `tmux_cli.rs` 是 Cold CLI（每次 fork socket 调用 GUI），不能持有"一个会话"语义。

### 对标实现

| 来源 | 路径 |
|------|------|
| **Warp** 控制模式状态机 | `/Users/admin/Code/Github/Terminal/warp-main/app/src/terminal/writeable_pty/pty_controller.rs:82-140` |
| **Warp** SSH-tmux wrapper | `/Users/admin/Code/Github/Terminal/warp-main/app/src/lib.rs:1829-1837` |
| **iTerm2** -CC | `/Users/admin/Code/Github/Terminal/iTerm2-master/sources/TmuxController.{h,m}` + `PTYSession.h:848` |
| **Kaku** envelope parser | `/Users/admin/Code/Github/Terminal/Kaku-main/mux/src/tmux.rs` + `crates/wezterm-escape-parser/src/tmux_cc/mod.rs` |
| **Ghostty** | `/Users/admin/Code/Github/Terminal/ghostty-main/src/termio/stream_handler.zig:73-430` |
| **cmux** | `/Users/admin/Code/Github/Terminal/cmux-main/CLI/CMUXCLI+TmuxCompatSupport.swift` |

### 实现方向

1. 新建 `crates/pureterm/src/tmux_cc/` 目录（`envelope.rs` / `pty.rs` / `backend.rs` / `mirror.rs`），结构对齐 Kaku `mux/src/tmux.rs`。
2. `tmux_cc/envelope.rs` —— 解析 8 种核心 envelope：`%begin %0 ...` / `%end` / `%output` / `%input` / `%session_changed` / `%windows` + `error` / `pause` / `subscription-changed`。
3. `tmux_cc/pty.rs` —— 启 `tmux -CC new-session -A` / `attach -t`，持 alacritty `tty::new`，stdout 喂 envelope parser。
4. `tmux_cc/backend.rs` —— `Surface::TmuxControl(TmuxControlPty)` variant，挂 `split_tree.rs:50-55`。
5. `tmux_cc/mirror.rs` —— 把 `%windows` / `%window-add` / `%window-close` / `%window-renamed` / `%layout-change` mirror 到 `split_tree`。
6. `cli.rs:1225-1227` 把 `Command::Tmux` 拆为 `TmuxCli`（已有）+ `TmuxAttach { session, new_window }`。
7. `app_config.rs:679-681` `resume_bindings kind = "tmux"` 改支持 `-CC` attach（兼容回退）。

### 验收
- `tmux_cc/tests/envelope.rs` ≥10 用例。
- `tmux_cc/tests/backend.rs` ≥6 用例。
- 不破坏 `tmux_cli.rs` 30+ 旧测。
- E2E 真启 `tmux -CC`，拉几个窗口肉眼对得上。

---

## 4. 组件 t-core-shell-integ —— Shell integration 覆盖 fish / nu / PowerShell / elvish

**优先级 P1，估时 S（5–8 d）。**

### PureTerm 现状（审计证据）

**已存在**
- `crates/pureterm-daemon/src/shell_integration.rs:1-271` —— `ShellIntegrationOpts { zsh, bash }` + `ZSH_ZSHENV`(52-104, `add-zsh-hook`) + `BASH_PROMPT_COMMAND`(116-156, `DEBUG` trap)。
- `:11` 模块 doc 自承认："Only zsh and bash are covered (fish/nu are a future gap)."
- `crates/pureterm-daemon/src/osc7.rs:1-108` —— OSC 7，仅 zsh+bash。
- `crates/alacritty_terminal/src/event_loop.rs:166-179` 调用点 + `:572 scan_osc133` + `:655-940+ tests_osc133`。
- `crates/alacritty_terminal/src/event.rs:9-44` —— `ShellMarkKind` / `ShellMark`。
- 注入路径：`crates/pureterm/src/terminal_context.rs:307-352`（GUI）/ `crates/pureterm-daemon/src/session.rs:165`（daemon）。
- 注入只用 `tty::Options::env` + 临时 `ZDOTDIR` / `PROMPT_COMMAND`。

**核心缺口**
- 无 fish `fish_prompt` event、无 nu `config.nu` 钩子、无 elvish `edit:after-command`、无 PowerShell `prompt` 函数 override。
- `ShellIntegrationOpts` 是 2 个 bool，应改 enum `Shell`。
- 注入路径在 Windows 上破坏（case-insensitive env + 无 `/tmp`）。
- PowerShell 不读 `PROMPT_COMMAND`，参数语义不同。

### 对标实现

| 来源 | 路径 |
|------|------|
| **Ghostty** 全 5 种 | `/Users/admin/Code/Github/Terminal/ghostty-main/src/shell-integration/{bash,zsh,fish,nushell,elvish}/` |
| **Ghostty** bash | `/Users/admin/Code/Github/Terminal/ghostty-main/src/shell-integration/bash/ghostty.bash:191-323` |
| **Ghostty** bash-preexec | `/Users/admin/Code/Github/Terminal/ghostty-main/src/shell-integration/bash/bash-preexec.sh:1` |
| **Warp** bootstrap | `/Users/admin/Code/Github/Terminal/warp-main/app/assets/bundled/bootstrap/` (含 `pwsh.ps1`, `bash_body.sh`, `zsh_body.sh`) |
| **iTerm2** prompt API | `/Users/admin/Code/Github/Terminal/iTerm2-master/api/library/python/iterm2/iterm2/prompts.py` |
| **iTerm2** Claude hook | `/Users/admin/Code/Github/Terminal/iTerm2-master/sources/ClaudeCode/ClaudeCodeHookEvent.swift` |

### 实现方向

1. `shell_integration.rs:19-22` 把 `ShellIntegrationOpts { zsh, bash }` 改为 `enum Shell` + `HashMap<Shell, PerShellScript>`。
2. 同文件新增 4 个 const 脚本：`FISH_FISH_PREAMBLE` / `NUSHELL_INIT` / `ELVISH_RC_OVERLAY` / `PWSH_FUNCTIONS_DIR`。
3. `:169 prepare_shell_env(pid, opts)` 改 `match opts.shells` 分发。
4. `osc7.rs:51-52` 同样改造，去 "only zsh/bash" 注释。
5. 注入路径 `terminal_context.rs:345-350` + `session.rs:165` 同改。
6. `app_config.rs:328-1037` `ShellIntegrationConfig` 加 `per_shell: HashMap<String, bool>`。
7. 把所有 `/tmp/.pureterm-zdotdir-<tag>` 改为 `std::env::temp_dir().join(".pureterm-...")`。

### 验收
- `shell_integration.rs:200-260` 已有 5 个 + 新增 8 个覆盖 4 个新 shell。
- `osc7.rs:100-107` 已有 1 + 新增 3 个非 zsh/bash 路径。
- E2E `crates/pureterm/tests/{fish,pwsh}_prompt.rs` 真跑子进程比对 `ShellMark`。

---

## 5. 组件 t-core-windows-conpty —— Windows / ConPTY 上线

**优先级 P1，估时 L（10–15 d）。**

### PureTerm 现状（审计证据）

**已存在**
- `crates/alacritty_terminal/src/tty/windows/{mod.rs, child.rs, conpty.rs, blocking.rs}` —— 999 行 ConPTY（`conpty.rs:187` `CreatePseudoConsole`）。
- `crates/alacritty_terminal/Cargo.toml:33-48` —— `cfg(windows)` block + `Win32_System_Console`。
- 几处小 stub（`window_effects` / `print_support` / `ipc/methods` / `agent_hooks` / `settings/config_io`）。

**核心缺口（致命）**
- `crates/pureterm-alacritty-runtime/build.rs:11-17` 引用不存在 `src/alacritty/windows/alacritty.rc`，Windows build 直接失败。
- `crates/pureterm/src/main.rs:73-139` 全是 `#[cfg(unix)]` / `#[cfg(target_os = "macos")]`，无 windows 分支。
- `crates/pureterm/src/{daemon_client, feed_client, resume_bindings, daemon_pty, ssh_cli}.rs` 与 `crates/pureterm-daemon/src/{session, discovery, osc7, shell_integration}.rs` 大量用 `std::os::unix::*` + `UnixStream` + `/tmp/.pureterm-zdotdir-*`。
- Iced + wgpu 本身支持 Windows，所以**窗口能开**，但 IPC + 守护 + SSH + pipe-pane + shell-injection **整体跑不起来**。

### 对标实现

| 来源 | 路径 |
|------|------|
| **alacritty** vendored ConPTY | `crates/alacritty_terminal/src/tty/windows/conpty.rs:107` 起 |
| **Warp** bundled conpty.dll | `/Users/admin/Code/Github/Terminal/warp-main/crates/warp_util/src/assets.rs:8` |
| **Tabby** Win 检测 | `/Users/admin/Code/Github/Terminal/tabby-master/tabby-local/src/components/shellSettingsTab.component.ts:9-18` |
| **Tabby** Win pty | `/Users/admin/Code/Github/Terminal/tabby-master/tabby-electron/src/pty.ts:14` |
| **Nezha** platform 抽象 | `/Users/admin/Code/Github/Terminal/nezha-main/src-tauri/src/platform/{unix,windows,mod}.rs` |
| **Orca** 三平台并列 | `/Users/admin/Code/Github/Terminal/orca-main/native/computer-use-{linux,macos,windows}` |

### 实现方向

1. 修 `pureterm-alacritty-runtime/build.rs:13` 路径或删 `embed_resource` 调。
2. 加 `embed_resource` build-dep。
3. 新增 `crates/pureterm/src/local_socket.rs` 平台抽象（`UnixStream` / `NamedPipeClient`）。
4. 修 `crates/pureterm/src/{daemon_client, feed_client, resume_bindings, daemon_pty, ssh_cli}.rs` + `crates/pureterm-daemon/src/{session, discovery, osc7, shell_integration}.rs` 所有 `UnixStream` 调用。
5. 拆 `main.rs:73-139` `#[cfg(unix)]` 为公用块，加 `#[cfg(target_os = "windows")]` 子模块。
6. ZDOTDIR 路径化（与 t-core-shell-integ 同改）。
7. `terminal_context.rs:307-352` `tty::new` 在 Windows 走 vendored `tty::windows::new`。

### 验收
- `cargo build --target x86_64-pc-windows-msvc` 通过。
- CI 加 windows-2022 runner。
- `crates/pureterm/tests/conpty_smoke.rs` 真启 cmd.exe + PowerShell。
- 已有 Linux 单测不回归。

---

## 6. 组件 t-core-plugin-runtime —— Plugin runtime

**优先级 P2，估时 L（12–18 d）。**

### PureTerm 现状（审计证据）

**已存在**
- `crates/pureterm/src/plugin_system.rs:1-319` —— manifest 校验 / 列出 / install_stub / uninstall。
- `:55-64` `PluginManifest { name, version, description?, author? }`；注释行 `:15` 给出 `provides = [...]` 设想但未进入 struct。
- `crates/pureterm/src/p4_cli.rs:9-21, 116-152` —— `PluginSub { List, Install, Uninstall }`。
- `crates/pureterm/src/app_config.rs:1506-1507` —— `AppConfig::load_plugins` 读 `[plugins]`。
- 9 处候选 hook 点（**全部 EXISTS** 未接到 plugin）：parser（`event_loop.rs:160-200`）/ renderer（`terminal_shader.rs` + `WgpuGlyphAtlas`）/ 命令面板（`natural_language.rs` + `iced_app.rs`）/ IPC（`ipc/methods.rs`）/ status bar（`status_bar/`）/ sidebar（`sidebar/` + `sidebar_dsl.rs`）。

**核心缺口**
- 无 wasmtime / wasmer / libloading / cdylib 依赖。
- 无 `trait Plugin` / `ExtensionPoint` / `register_*` 钩子。
- 无 ABI / `provides` 协议。

### 对标实现

| 来源 | 路径 |
|------|------|
| **Tabby** plugin 生态 | `/Users/admin/Code/Github/Terminal/tabby-master/tabby-core/src/api/{profileProvider, hostApp, hotkeyProvider, hostWindow, toolbarButtonProvider, selector, tabContextMenuProvider, configProvider, fileProvider, mainProcess, menu, platform, theme, tabRecovery, hotkeyProvider}.ts` |
| **Tabby** 加载 | `/Users/admin/Code/Github/Terminal/tabby-master/tabby-electron/src/colorSchemes.ts:10-31` + `platform.service.ts:74-75` |
| **Kaku** Lua API | `/Users/admin/Code/Github/Terminal/Kaku-main/kaku-gui/src/scripting/` + `lua-api-crates/*` |
| **Warp** workflows | `/Users/admin/Code/Github/Terminal/warp-main/crates/integration/src/test/workflows.rs` + `crates/warp-workflows` |
| **iTerm2** status bar API | `/Users/admin/Code/Github/Terminal/iTerm2-master/api/library/python/iterm2/iterm2/statusbar.py` |

### 实现方向

1. `plugin_system.rs:55-64` 扩 `PluginManifest`：`abi_version` + `entry_points: HashMap<String, String>` + `provides: Vec<Provides>` + `config_schema: Option<Schema>`。
2. 引入 `enum Provides { ProfileProvider, HostApp, ToolbarButtonProvider, TabContextMenuProvider, ConfigProvider, StatusBarComponent, SidebarTab, CommandPaletteItem, IpcMethod, TerminalParserHook, RendererOverlay }`。
3. `plugin_system.rs` 新增 `loader.rs`：`wasmtime = "30"` + `wasmtime-wasi` + `runtime/{host_fn}.rs`。
4. 9 处 hook 点全部到位（见上"已存在"清单）。
5. `p4_cli.rs:9-21` `PluginSub` 加 `Enable` / `Disable`。
6. 加 `tests/loader/*.rs` ≥10 个。

### 验收
- 官方 plugins docs 一页 "Authoring PureTerm 插件"。
- ≥2 个示例 plugin：① status bar CPU 监控；② IPC method `plugin.hello(name)`。
- `plugin_system/tests/loader.rs` ≥10 用例。

---

## 7. 依赖关系图

```
        ┌────────────────┐
        │ t-core-graphics│
        └───────┬────────┘
                │
   ┌────────────┼──────────────┐
   ▼            ▼              ▼
┌────────┐ ┌────────────┐ ┌─────────────┐
│t-core- │ │ t-core-    │ │ t-core-     │
│shaper  │ │ tmux-cc    │ │ shell-integ │
│rusty+  │ │            │ │ fish/nu/pwsh│
│cluster │ │            │ └──────┬──────┘
└───┬────┘ └─────┬──────┘        │
    │            │               │
    │            ▼               │
    │     ┌─────────────┐        │
    │     │t-core-win/  │◀───────┘
    │     │ConPTY       │
    │     └──────┬──────┘
    │            │
    ▼            ▼
┌────────────────────┐
│ t-core-plugin-     │
│ runtime            │
└────────────────────┘
```

并行要点：
- **t-core-graphics ‖ t-core-shaper**（graphics 和 shaper 独立 crate，互不影响）。
- **t-core-tmux-cc** 不阻塞 shell-integ，但 t-core-win/ConPTY 走完 t-core-tmux-cc 才知道是不是真 tmux 会话。
- **t-core-shell-integ 必须在 t-core-windows-conpty 之前完成 PowerShell**（pwsh 必须 OSC 133）。
- **t-core-plugin-runtime 依赖所有 P0+P1**（plugin ABI 必须基于稳定 hook surface）。

---

## 8. 推荐落地顺序（4 人 / 12 周）

| 周 | 主题 | 负责 |
|---|---|---|
| W1–W2 | t-core-shaper (Rustybuzz) + t-core-graphics Sixel 部分 | SE-A / SE-B |
| W3–W4 | t-core-graphics (Kitty + iTerm2 inline) | SE-B |
| W3–W5 | t-core-tmux-cc | SE-C |
| W6–W7 | t-core-shell-integ (fish/nu/PowerShell/elvish) | SE-A |
| W6–W9 | t-core-windows-conpty（穿 t-core-tmux-cc 收尾间隙） | SE-D |
| W10–W12 | t-core-plugin-runtime (wasmtime + provides ABI + 2 示例) | 全员 |

---

## 9. 不在本 Roadmap（下轮）

- Reflow on resize 显式可开关
- Semantic zones（iTerm2 独有）
- Cloud shared session（Warp Drive，需要后端）
- Mobile / web runtime（Ghostty/Warp WASM、Orca mobile）
- Computer-use sandbox（已有 GAP-04，需补 Linux/Windows）

---

## 10. 工作量

**总** XL（57–85 人天 / 12 周 4 人）。

| 组件 | 工作量档 |
|---|---|
| t-core-graphics | M (10–14 d) |
| t-core-shaper | M (8–12 d) |
| t-core-tmux-cc | L (12–18 d) |
| t-core-shell-integ | S (5–8 d) |
| t-core-windows-conpty | L (10–15 d) |
| t-core-plugin-runtime | L (12–18 d) |
