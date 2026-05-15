mod accessibility;
mod app;
mod cli;
mod event_handler;
mod key_input;
mod overlay;
mod pane_ops;
mod platform;
mod quake_mode;
mod tab_ops;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use clap::Parser;
use pureterm_core::config::{AppearanceConfig, ConfigService};
use pureterm_core::prevent_sleep::PreventSleep;
use pureterm_core::theme::ThemeService;
use pureterm_gui::EguiBackend;
use pureterm_session::base::ConnectParams;
use pureterm_ui::app::App;
use pureterm_ui::command_palette::CommandPalette;
use pureterm_ui::context_menu::ContextMenuState;
use pureterm_ui::profile_selector::ProfileSelector;
use pureterm_ui::settings::SettingsPanel;
use pureterm_ui::split::SplitContainer;
use pureterm_ui::tab_bar::TabBarState;
use pureterm_ui::tabs::TabContent;
use pureterm_ui::WelcomePage;
use winit::keyboard::ModifiersState;

use app::PureTermApp;
use quake_mode::QuakeMode;

fn main() -> Result<()> {
    #[cfg(debug_assertions)]
    let default_level = "debug";
    #[cfg(not(debug_assertions))]
    let default_level = "warn";

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(default_level)
    ).init();

    // 初始化崩溃报告（仅本地存储，不发送到网络）
    // 使用本地崩溃目录
    let crash_dir = std::env::temp_dir().join("pureterm-crashes");
    std::fs::create_dir_all(&crash_dir).ok();

    // 初始化 sentry - 空 DSN 禁用网络传输
    let _guard = sentry::init(sentry::ClientOptions {
        dsn: None,
        release: Some(env!("CARGO_PKG_VERSION").into()),
        ..Default::default()
    });
    // 设置 panic 钩子以捕获崩溃
    std::panic::set_hook(Box::new(|panic_info| {
        let payload = panic_info.payload();
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };
        let location = panic_info.location().map(|loc| {
            format!("{}:{}:{}", loc.file(), loc.line(), loc.column())
        }).unwrap_or_else(|| "unknown".to_string());
        eprintln!("PANIC: {} at {}", message, location);
        sentry::capture_message(&format!("Panic: {} at {}", message, location), sentry::Level::Error);
    }));

    log::info!("PureTerm starting... (build: {})", if cfg!(debug_assertions) { "debug" } else { "release" });

    let cli = cli::Cli::parse();

    // 处理子命令
    match cli.command {
        Some(cli::Commands::List) => {
            println!("Available profiles:");
            // 在这里加载并列出配置文件
            println!("  (use --profile to connect)");
            return Ok(());
        }
        Some(cli::Commands::Settings) => {
            println!("Opening settings...");
            // 在这里触发设置打开
            return Ok(());
        }
        Some(cli::Commands::ListFonts) => {
            println!("Available fonts:");
            println!("  JetBrainsMono Nerd Font (embedded)");
            println!("  (custom fonts can be configured in settings)"); // (custom fonts can be configured in settings)
            return Ok(());
        }
        Some(cli::Commands::ListThemes) => {
            use pureterm_core::theme::builtin_color_schemes;
            println!("Available themes:");
            for scheme in builtin_color_schemes() {
                println!("  {}", scheme.name);
            }
            return Ok(());
        }
        Some(cli::Commands::ShowConfig) => {
            let config_service = ConfigService::load()?;
            let config = config_service.get();
            println!("Current configuration:");
            println!("{}", serde_json::to_string_pretty(&*config).unwrap_or_default());
            return Ok(());
        }
        Some(cli::Commands::Version) => {
            println!("PureTerm {}", env!("CARGO_PKG_VERSION"));
            println!("Built with: Rust, wgpu, egui");
            println!("Features: GPU-accelerated rendering, ligatures, unicode box-drawing");
            return Ok(());
        }
        None => {}
    }

    let config_service = ConfigService::load()?;
    let mut theme_service = ThemeService::new();
    let mut config = config_service.get().clone();

    // 处理快速 SSH 连接
    if let Some(ssh_spec) = cli.ssh {
        if let Some(profile) = cli::parse_ssh_spec(&ssh_spec) {
            config.profiles.push(profile);
        }
    }

    let color_scheme = config.terminal.color_scheme.clone()
        .or_else(|| theme_service.get_scheme(&config.appearance.theme).cloned())
        .unwrap_or_else(|| theme_service.default_scheme().clone());

    // 加载用户主题
    let themes_dir = pureterm_core::config::AppConfig::config_dir().join("themes");
    theme_service.load_user_themes(&themes_dir);

    let event_loop = winit::event_loop::EventLoop::new()?;
    let proxy = event_loop.create_proxy();
    let quake_mode = cli.quake;

    let mut trigger_svc = pureterm_core::trigger::TriggerService::new();
    trigger_svc.replace_all(config.triggers.clone());
    let trigger_service = std::sync::Arc::new(std::sync::RwLock::new(trigger_svc));

    let auto_profile_switcher = std::sync::Arc::new(std::sync::RwLock::new(
        pureterm_core::auto_profile::AutoProfileSwitcher::new(config.auto_profile_rules.clone()),
    ));

    let smart_selection = {
        let mut svc = pureterm_core::smart_selection::SmartSelectionService::new();
        let rules = if config.smart_selection_rules.is_empty() {
            pureterm_core::smart_selection::SmartSelectionService::default_rules()
        } else {
            config.smart_selection_rules.clone()
        };
        svc.replace_rules(rules);
        std::sync::Arc::new(svc)
    };

    let mut app = PureTermApp {
        window: None,
        renderer: None,
        surface: None,
        app: None,
        panes: HashMap::new(),
        split_root: None,
        focused_pane_id: None,
        context_menu: ContextMenuState::new(),
        color_scheme,
        modifiers: ModifiersState::default(),
        mouse_pos: (0.0, 0.0),
        clipboard_text: String::new(),
        divider_drag: None,
        hovered_divider: None,
        egui_backend: None,
        settings_panel: SettingsPanel::default(),
        command_palette: CommandPalette::new(),
        profile_selector: ProfileSelector::default(),
        welcome_page: WelcomePage::new(),
        rename_modal: None,
        color_modal: None,
        quake_mode: if quake_mode { QuakeMode::new() } else { QuakeMode::default() },
        tab_bar: TabBarState::new(),
        traffic_light_w: 0.0,
        appearance: AppearanceConfig::default(),
        proxy,
        theme_service,
        prevent_sleep: PreventSleep::new(),
        sleep_prevented: false,
        pending_marks: Arc::new(Mutex::new(Vec::new())),
        trigger_service,
        auto_profile_switcher,
        smart_selection,
        quit_requested: false,
        should_quit: false,
        last_tab_click_time: None,
        last_tab_click_id: None,
        status_bar: pureterm_ui::status_bar::StatusBar::default_layout(),
        toolbelt: pureterm_ui::toolbelt::ToolbeltState::new(
            pureterm_ui::toolbelt::ToolbeltConfig {
                visible: config.toolbelt_visible,
                ..Default::default()
            },
        ),
    };

    event_loop.run_app(&mut app)?;

    log::info!("PureTerm shutting down");
    Ok(())
}
