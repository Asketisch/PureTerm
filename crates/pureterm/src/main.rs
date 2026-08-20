mod automation;
mod script_cli;
// `macos_url_scheme` 自带 `#[cfg(not(target_os = "macos"))]` 空实现
//（`register_default_handler` / `consume_pending_url` / `add_open_urls_method`
// 均为 no-op），因此该模块可无条件包含。文件内部用 `#[cfg(target_os = "macos")]`
// 包裹了仅 macOS 使用的 `use objc2::*` 导入。
mod macos_url_scheme;
mod agent_adapters;
mod agent_detect;
#[cfg(all(test, unix))]
mod agent_e2e;
/// GAP-6.6：join-pane 的 Arc-follows 不变量（headless，无 PTY）——
/// 跨模块接缝 `apply_attach_pane` 在分割树与 pane 映射上驱动。
#[cfg(all(test, unix))]
mod join_pane_e2e;
/// GAP-2.1: Workspace 切换「不丢 Surface 状态」不变量(headless,无 PTY)。
#[cfg(all(test, unix))]
mod workspace_e2e;
/// GAP-2.1: detached 键盘路由「焦点指针决定按键落点」不变量(headless,无 GUI)。
#[cfg(all(test, unix))]
mod keyboard_routing_e2e;
/// GAP-3.5: Browser 地址栏 ⌘⇧L/⌘L — spawn-web 插入序列 + confirm load_url
/// 往返(headless,无 PTY/GUI)。
#[cfg(all(test, unix))]
mod browser_address_bar_e2e;
mod agent_hooks;
mod agent_sessions;
/// GAP-01：内置 AI 对话面板——LLM 提供商抽象（Anthropic + OpenAI 兼容）、
/// SSE 流式、Markdown 渲染、可停靠面板。
mod ai_chat;
/// GAP-02：错误恢复——命令失败时的 LLM 修复建议。
mod error_recovery;
/// GAP-04：Computer Use——桌面截图 + 输入桩。
mod computer_use;
/// GAP-08：自然语言 shell 命令（`# description`）。
mod natural_language;
/// GAP-07：灵魂系统——AI 对话的人格文件。
mod soul;
/// GAP-4.2：智能选区悬停预览——提示气泡 + 语义操作。
mod hover_preview;
/// GAP-33：端口扫描器——监听端口侧边栏标签页。
mod port_scanner;
/// GAP-43：远程文件浏览——侧边栏 Files 标签页。
mod remote_files;
/// GAP-06：环境 Agent 本地任务队列 MVP。
mod ambient_agents;
/// GAP-13：向多个 pane 广播键盘输入。
mod broadcast_input;
/// GAP-18：双向文本重排辅助工具。
mod bidi;
/// GAP-26：工作区侧边栏分组。
mod workspace_groups;
/// GAP-30：工具腰带侧边栏面板。
mod toolbelt;
/// GAP-31：Quake 风格下拉终端。
mod quake_dock;
/// GAP-37：SSH 密码管理器集成。
mod password_manager;
/// GAP-39：串口与 Telnet 会话类型。
mod serial_telnet;
/// GAP-42：SSH 登录脚本。
mod login_scripts;
mod app_config;
/// GAP-3.5: 内置 Browser 面板(WebView)— pane 的第二种内容载体。
mod browser;
mod cli;
mod codeindex;
mod config_watcher;
/// GAP-6.7：统一事件结构（Category + AppEvent）。
mod events;
/// GAP-6.7：进程内事件总线（广播 + JSONL 双写）。
mod events_bus;
/// GAP-6.7：事件日志的纯文件 I/O（追加/轮转/读取/tail）。
mod events_log;
/// GAP-32：通过 `gh` CLI 获取 GitHub PR 状态（复用用户已有的 `gh auth`）。
mod gh_pr;
/// T-core-windows-conpty v2：Windows 专属绑定。
#[cfg(target_os = "windows")]
mod windows;
/// T-core-plugin-runtime v2：插件加载器（trait + 静态 + wasm 草图）。
mod plugin_loader;
/// T-core-graphics v3: 后台图像解码 worker(PNG → RGBA)。
/// 主线程把 GraphicsImage 推入 tx,worker 解码后通过 rx 喂给 GPU atlas。
mod image_decode_worker;
/// T-core-graphics v3: image quad pass 单测(Vertex 布局、atlas dirty-region、
/// ImageStore → ImageQuadDraw 端到端管线)。覆盖 Step 4 验收项。
#[cfg(test)]
mod graphics_render_tests;
/// T-core-graphics v3 E2E (checklist A.4):真启 shell 跑 `img2sixel` /
/// 外部参考图形协议发射器。默认 `#[ignore]`,需要外部工具链时手动跑。
#[cfg(all(test, unix))]
mod graphics_e2e;
/// T-core-tmux-cc v3 E2E (checklist C.6):真启 `tmux -CC`,3-window +
/// 跨重启 reattach。默认 `#[ignore]`,需要 tmux 时手动跑。
#[cfg(all(test, unix))]
mod tmux_cc_e2e;

#[cfg(unix)]
mod feed_client;
/// GAP-7.6：只读 feed 审计日志查看器（`pureterm feed list`）。
mod feed_log;
#[cfg(unix)]
mod feed_panel;
#[cfg(unix)]
mod hibernate_panel;
/// GAP-7.6：探测-恢复提示面板状态机（GUI 侧）。
#[cfg(unix)]
mod detect_panel;
#[cfg(unix)]
mod daemon_client;
#[cfg(unix)]
mod daemon_cli;
#[cfg(unix)]
mod daemon_pty;
mod iced_app;
mod ime_widget;
mod ipc;
/// GAP-7.10：MCP 服务器（`pureterm mcp serve`）——以原生 MCP 工具形式向 AI
/// Agent 暴露终端表面。rmcp 通过 `mcp-server` feature 门控；名册/白名单/
/// 配置部分在不启用该 feature 时仍会编译。
mod mcp;
mod keymap;
/// GAP-35：LSP 客户端——rust-analyzer stdio + 诊断。
mod lsp;
mod pane_exit;
mod search;
mod session_restore;
mod split_tree;
/// GAP-19：双栏侧边栏——左侧（Workspace + Git）/ 右侧（Find / Feed / Git）。
mod sidebar;
/// GAP-3.2：可编程侧边栏 DSL（TOML 脚本 + EventBus）。
mod sidebar_dsl;
/// GAP-11：即时回放——events.jsonl 时间线导出 + 拖动查看器。
mod instant_replay;
/// GAP-12：标注与标记——OSC 133 网格标记 + 行注释。
mod annotations;
/// GAP-25：多窗口跨进程 IPC（workspace.switch 扇出）。
mod multi_window_ipc;
mod status_bar;
mod surface_snapshot;
#[cfg(target_os = "macos")]
mod macos_menu;
#[cfg(target_os = "macos")]
mod macos_reopen;
/// GAP-09 / GAP-21：通过 NSDockTile 显示 Dock 徽标（未读 + Feed 待处理）。
#[cfg(target_os = "macos")]
mod macos_dock_badge;
/// GAP-21：Dock 右键菜单——通过 NSMenu 显示最近工作区。
#[cfg(target_os = "macos")]
mod macos_dock_menu;
/// GAP-09：macOS 桌面通知（UNUserNotificationCenter 通知）。
#[cfg(target_os = "macos")]
mod macos_notifications;
#[cfg(target_os = "macos")]
mod macos_eyedropper;
#[cfg(unix)]
mod ssh;
#[cfg(unix)]
mod ssh_cli;
/// GAP-5.2：`pureterm ssh tunnel …`——russh SOCKS5 隧道（守护进程直连）。
#[cfg(unix)]
mod tunnel_cli;
/// GAP-7.7：自定义恢复绑定（`pureterm surface resume ...`）。
#[cfg(unix)]
mod resume_bindings;
/// GAP-6.6：`pureterm tmux …`——与 tmux 兼容的命令子集。
mod tmux_cli;
/// T-core-tmux-cc：tmux 控制模式（-CC）后端——信封解析器 +
/// PTY 骨架 + Surface 变体 + 镜像。
mod tmux_cc;
/// T-core-windows-conpty：跨平台本地 socket 抽象。
mod local_socket;
/// GAP-32：Git 浮层面板状态 + 快捷操作枚举（视图位于 iced_app）。
mod git_panel;
/// GAP-09：内存通知存储（历史 + 未读 + 关注 + 铃声节流）。
mod notification_store;
/// GAP-09：通知浮层面板状态（视图位于 iced_app）。
mod notification_panel;
/// GAP-15：每 pane 的 shell 集成状态（OSC 133 A/B/C/D 状态机）。
mod shell_state;
/// T-core-shaper v2：字形整形配置（后端、回退链）。
mod shaper_config;
/// GAP-51：基于块的输出脚手架（数据模型 + feature 开关）。
mod output_blocks;
/// GAP-10：模式匹配触发器（对输出做正则匹配 → 高亮/通知/发送文本/执行）。
mod triggers;
/// GAP-15：面向 AI 的 shell 上下文构建器（MCP get_shell_context + 未来 AI 面板）。
mod shell_context;
mod terminal_context;
mod terminal_shader;
mod vi_mode_handler;
/// GAP-05：语音输入（一键通话脚手架）。
mod voice_input;
/// GAP-17：Sixel / 分块图形协议脚手架。
mod terminal_graphics;
/// GAP-20：macOS 玻璃 / 活力效果。
// 函数体已做到跨平台可移植：`apply_glass_effect` 在非 macOS 上为空操作。
mod macos_glass_effect;
/// GAP-22：MacBook Touch Bar 脚手架。
// 函数体已做到跨平台可移植：`refresh_touch_bar` 在非 macOS 上为空操作。
mod macos_touch_bar;
/// GAP-23：可视标签栏配置。
mod visual_tab_bar;
/// GAP-28：自定义窗口边框。
mod window_border;
/// GAP-29：环境徽章浮层。
mod badge_overlay;
/// GAP-34：文件预览（图片/PDF/文本）。
mod file_preview;
/// GAP-38：SCP/SFTP 文件传输。
mod file_transfer;
/// GAP-44：共享会话（占位实现）。
mod shared_sessions;
/// GAP-45：自动更新检查脚手架。
mod auto_update;
/// GAP-49：插件系统（占位实现）。
mod plugin_system;
/// GAP-50：云同步（本地包 MVP）。
mod cloud_sync;
/// GAP-52：参数化工作流。
mod workflows;
/// GAP-53：交互式笔记本。
mod notebooks;
/// GAP-54：背景图片配置。
mod background_image;
/// GAP-55：窗口透明 + 模糊。
mod window_effects;
/// GAP-56：随鼠标聚焦。
mod focus_follows_mouse;
/// GAP-57：MacBook 触觉反馈。
// 函数体已做到跨平台可移植：`perform` 在非 macOS 上返回 `Skipped`。
mod macos_haptic;
/// GAP-58：扩展主题覆盖。
mod custom_theme;
/// GAP-59：Emoji 渲染通道。
mod emoji_rendering;
/// GAP-60：Pane 截图 / 导出。
mod pane_capture;
/// GAP-61：打印支持。
mod print_support;
/// GAP-4.6：VoiceOver / 无障碍播报。
mod accessibility;
/// P4 GAP CLI 辅助命令（plugin、sync、share 等）。
mod p4_cli;
/// 供 iced_app 使用的 P4 GAP 运行时包。
mod p4_runtime;

fn main() -> iced::Result {
    env_logger::init();

    // GAP-6.2：结构化 CLI 命令分组（clap derive）。裸运行 `pureterm`（无参数）
    // 启动 GUI；任何参数都会派发到 CLI。
    if std::env::args().count() > 1 {
        cli::run(cli::parse_from_env());
        std::process::exit(0);
    }

    ensure_default_config();
    iced_app::run()
}

// ── 键盘快捷键辅助函数(由 cli.rs Shortcuts 组调用) ─────────────────────────────────────

pub(crate) fn shortcuts_list() {
    use iced::keyboard::Modifiers;

    let km = keymap::Keymap::from_defaults();
    for (mods, bk, action) in km.list_bindings() {
        let mods_str = format_mods(mods);
        let key_str = format_binding_key(&bk);
        let action_str = format_action(&action);
        println!("{:>20} {:>12} → {}", mods_str, key_str, action_str);
    }
}

pub(crate) fn shortcuts_preset(name: &str) {
    let content = match name {
        "default" => include_str!("../../../resources/presets/default.toml").to_string(),
        "vim" => include_str!("../../../resources/presets/vim.toml").to_string(),
        "tmux" => include_str!("../../../resources/presets/tmux.toml").to_string(),
        "macos" => include_str!("../../../resources/presets/macos.toml").to_string(),
        "vscode" => include_str!("../../../resources/presets/vscode.toml").to_string(),
        "agent-triage" => include_str!("../../../resources/presets/agent-triage.toml").to_string(),
        _ => {
            eprintln!("Unknown preset: {name}");
            eprintln!("Available: default, vim, tmux, macos, vscode, agent-triage");
            std::process::exit(1);
        }
    };

    // 将预设作为 [[keyboard.bindings]] 写入配置文件
    let config_path = app_config::AppConfig::config_path();
    write_keybindings_to_config(&config_path, &content);
    eprintln!("Applied preset '{name}' to {}", config_path.display());
    eprintln!("If PureTerm is running, keybindings will reload automatically.");
}

pub(crate) fn shortcuts_reset() {
    let config_path = app_config::AppConfig::config_path();
    write_keybindings_to_config(&config_path, "");
    eprintln!("Reset keybindings to defaults in {}", config_path.display());
}

/// 将 [[keyboard.bindings]] 段落写入配置文件。
/// 使用 toml_edit 以保留配置的其余部分。
fn write_keybindings_to_config(config_path: &std::path::Path, preset_content: &str) {
    use pureterm_settings::config_io;

    if let Err(e) = config_io::mutate_config(config_path, |doc| {
        // 移除已有的键盘配置段
        doc.remove("keyboard");

        // 解析预设 TOML 并插入绑定
        if !preset_content.is_empty() {
            if let Ok(preset_doc) = preset_content.parse::<toml_edit::DocumentMut>() {
                if let Some(kb) = preset_doc.get("keyboard") {
                    doc.insert("keyboard", kb.clone());
                }
            }
        }
        Ok(())
    }) {
        eprintln!("Error writing config: {e}");
        std::process::exit(1);
    }
}

// ── 格式化辅助函数 ─────────────────────────────────────────────────────────────

fn format_mods(mods: iced::keyboard::Modifiers) -> String {
    let mut parts = Vec::new();
    if mods.contains(iced::keyboard::Modifiers::COMMAND) {
        parts.push("⌘");
    }
    if mods.contains(iced::keyboard::Modifiers::SHIFT) {
        parts.push("⇧");
    }
    if mods.contains(iced::keyboard::Modifiers::ALT) {
        parts.push("⌥");
    }
    if mods.contains(iced::keyboard::Modifiers::CTRL) {
        parts.push("⌃");
    }
    parts.join("")
}

fn format_binding_key(bk: &keymap::BindingKey) -> String {
    match bk {
        keymap::BindingKey::Character(s) => s.clone(),
        keymap::BindingKey::Named(nk) => match nk {
            keymap::BindingNamedKey::ArrowUp => "↑".into(),
            keymap::BindingNamedKey::ArrowDown => "↓".into(),
            keymap::BindingNamedKey::ArrowLeft => "←".into(),
            keymap::BindingNamedKey::ArrowRight => "→".into(),
            keymap::BindingNamedKey::Enter => "↵".into(),
            keymap::BindingNamedKey::Escape => "⎋".into(),
            keymap::BindingNamedKey::Tab => "⇥".into(),
            keymap::BindingNamedKey::Backspace => "⌫".into(),
            keymap::BindingNamedKey::Space => "Space".into(),
            keymap::BindingNamedKey::F(n) => format!("F{n}"),
            other => format!("{:?}", other),
        },
    }
}

fn format_action(action: &keymap::Action) -> String {
    match action {
        keymap::Action::NewTab => "New Tab".into(),
        keymap::Action::CloseTab => "Close Tab".into(),
        keymap::Action::ClosePaneOrTab => "Close Pane/Tab".into(),
        keymap::Action::NextTab => "Next Tab".into(),
        keymap::Action::PrevTab => "Prev Tab".into(),
        keymap::Action::ActivateTabByIndex(i) => format!("Tab {}", i + 1),
        keymap::Action::MovePaneToNewTab => "Pane → New Tab".into(),
        keymap::Action::MovePaneToNewWindow => "Pane → New Window".into(),
        keymap::Action::NewWindow => "New Window".into(),
        keymap::Action::NewWorkspace => "New Workspace".into(),
        keymap::Action::NextWorkspace => "Next Workspace".into(),
        keymap::Action::PrevWorkspace => "Previous Workspace".into(),
        keymap::Action::ActivateWorkspaceByIndex(i) => format!("Workspace {}", i + 1),
        keymap::Action::SplitHorizontal => "Split Horizontal".into(),
        keymap::Action::SplitVertical => "Split Vertical".into(),
        keymap::Action::SplitWeb => "Split Browser Pane".into(),
        keymap::Action::EqualizeSplits => "Equalize Splits".into(),
        keymap::Action::SwapPane => "Swap Pane".into(),
        keymap::Action::ToggleZoom => "Toggle Zoom".into(),
        keymap::Action::FocusPaneInOrder(d) => {
            if *d > 0 {
                "Focus Next Pane".into()
            } else {
                "Focus Prev Pane".into()
            }
        }
        keymap::Action::PaneFocusAdjacent(dir) => {
            format!("Focus Adjacent ({:?})", dir)
        }
        keymap::Action::ClipboardPaste => "Paste".into(),
        keymap::Action::ClipboardCopy => "Copy".into(),
        keymap::Action::SelectAll => "Select All".into(),
        keymap::Action::OpenSshDialog => "SSH Dialog".into(),
        keymap::Action::OpenCommandPalette => "Command Palette".into(),
        keymap::Action::OpenSettings => "Settings".into(),
        keymap::Action::FocusAddressBar => "Browser Address Bar".into(),
        keymap::Action::ToggleGitPanel => "Git Panel".into(),
        keymap::Action::ToggleLeftSidebar => "Left Sidebar".into(),
        keymap::Action::ToggleRightSidebar => "Right Sidebar".into(),
        keymap::Action::SidebarRightMode(m) => format!("Sidebar: {}", m.label()),
        keymap::Action::ToggleNotificationsPanel => "Notifications Panel".into(),
        keymap::Action::ErrorRecoveryToggle => "Error Recovery".into(),
        keymap::Action::ToggleAiChatPanel => "AI Chat Panel".into(),
        keymap::Action::ToggleInstantReplay => "Instant Replay".into(),
        keymap::Action::PrevMark => "Previous Mark".into(),
        keymap::Action::NextMark => "Next Mark".into(),
        keymap::Action::OpenHoverTarget => "Open URL/Path".into(),
        keymap::Action::OpenSemanticInEditor => "Open in Editor".into(),
        keymap::Action::PreviewFile => "Preview File".into(),
        keymap::Action::OpenLazygit => "Open Lazygit".into(),
        keymap::Action::IncreaseFontSize => "Font +".into(),
        keymap::Action::DecreaseFontSize => "Font −".into(),
        keymap::Action::ResetFontSize => "Font Reset".into(),
        keymap::Action::ToggleFullscreen => "Fullscreen".into(),
        keymap::Action::SpawnNewInstance => "New Instance".into(),
        keymap::Action::ToggleViMode => "Toggle Vi/Copy Mode".into(),
        keymap::Action::SearchForward => "Find".into(),
        keymap::Action::SearchInDirectory => "Find in Directory".into(),
        keymap::Action::SearchNext => "Find Next".into(),
        keymap::Action::SearchPrev => "Find Previous".into(),
        keymap::Action::SearchClose => "Close Find".into(),
        keymap::Action::SearchToggleCase => "Toggle Case-Sensitive".into(),
        keymap::Action::SearchToggleRegex => "Toggle Regex".into(),
        keymap::Action::SearchToggleWholeWord => "Toggle Whole Word".into(),
        keymap::Action::SearchToggleFollow => "Toggle Follow Tail".into(),
        keymap::Action::ReplayScrub(d) => format!("Replay Scrub ({d})"),
        keymap::Action::ToggleBroadcastInput => "Broadcast Input".into(),
        keymap::Action::ToggleQuakeDock => "Quake Dock".into(),
        keymap::Action::ToggleToolbelt => "Toolbelt".into(),
        // GAP-6.4：用户自定义命令不属于预设集合，
        // 但为保证 `shortcuts` CLI 的匹配穷尽性，仍需处理。
        keymap::Action::CustomCommand(id) => format!("Custom: {id}"),
    }
}

// ── 配置辅助函数 ──────────────────────────────────────────────────────────────

fn ensure_default_config() {
    let config_dir = pureterm_config_dir();
    let config_file = config_dir.join("pureterm.toml");

    if config_file.exists() {
        return;
    }
    if config_dir.join("pureterm.toml").exists() {
        return;
    }
    if alacritty_config_exists() {
        return;
    }

    if let Err(e) = std::fs::create_dir_all(&config_dir) {
        eprintln!("PureTerm: unable to create config dir {:?}: {e}", config_dir);
        return;
    }

    let default_config = include_str!("../default-config.toml");
    if let Err(e) = std::fs::write(&config_file, default_config) {
        eprintln!("PureTerm: unable to write default config {:?}: {e}", config_file);
    }
}

#[cfg(target_os = "macos")]
fn pureterm_config_dir() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_default().join(".config/pureterm")
}

#[cfg(not(target_os = "macos"))]
fn pureterm_config_dir() -> std::path::PathBuf {
    xdg::BaseDirectories::with_prefix("pureterm")
        .get_config_home()
        .unwrap_or_else(|_| std::path::PathBuf::from(".config/pureterm"))
}

#[cfg(target_os = "macos")]
fn alacritty_config_exists() -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".config/alacritty/alacritty.toml").exists()
        || home.join(".config/alacritty/alacritty.yml").exists()
        || home.join(".alacritty.toml").exists()
        || home.join(".alacritty.yml").exists()
}

#[cfg(not(target_os = "macos"))]
fn alacritty_config_exists() -> bool {
    xdg::BaseDirectories::with_prefix("alacritty")
        .find_config_file("alacritty.toml")
        .is_some()
}
