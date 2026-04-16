# pureterm/src — 二进制入口源代码

纯 Term 主程序源文件，包含事件循环、平台特定实现和多窗口支持。

## 文件结构

```
src/
├── main.rs              # 主入口、事件循环、PureTermApp
├── quake_mode.rs        # Quake 模式（dropdown terminal）
├── tray.rs              # 系统托盘图标
├── multi_window.rs      # 多窗口管理
├── single_instance.rs   # 单实例锁定
├── global_hotkeys.rs    # 全局快捷键
└── platform/
    ├── mod.rs           # PlatformWindowExt trait
    └── macos.rs         # macOS 平台实现
```

## 模块详情

### main.rs — 主入口

应用程序入口点，负责初始化和事件循环：

```rust
struct PureTermApp {
    window: Option<Arc<Window>>,
    renderer: Option<TerminalRenderer>,
    surface: Option<wgpu::Surface<'static>>,
    app: Option<App>,
    panes: HashMap<u64, TerminalPane>,
    // ...
}
```

**启动流程（`resumed`）：**
1. 创建 winit Window
2. 异步初始化 TerminalRenderer
3. 配置 surface（透明度、圆角）
4. 初始化 EguiBackend
5. 创建初始 TerminalPane

**事件处理（`window_event`）：**
- `CloseRequested` — 退出应用
- `Resized` — 重新配置 surface
- `RedrawRequested` — 渲染终端
- `KeyboardInput` — 键盘事件
- `MouseInput` — 鼠标事件

### quake_mode.rs — Quake 模式

下拉终端模式（类似 Quake 控制台）：

```rust
pub struct QuakeMode {
    visible: bool,
    height_ratio: f32,    // 屏幕高度比例
    animation_progress: f32,
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `toggle()` | 切换显示/隐藏 |
| `show()` | 显示终端 |
| `hide()` | 隐藏终端 |
| `apply_window_attrs(w)` | 应用窗口属性（浮动、顶部） |

### tray.rs — 系统托盘

系统托盘图标支持：

```rust
pub struct TrayManager {
    icon: Option<TrayIcon>,
}
```

### multi_window.rs — 多窗口

多窗口管理（预留功能）：

```rust
pub struct MultiWindowManager {
    windows: HashMap<WindowId, WindowState>,
}
```

### single_instance.rs — 单实例

单实例锁定，防止多次运行：

```rust
pub struct SingleInstance {
    lock_file: PathBuf,
}
```

### global_hotkeys.rs — 全局快捷键

全局快捷键注册（预留功能）。

### platform/ — 平台特定实现

```rust
pub trait PlatformWindowExt {
    fn set_ignore_cursor_events(&self, ignore: bool);
    fn set_vibrancy(&self, style: VibrancyStyle);
}
```

**macOS 实现：**
- `set_vibrancy()` — 设置窗口 vibrancy 效果
- `set_ignore_cursor_events()` — 鼠标穿透

## CLI 命令

```rust
#[derive(Parser)]
struct Cli {
    profile: Option<String>,
    ssh: Option<String>,
    new_tab: bool,
    quake: bool,
    command: Option<Commands>,
}

enum Commands {
    List,        // 列出 profiles
    Settings,   // 打开设置
    ListFonts,  // 列出字体
    ListThemes, // 列出主题
    ShowConfig, // 显示配置
    Version,    // 版本信息
}
```

## 崩溃报告

使用 sentry 实现本地崩溃报告：

```rust
let _guard = sentry::init(sentry::ClientOptions {
    dsn: None,  // 离线模式，不发送网络
    release: Some(env!("CARGO_PKG_VERSION").into()),
    ..Default::default()
});

std::panic::set_hook(Box::new(|panic_info| {
    // 捕获崩溃信息
    sentry::capture_message(&msg, sentry::Level::Error);
}));
```

## 父级导航

← [pureterm/AGENTS.md](../AGENTS.md)