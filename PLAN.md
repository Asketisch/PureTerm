# PureTerm: 纯 Rust 终端模拟器 - 完整技术规划

> 使用纯 Rust 完整重写 Tabby 终端模拟器
> 参考项目: Alacritty, WezTerm, Rio

---

## 目录

1. [项目概述](#1-项目概述)
2. [技术选型](#2-技术选型)
3. [架构总览](#3-架构总览)
4. [Workspace 结构](#4-workspace-结构)
5. [核心模块详细设计](#5-核心模块详细设计)
6. [终端模拟器实现](#6-终端模拟器实现)
7. [会话层实现](#7-会话层实现)
8. [UI 层实现](#8-ui-层实现)
9. [分屏与标签页系统](#9-分屏与标签页系统)
10. [配置系统](#10-配置系统)
11. [主题系统](#11-主题系统)
12. [快捷键系统](#12-快捷键系统)
13. [插件系统](#13-插件系统)
14. [平台集成](#14-平台集成)
15. [功能对照表](#15-功能对照表)
16. [依赖清单](#16-依赖清单)
17. [文件结构](#17-文件结构)
18. [开发阶段规划](#18-开发阶段规划)
19. [风险与备选方案](#19-风险与备选方案)

---

## 1. 项目概述

### 1.1 目标

使用纯 Rust 重写 Tabby 终端模拟器，保留其全部核心功能，实现跨平台 (macOS/Linux/Windows) 运行。

### 1.2 Tabby 原项目分析

Tabby 是一个高度可配置的终端模拟器、SSH 和串口客户端，核心功能包括：

| 类别 | 功能 |
|------|------|
| **终端** | 多标签页、嵌套分屏、搜索、滚动历史、字体连字 |
| **连接** | 本地 Shell、SSH、串口、Telnet |
| **SSH** | X11/端口转发、Agent 转发、SFTP、会话复用 |
| **体验** | 主题、快捷键、插件系统、标签页恢复 |

**Tabby 原技术栈**:
- Electron (桌面 shell)
- Angular + TypeScript (前端)
- xterm.js (终端渲染)
- node-pty (本地 Shell)
- russh (SSH - 已是 Rust!)
- serialport (串口 - 已是 Rust!)

### 1.3 核心理念

1. **纯 Rust**: 100% Rust 实现，无外部运行时依赖
2. **性能优先**: GPU 加速渲染，一次绘制整个屏幕
3. **正确性**: 严格遵循 VT100/xterm 规范
4. **模块化**: 清晰的层次划分，可测试，可复用
5. **跨平台**: macOS/Linux/Windows 一套代码

---

## 2. 技术选型

### 2.1 核心技术栈

| 层级 | 组件 | 选型 | 参考 |
|------|------|------|------|
| **窗口/事件** | `winit` | 必选 | 跨平台窗口管理，事实标准 |
| **终端解析** | `vte` | 必选 | Alacritty/Rio 在用，成熟稳定 |
| **GPU 渲染** | `wgpu` | 必选 | WezTerm/Rio 在用，跨平台 |
| **字体渲染** | `fontdue` + `freetype` | 必选 | WezTerm 在用，连字支持 |
| **本地 Shell** | `portable-pty` | 必选 | 跨平台 PTY，macOS/Linux/Win |
| **SSH** | `russh` | 必选 | 成熟 SSH 客户端，纯 Rust |
| **串口** | `serialport` | 必选 | 纯 Rust 串口库 |
| **UI 框架** | `egui` | 必选 | 配合 wgpu，成熟度高 |
| **异步运行时** | `tokio` | 必选 | 异步 IO，SSH/串口需要 |
| **配置格式** | `TOML` + `serde` | 必选 | 简单、主流 |

### 2.2 关键设计决策

**Q: 为什么用 wgpu 而不是 raw OpenGL?**
- wgpu 是跨平台的 (Vulkan/Metal/DX12)
- 安全性更好 (不直接操作 GPU 指针)
- 维护性更好，Rio 和 WezTerm 都在用

**Q: 为什么用 egui 而不是 iced/druid?**
- egui 开发活跃
- 与 wgpu 集成良好 (egui-wgpu)
- 声明式 API 简单

**Q: 为什么 Session 用 async_trait?**
- SSH 和串口操作是异步的
- tokio 是事实标准
- 便于并发管理

**Q: 为什么配置用 TOML 而不是 YAML?**
- TOML 更简单
- serde 支持好
- 无缩进问题

---

## 3. 架构总览

### 3.1 整体架构

```
┌─────────────────────────────────────────────────────────────────────┐
│                         PureTerm                                     │
│                    (Workspace Root)                                  │
├─────────────────────────────────────────────────────────────────────┤
│  ┌──────────────┐     ┌──────────────────────────────────────────┐ │
│  │   pureterm   │     │              pureterm-gui                │ │
│  │  (主程序入口) │     │     (egui + wgpu UI / TerminalWidget)    │ │
│  └──────┬───────┘     └──────────────────┬───────────────────────┘ │
│         │                                │                          │
│         │         ┌──────────────────────┴───────────────────────┐ │
│         │         │              pureterm-ui                      │ │
│         │         │  ┌─────────┐ ┌─────────┐ ┌─────────────────┐   │ │
│         │         │  │TabManager│ │SplitPane│ │TerminalWidget   │   │ │
│         │         │  └─────────┘ └─────────┘ └─────────────────┘   │ │
│         │         │  ┌─────────────────────────────────────────┐   │ │
│         │         │  │           TerminalPane                   │   │ │
│         │         │  │  (终端渲染区: wgpu + fontdue)          │   │ │
│         │         │  └─────────────────────────────────────────┘   │ │
│         │         └──────────────────┬──────────────────────────────┘ │
│         │                            │                                │
│         │         ┌──────────────────┴──────────────────────────────┐│
│         │         │              pureterm-terminal                   ││
│         │         │  ┌──────────┐ ┌──────────┐ ┌───────────────┐     ││
│         │         │  │VteParser │ │ Screen   │ │ EscapeSeq    │     ││
│         │         │  │ (解析)    │ │ (缓冲区) │ │ (转义序列)   │     ││
│         │         │  └──────────┘ └──────────┘ └───────────────┘     ││
│         │         └──────────────────┬──────────────────────────────┘ │
│         │                            │                                │
│         │         ┌──────────────────┴──────────────────────────────┐│
│         │         │              pureterm-session                   ││
│         │         │  ┌──────────┐ ┌──────────┐ ┌──────────┐        ││
│         │         │  │LocalSess │ │ SSHSess  │ │SerialSess│        ││
│         │         │  └──────────┘ └──────────┘ └──────────┘        ││
│         │         └──────────────────┬──────────────────────────────┘ │
│         │                            │                                │
│         │         ┌──────────────────┴──────────────────────────────┐│
│         │         │               pureterm-core                     ││
│         │         │  ┌────────┐ ┌────────┐ ┌────────┐ ┌─────────┐   ││
│         │         │  │ Config │ │ Profile│ │ Theme  │ │ Hotkey  │   ││
│         │         │  └────────┘ └────────┘ └────────┘ └─────────┘   ││
│         │         └──────────────────────────────────────────────────┘ │
│  ┌──────┴────────────────────────────────────────────────────────────┐│
│  │                     外部 crates                                   │ │
│  │  winit │ wgpu │ vte │ russh │ serialport │ portable-pty │ fontdue│ │
│  └───────────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────────┘
```

### 3.2 数据流

```
用户输入 → winit 事件 → TerminalPane.input()
    ↓
TerminalPane 发送按键到 Session
    ↓
Session (Local/SSH/Serial) 产生输出数据
    ↓
VteParser 解析字节流
    ↓
Screen 更新单元格状态
    ↓
TerminalPane.request_render() 请求重绘
    ↓
wgpu 渲染整个屏幕 (一次绘制)
```

---

## 4. Workspace 结构

```
PureTerm/
├── Cargo.toml                    # Workspace 根配置
├── README.md
├── PLAN.md                       # 本文档
│
├── crates/
│   ├── pureterm/                 # 主程序入口 crate
│   ├── pureterm-core/            # 核心服务 (无 UI 依赖)
│   ├── pureterm-terminal/        # 终端模拟器核心
│   ├── pureterm-session/         # 会话管理
│   ├── pureterm-ui/              # UI 层 (egui)
│   └── pureterm-gui/             # wgpu 渲染集成
│
└── assets/                       # 资源文件
```

---

## 5. 核心模块详细设计

### 5.1 pureterm-core::config

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub appearance: AppearanceConfig,
    pub terminal: TerminalConfig,
    pub hotkeys: HotkeysConfig,
    pub profiles: Vec<Profile>,
    pub groups: Vec<ProfileGroup>,
}

pub struct ConfigService {
    config_dir: PathBuf,
    config: RwLock<AppConfig>,
}

impl ConfigService {
    pub fn load() -> Result<Self>;
    pub fn save(&self) -> Result<()>;
    pub fn get(&self) -> Guard<AppConfig>;
    pub fn update<F>(&self, f: F);
    pub fn config_path() -> PathBuf;
}
```

### 5.2 pureterm-core::profile

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProfileType {
    Ssh, Serial, Local, Telnet,
}

pub trait ProfileProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn create_tab_params(&self, profile: &Profile) -> Result<TabParams>;
    fn get_description(&self, profile: &Profile) -> String;
    fn quick_connect(&self, query: &str) -> Result<Profile>;
}

pub struct ProfileManager {
    providers: HashMap<&'static str, Box<dyn ProfileProvider>>,
    profiles: RwLock<Vec<Profile>>,
    groups: RwLock<Vec<ProfileGroup>>,
}
```

### 5.3 pureterm-core::theme

```rust
pub const BUILTIN_COLOR_SCHEMES: &[(&str, ColorScheme)] = &[
    ("Monokai Pro", MONOKAI_PRO),
    ("Monokai", MONOKAI),
    ("Solarized Dark", SOLARIZED_DARK),
    ("Solarized Light", SOLARIZED_LIGHT),
    ("Dracula", DRACULA),
    ("One Dark", ONE_DARK),
    ("GitHub Dark", GITHUB_DARK),
    ("Nord", NORD),
    ("Catppuccin Mocha", CATPPUCCIN_MOCHA),
];
```

### 5.4 pureterm-core::hotkey

```rust
pub const BUILTIN_HOTKEYS: &[(&str, Hotkey)] = &[
    ("tab_new", Hotkey { id: "tab_new", name: "New Tab", ... }),
    ("tab_close", Hotkey { id: "tab_close", name: "Close Tab", ... }),
    ("copy", Hotkey { id: "copy", name: "Copy", ... }),
    ("paste", Hotkey { id: "paste", name: "Paste", ... }),
    ("search", Hotkey { id: "search", name: "Search", ... }),
    ("split_vertical", Hotkey { id: "split_vertical", name: "Split Vertically", ... }),
    ("split_horizontal", Hotkey { id: "split_horizontal", name: "Split Horizontally", ... }),
];
```

---

## 6. 终端模拟器实现

### 6.1 终端解析 (VTE)

```rust
pub struct VteParser {
    parser: Parser,
    perform: VtePerform,
}

impl VteParser {
    pub fn new(screen: Rc<RefCell<Screen>>) -> Self;
    pub fn parse(&mut self, data: &[u8]);
    pub fn parse_str(&mut self, s: &str);
}
```

### 6.2 屏幕缓冲区

```rust
#[derive(Debug, Clone)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: CellAttrs,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Color {
    Named(NamedColor),
    Palette(u8),
    Rgb(u8, u8, u8),
}

pub struct Screen {
    pub grid: Grid<Cell>,
    pub cursor: Cursor,
}

impl Screen {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self;
    pub fn resize(&mut self, cols: usize, rows: usize);
    pub fn put_char(&mut self, c: char);
    pub fn execute(&mut self, c: char);
}
```

### 6.3 终端主结构

```rust
pub struct Terminal {
    pub screen: Screen,
    parser: VteParser,
    dirty: bool,
    title: String,
    current_dir: Option<String>,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self;
    pub fn write(&mut self, data: &[u8]);
    pub fn resize(&mut self, cols: usize, rows: usize);
    pub fn is_dirty(&self) -> bool;
    pub fn snapshot(&self) -> ScreenSnapshot;
}
```

---

## 7. 会话层实现

### 7.1 Session Trait

```rust
#[async_trait]
pub trait Session: Send {
    fn is_open(&self) -> bool;
    async fn start(&mut self) -> Result<()>;
    fn resize(&mut self, cols: u16, rows: u16) -> Result<()>;
    fn write(&mut self, data: &[u8]) -> Result<()>;
    fn kill(&mut self, signal: &str) -> Result<()>;
    fn output_stream(&self) -> mpsc::Receiver<Vec<u8>>;
}
```

### 7.2 本地 Shell 会话

```rust
pub struct LocalSession {
    pty_pair: Option<PtyPair>,
    writer: Box<dyn AsyncWriteExt + Send + Unpin>,
    output_tx: mpsc::Sender<Vec<u8>>,
    output_rx: mpsc::Receiver<Vec<u8>>,
    child_id: Option<u32>,
}

impl LocalSession {
    pub async fn new(params: ConnectParams) -> Result<Self>;
}

#[async_trait]
impl Session for LocalSession {
    fn is_open(&self) -> bool;
    async fn start(&mut self) -> Result<()>;
    fn resize(&mut self, cols: u16, rows: u16) -> Result<()>;
    fn write(&mut self, data: &[u8]) -> Result<()>;
}
```

### 7.3 SSH 会话

```rust
pub struct SshSession {
    client: SshClient,
    channel: Option<Channel>,
    output_tx: mpsc::Sender<Vec<u8>>,
    output_rx: mpsc::Receiver<Vec<u8>>,
}

pub enum AuthMethod {
    PromptPassword,
    Password(String),
    PublicKey { key: PathBuf, passphrase: Option<String> },
    Agent,
    None,
}

#[async_trait]
impl Session for SshSession {
    async fn start(&mut self) -> Result<()>;
    fn resize(&mut self, cols: u16, rows: u16) -> Result<()>;
    fn write(&mut self, data: &[u8]) -> Result<()>;
}
```

### 7.4 串口会话

```rust
pub struct SerialSession {
    port: Box<dyn SerialPort>,
    writer: Box<dyn SerialPort>,
    output_tx: mpsc::Sender<Vec<u8>>,
    output_rx: mpsc::Receiver<Vec<u8>>,
}

#[async_trait]
impl Session for SerialSession {
    fn is_open(&self) -> bool;
    async fn start(&mut self) -> Result<()>;
    fn write(&mut self, data: &[u8]) -> Result<()>;
}
```

### 7.5 中间件系统

```rust
pub trait SessionMiddleware: Send {
    fn process_output_to_terminal(&mut self, data: &[u8]) -> Bytes;
    fn process_output_to_session(&mut self, data: &[u8]) -> Bytes;
}

pub struct MiddlewareStack {
    middlewares: Vec<Box<dyn SessionMiddleware>>,
}

impl MiddlewareStack {
    pub fn new() -> Self;
    pub fn push(&mut self, mw: Box<dyn SessionMiddleware>);
}

pub struct OscProcessorMiddleware {
    pub cwd_reported: CallbackEmitter<String>,
}
```

---

## 8. UI 层实现

### 8.1 应用状态

```rust
pub struct App {
    pub config: AppConfig,
    pub tab_manager: TabManager,
    pub split_root: SplitContainer,
    pub show_settings: bool,
    pub search_visible: bool,
}
```

### 8.2 终端面板

```rust
pub struct TerminalPane {
    pub terminal: Terminal,
    pub session: Option<Box<dyn Session>>,
    pub selection: Option<Selection>,
    pub search: SearchState,
    pub zoom: f32,
    dirty: bool,
}

impl TerminalPane {
    pub fn new(session: Box<dyn Session>, cols: usize, rows: usize) -> Self;
    pub fn handle_input(&mut self, event: &InputEvent);
    pub fn render(&self, ui: &mut egui::Ui, rect: Rect);
    pub fn copy_selection(&self) -> Option<String>;
    pub fn zoom_in(&mut self);
    pub fn zoom_out(&mut self);
}
```

### 8.3 渲染器 (wgpu)

```rust
pub struct TerminalRenderer {
    device: Device,
    queue: Queue,
    atlas: GlyphAtlas,
    text_pipeline: TextPipeline,
    quad_pipeline: QuadPipeline,
}

impl TerminalRenderer {
    pub async fn new() -> Result<Self>;
    pub fn render_terminal(&self, target: &wgpu::TextureView, terminal: &TerminalSnapshot, viewport: Rect);
}
```

---

## 9. 分屏与标签页系统

### 9.1 SplitContainer 树

```rust
pub struct SplitContainer {
    pub direction: Direction,
    pub children: Vec<SplitChild>,
    pub ratios: Vec<f32>,
}

pub enum Direction { Horizontal, Vertical }

pub enum SplitChild {
    Container(Box<SplitContainer>),
    Pane(TerminalPane),
}

impl SplitContainer {
    pub fn new(direction: Direction) -> Self;
    pub fn add_pane(&mut self, pane: TerminalPane, ratio: f32);
    pub fn split_pane(&mut self, pane_id: u64, direction: Direction) -> u64;
    pub fn close_pane(&mut self, pane_id: u64) -> Option<TerminalPane>;
    pub fn equalize(&mut self);
}
```

### 9.2 TabManager

```rust
pub struct Tab {
    pub id: u64,
    pub title: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub content: TabContent,
    pub has_activity: bool,
    pub has_focus: bool,
}

pub enum TabContent {
    Pane(u64),
    Split(Box<SplitContainer>),
}

pub struct TabManager {
    tabs: Vec<Tab>,
    active_tab: Option<u64>,
    closed_tabs_stack: Vec<ClosedTab>,
}

impl TabManager {
    pub fn new() -> Self;
    pub fn open_tab(&mut self, content: TabContent) -> u64;
    pub fn close_tab(&mut self, id: u64) -> Option<ClosedTab>;
    pub fn select_tab(&mut self, id: u64);
    pub fn previous_tab(&mut self);
    pub fn next_tab(&mut self);
}
```

---

## 10. 配置系统

### 10.1 TOML 配置格式

```toml
[appearance]
tabs_location = "top"
theme = "dark"
frame = "thin"
opacity = 1.0

[terminal]
font_family = "JetBrains Mono"
font_size = 14
scrollback_lines = 25000
renderer = "wgpu"
bell = "visual"
ligatures = true
cursor_style = "block"
cursor_blink = true

[terminal.color_scheme]
name = "Monokai Pro"
foreground = "#cfcfc2"
background = "#1e1e1e"

[hotkeys]
tab_new = { key = "t", modifiers = { ctrl = true, shift = true } }
tab_close = { key = "w", modifiers = { ctrl = true, shift = true } }
```

---

## 11. 主题系统

```rust
pub static MONOKAI_PRO: ColorScheme = ColorScheme {
    name: "Monokai Pro".into(),
    colors: ["#1e1e1e", "#f92672", "#a6e22e", "#f4bf75",
             "#66d9ef", "#ae81ff", "#a1efe4", "#cfcfc2",
             "#75715e", "#f92672", "#a6e22e", "#f4bf75",
             "#66d9ef", "#ae81ff", "#a1efe4", "#f9f5f1"],
    foreground: "#cfcfc2".into(),
    background: "#1e1e1e".into(),
    cursor: "#f8f8f2".into(),
    selection_background: "#49483e".into(),
    selection_foreground: "#f8f8f2".into(),
    hyperlinks: None,
};
```

---

## 12. 快捷键系统

```rust
pub const BUILTIN_HOTKEYS: &[(&str, Hotkey)] = &[
    ("tab_new", Hotkey { id: "tab_new", name: "New Tab",
        key: KeyPattern { modifiers: CTRL_SHIFT, key: "t" } }),
    ("tab_close", Hotkey { id: "tab_close", name: "Close Tab",
        key: KeyPattern { modifiers: CTRL_SHIFT, key: "w" } }),
    ("copy", Hotkey { id: "copy", name: "Copy",
        key: KeyPattern { modifiers: CTRL_SHIFT, key: "c" } }),
    ("paste", Hotkey { id: "paste", name: "Paste",
        key: KeyPattern { modifiers: CTRL_SHIFT, key: "v" } }),
    ("search", Hotkey { id: "search", name: "Search",
        key: KeyPattern { modifiers: CTRL, key: "f" } }),
    ("split_vertical", Hotkey { id: "split_vertical", name: "Split Vertically",
        key: KeyPattern { modifiers: CTRL_SHIFT, key: "d" } }),
    ("split_horizontal", Hotkey { id: "split_horizontal", name: "Split Horizontally",
        key: KeyPattern { modifiers: CTRL, key: "d" } }),
    ("zoom_in", Hotkey { id: "zoom_in", name: "Zoom In",
        key: KeyPattern { modifiers: CTRL, key: "=" } }),
    ("zoom_out", Hotkey { id: "zoom_out", name: "Zoom Out",
        key: KeyPattern { modifiers: CTRL, key: "-" } }),
    ("zoom_reset", Hotkey { id: "zoom_reset", name: "Reset Zoom",
        key: KeyPattern { modifiers: CTRL, key: "0" } }),
];
```

---

## 13. 插件系统

```rust
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn init(&self, app: &mut App) -> Result<()>;
    fn provides(&self) -> Vec<Service>;
    fn requires(&self) -> Vec<&str>;
}

pub enum Service {
    Command(CommandProvider),
    ToolbarButton(ToolbarButtonProvider),
    ContextMenu(ContextMenuProvider),
    Hotkey(HotkeyProvider),
    ConfigProvider(ConfigProvider),
}

pub struct PluginManager {
    plugins: Vec<Box<dyn Plugin>>,
}

impl PluginManager {
    pub fn new() -> Self;
    pub fn load_plugin(&mut self, path: &Path) -> Result<()>;
    pub fn unload_plugin(&mut self, name: &str);
}
```

---

## 14. 平台集成

```rust
pub trait Clipboard: Send + Sync {
    fn read(&self) -> Result<String>;
    fn write(&self, text: &str);
}

pub trait Notifications: Send + Sync {
    fn notify(&self, title: &str, body: &str);
}

pub struct SystemTray {
    pub icon: Icon,
    pub menu: TrayMenu,
}

impl SystemTray {
    pub fn new() -> Self;
    pub fn show(&self);
    pub fn hide(&self);
}
```

---

## 15. 功能对照表

| Tabby 功能 | PureTerm 实现 | 模块 |
|-----------|--------------|------|
| 标签页管理 | TabManager | pureterm-ui/tabs |
| 分屏 (嵌套) | SplitContainer | pureterm-ui/split |
| 本地 Shell | LocalSession | pureterm-session/local |
| SSH 连接 | SshSession | pureterm-session/ssh |
| 串口连接 | SerialSession | pureterm-session/serial |
| Telnet | TelnetSession | pureterm-session/telnet |
| 终端渲染 | wgpu + fontdue | pureterm-gui |
| 滚动缓冲 | Grid<Cell> | pureterm-terminal/screen |
| 搜索 | SearchState | pureterm-ui/terminal |
| 颜色方案 | ThemeService | pureterm-core/theme |
| 快捷键 | HotkeyService | pureterm-core/hotkey |
| 连接配置 | ProfileManager | pureterm-core/profile |
| TOML 配置 | ConfigService | pureterm-core/config |
| 系统托盘 | SystemTray | pureterm-ui |
| 插件系统 | PluginManager | pureterm-core/plugin |
| 进程通知 | Notifications | pureterm-core/platform |
| X11 转发 | X11Forward | pureterm-session/ssh |
| Agent 转发 | AgentForward | pureterm-session/ssh |
| 端口转发 | PortForward | pureterm-session/ssh |
| SFTP | SftpSession | pureterm-session/ssh |
| 登录脚本 | LoginScriptMiddleware | pureterm-session/middleware |
| OSC 1337 (cwd) | OscProcessorMiddleware | pureterm-session/middleware |
| Bracketed Paste | 输入处理 | pureterm-terminal/input |
| 字体连字 | fontdue + freetype | pureterm-gui/fonts |
| 选中复制 | Selection | pureterm-ui/terminal |
| 右键菜单 | ContextMenu | pureterm-ui |
| 标签页拖拽 | TabDragDrop | pureterm-ui/tabs |
| Quake 模式 | QuakeMode | pureterm-ui |
| 多窗口 | MultipleWindows | pureterm |
| 标签页恢复 | TabRecovery | pureterm-ui/tabs |
| 配置加密 | VaultService | pureterm-core |

---

## 16. 依赖清单

```toml
[workspace]
resolver = "2"
members = [
    "crates/pureterm",
    "crates/pureterm-core",
    "crates/pureterm-terminal",
    "crates/pureterm-session",
    "crates/pureterm-ui",
    "crates/pureterm-gui",
]

[workspace.dependencies]
# Async
tokio = { version = "1.42", features = ["full"] }
futures = "0.3"
async-trait = "0.1"

# Terminal parsing
vte = "0.22"
unicode-width = "0.1"
unicode-segmentation = "1.11"

# SSH
russh = "0.48"
russh-keys = "0.48"

# Serial
serialport = "4.6"

# Local PTY
portable-pty = "0.12"

# UI - egui
egui = "0.29"
epaint = "0.29"

# GPU Rendering
wgpu = "0.20"
winit = "0.30"

# Fonts
fontdue = "0.9"
freetype = "0.7"

# Config
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "0.8"
dirs = "5.0"

# Utils
uuid = { version = "1.11", features = ["v4", "serde"] }
thiserror = "2.0"
anyhow = "1.0"
log = "0.4"
env_logger = "0.11"
parking_lot = "0.12"
once_cell = "1.20"
regex = "1.11"

# Crypto
sha2 = "0.10"
ed25519-dalek = "2.1"
rsa = "0.9"

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
```

---

## 17. 文件结构

```
PureTerm/
├── Cargo.toml
├── README.md
├── PLAN.md
│
├── crates/
│   ├── pureterm/                 # 主程序入口
│   │   └── src/main.rs
│   │
│   ├── pureterm-core/            # 核心服务
│   │   └── src/
│   │       ├── config.rs
│   │       ├── profile.rs
│   │       ├── theme.rs
│   │       ├── hotkey.rs
│   │       ├── plugin.rs
│   │       └── error.rs
│   │
│   ├── pureterm-terminal/        # 终端模拟器
│   │   └── src/
│   │       ├── parser/
│   │       │   └── perform.rs
│   │       ├── screen/
│   │       │   ├── cell.rs
│   │       │   ├── grid.rs
│   │       │   ├── cursor.rs
│   │       │   └── color.rs
│   │       ├── mode.rs
│   │       ├── input.rs
│   │       └── term.rs
│   │
│   ├── pureterm-session/         # 会话管理
│   │   └── src/
│   │       ├── base.rs
│   │       ├── local.rs
│   │       ├── middleware.rs
│   │       ├── osc_processor.rs
│   │       ├── ssh/
│   │       │   ├── client.rs
│   │       │   ├── auth.rs
│   │       │   ├── forward.rs
│   │       │   └── sftp.rs
│   │       ├── serial/
│   │       └── telnet/
│   │
│   ├── pureterm-ui/              # UI 层
│   │   └── src/
│   │       ├── app.rs
│   │       ├── tabs/
│   │       │   └── manager.rs
│   │       ├── split/
│   │       │   └── container.rs
│   │       ├── terminal/
│   │       │   └── pane.rs
│   │       └── settings/
│   │
│   └── pureterm-gui/             # wgpu 渲染
│       └── src/
│           ├── renderer.rs
│           ├── pipeline.rs
│           ├── textures.rs
│           ├── fonts.rs
│           └── shaders/
│
└── assets/
```

---

## 18. 开发阶段规划

### 阶段 1: 基础设施 (4 周)

| 周 | 任务 | 交付物 |
|----|------|--------|
| 1 | 项目初始化，workspace 结构 | 编译通过的空白项目 |
| 1 | ConfigService | 配置加载/保存 |
| 2 | ProfileManager | 连接配置管理 |
| 2 | ThemeService + 9 个内建配色 | 主题系统 |
| 3 | HotkeyService | 快捷键系统 |
| 4 | PluginManager | 插件接口定义 |

### 阶段 2: 终端模拟器 (6 周)

| 周 | 任务 | 交付物 |
|----|------|--------|
| 5 | VteParser + Perform | 解析器封装 |
| 5 | Cell, Grid, Screen | 屏幕缓冲区 |
| 6 | CSI/ESC/OSC 序列 | 终端模式支持 |
| 6 | Terminal 主结构 | 终端 API |
| 7 | wgpu 渲染器架构 | 渲染管线 |
| 7 | 字体加载 + 图集 | 纹理管理 |
| 8 | 终端渲染 | 可显示文字 |
| 8 | 光标渲染 | 闪烁光标 |
| 9 | 选中 + 搜索高亮 | 文本选择 |
| 10 | 滚动 + 视口 | 历史记录 |

### 阶段 3: 会话层 (4 周)

| 周 | 任务 | 交付物 |
|----|------|--------|
| 11 | Session trait | 统一接口 |
| 11 | LocalSession | 本地 Shell |
| 12 | MiddlewareStack | 中间件系统 |
| 12 | OSC 处理器 | CWD 追踪 |
| 13 | SshSession | SSH 连接 |
| 13 | SSH Auth | 认证 |
| 14 | SerialSession | 串口连接 |
| 14 | TelnetSession | Telnet |

### 阶段 4: UI 层 (5 周)

| 周 | 任务 | 交付物 |
|----|------|--------|
| 15 | App 状态, TabManager | 标签页管理 |
| 16 | SplitContainer | 分屏 |
| 16 | Divider 拖拽 | 分屏调整 |
| 17 | TerminalPane | 终端面板 UI |
| 17 | SearchPanel | 搜索 |
| 18 | ContextMenu | 右键菜单 |
| 18 | TitleBar | 标题栏 |
| 19 | SettingsPanel | 设置界面 |

### 阶段 5: SSH 高级功能 (3 周)

| 周 | 任务 | 交付物 |
|----|------|--------|
| 20 | SFTP 会话 | 文件传输 |
| 20 | 端口转发 | Local/Dynamic/Remote |
| 21 | X11 转发 | X11 支持 |
| 21 | SSH Agent 转发 | Agent Forward |
| 22 | SSH 复用 | Multiplexer |

### 阶段 6: 平台集成 (3 周)

| 周 | 任务 | 交付物 |
|----|------|--------|
| 23 | SystemTray | 托盘图标 |
| 23 | GlobalHotkey | 全局快捷键 |
| 24 | Notifications | 进程通知 |
| 24 | Clipboard | 复制粘贴 |
| 25 | 拖放 | 文件拖放 |
| 25 | 自动更新 | Updater |

### 阶段 7: 完善 (2 周)

| 周 | 任务 | 交付物 |
|----|------|--------|
| 26 | 标签页恢复 | TabRecovery |
| 26 | 配置加密 | VaultService |
| 27 | 插件系统完善 | PluginManager |
| 27 | Quake 模式 | QuakeMode |
| 28 | 多窗口 | MultipleWindows |
| 28 | 完整测试 | Test Suite |

---

### 总工期: ~28 周 (~7 个月)

**里程碑**:
- Week 11: MVP - 本地 Shell 可用
- Week 19: Alpha - SSH + 分屏 + 标签页
- Week 25: Beta - 完整功能
- Week 28: 1.0 Release

---

## 19. 风险与备选方案

### 风险 1: 终端渲染性能
xterm.js 经过多年优化，纯 Rust 渲染器可能难以匹敌。
**方案**: 借鉴 Alacritty 架构，使用 GPU 加速渲染，一次绘制整个屏幕。Alacritty 可达 500 FPS。

### 风险 2: 跨平台复杂度
Windows/macOS/Linux 的 PTY 和串口 API 差异大。
**方案**: 使用跨平台 crate (portable-pty, serialport, winit, wgpu)。

### 风险 3: 功能完整性
Tabby 有大量功能，重写需要大量时间。
**方案**: 分阶段实现，MVP 优先核心功能。

### 风险 4: 插件系统
Tabby 的 Angular 依赖注入系统无法直接迁移。
**方案**: 实现简化的插件接口，支持动态库加载。

---

## 附录 A: 参考项目

| 项目 | 仓库 | 许可 | 用途 |
|------|------|------|------|
| Alacritty | github.com/alacritty/alacritty | Apache 2.0 | vte + OpenGL 渲染参考 |
| WezTerm | github.com/wez/wezterm | Apache 2.0 | wgpu + 复杂功能参考 |
| Rio | github.com/raphamorim/rio | MIT | vte + wgpu 简单参考 |
| Tabby | github.com/Eugeny/tabby | MIT | 功能对齐目标 |
| vte | github.com/alacritty/vte | MIT | 终端解析器 |
| russh | github.com/warpdotoff/russh | BSD-3 | SSH 客户端 |
| serialport | github.com/serialport/rs-serialport | MIT | 串口通信 |
| portable-pty | github.com/wez/portable-pty | Apache 2.0 | 本地 PTY |
| egui | github.com/emilk/egui | Apache 2.0 | UI 框架 |
| wgpu | github.com/gfx-rs/wgpu | Apache 2.0 | GPU 渲染 |

---

**文档版本**: 1.0
**最后更新**: 2026-04-11
