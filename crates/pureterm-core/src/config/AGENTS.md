# pureterm-core/src/config — 配置管理

配置文件系统，支持 100+ 选项、条件配置、验证、多配置源加载、从其他终端导入配置。

## 文件结构

```
src/
├── mod.rs         # Config 结构体（100+ 选项）+ re-exports + 模块声明 (694 行)
├── options.rs     # 配置选项类型定义：Color、RepeatableString、枚举、默认值函数 (886 行)
├── conditional.rs # 条件配置解析器：if/else 块 + 条件求值器 (504 行)
├── loader.rs      # 多源配置加载器：文件 + CLI + 条件 + 引用 + 循环检测 (417 行)
├── validator.rs   # 配置验证器：字体/颜色/窗口/输入/命令/平台 (280 行)
├── cli.rs         # CLI 参数解析：--option=value、位置参数、-c/-h/-v (149 行)
├── import.rs      # 配置导入：iTerm2 plist + Alacritty TOML → AppConfig (950 行) **NEW**
├── legacy.rs      # 旧版配置兼容：AppConfig + AppearanceConfig + TerminalConfig + 持久化 (201 行) **NEW**
├── service.rs     # ConfigService 独立实现：运行时加载/保存/监控/热重载 (119 行) **NEW**
└── types.rs       # 共享类型：GlassEffectStyle 枚举 (20 行) **NEW**
```

## 模块依赖

```
config/ → lib.rs（re-export Config、AppConfig 等）
config/ → theme.rs（ThemeService，service.rs 中使用）
config/ → hotkey.rs（HotkeysConfig，legacy.rs 中使用）
config/ → profile.rs（Profile、ProfileGroup，legacy.rs 中使用）
config/ → error.rs（CoreError、Result，service.rs 中使用）
config/ → default_terminal.rs（service.rs 未直接使用，settings 面板调用）
```

内部依赖：
```
mod.rs → options.rs（Config 字段类型）
mod.rs → types.rs（GlassEffectStyle）
mod.rs → legacy.rs（AppConfig 等 re-export）
mod.rs → service.rs（ConfigService re-export）
loader.rs → cli.rs（CliArgs）
loader.rs → conditional.rs（parse_conditional、ConditionalEvaluator）
loader.rs → options.rs（Color、CursorStyle 等类型）
service.rs → legacy.rs（load_app_config / save_app_config）
service.rs → cli.rs（CliArgs）
service.rs → theme.rs（ThemeService）
import.rs → legacy.rs（AppConfig）
import.rs → theme.rs（ColorScheme）
legacy.rs → types.rs（GlassEffectStyle）
legacy.rs → profile.rs（Profile、ProfileGroup）
legacy.rs → hotkey.rs（HotkeysConfig）
```

## 核心类型

### Config（mod.rs）— 新版 100+ 选项配置

Ghostty 风格的完整配置结构，包含所有终端配置选项：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    // ── 字体 ── font_family / font_size / font_style / font_feature / font_variation / font_thicken
    // ── 颜色/主题 ── theme / background / foreground / cursor_color / palette / selection_*
    // ── 渲染/度量 ── adjust_cell_width / adjust_font_baseline / grapheme_width_method
    // ── 窗口/布局 ── window_padding / window_decorations / window_* / maximize / fullscreen
    // ── 背景/透明度 ── background_opacity / background_blur / unfocused_split_*
    // ── 鼠标/键盘 ── mouse_hide_while_typing / clipboard_* / keybind / key_remap
    // ── 命令/进程 ── command / env / shell_integration
    // ── 回滚 ── scrollback_limit / scrollbar
    // ── 链接 ── link / link_url / link_previews
    // ── 快速终端 ── quick_terminal_position / quick_terminal_size / quick_terminal_autohide
    // ── 通知 ── desktop_notifications / bell_features / osc_color_report_format
    // ── 平台特定 ── macos_* / gtk_* / linux_*
    // ── 自动更新 ── auto_update / auto_update_channel
}
```

**配置分组（共 16 组，100+ 字段）：**

| 分组 | 说明 | 关键字段 |
|------|------|---------|
| Font | 字体配置 | font_family, font_size, font_style, font_feature, font_variation |
| Colors/Theme | 颜色主题 | theme, background, foreground, cursor_color, palette, selection_* |
| Rendering/Metrics | 渲染度量 | adjust_cell_width, adjust_font_baseline, grapheme_width_method |
| Window/Layout | 窗口布局 | window_padding, window_decorations, window_show_tab_bar, maximize |
| Background/Opacity | 背景透明 | background_opacity, background_blur, unfocused_split_opacity |
| Mouse/Keyboard | 输入控制 | mouse_hide_while_typing, clipboard_*, keybind, key_remap |
| Command/Process | 命令进程 | command, env, shell_integration |
| Scrollback | 回滚缓冲 | scrollback_limit, scrollbar |
| Links | 链接规则 | link, link_url, link_previews |
| Quick Terminal | 快速终端 | quick_terminal_position, quick_terminal_autohide |
| Desktop Notifications | 桌面通知 | desktop_notifications, bell_features |
| Platform macOS | macOS 特定 | macos_option_as_alt, macos_titlebar_style |
| Platform GTK/Linux | Linux 特定 | gtk_single_instance, linux_memory_limit |
| Auto Update | 自动更新 | auto_update, auto_update_channel |

### AppConfig（legacy.rs）— 旧版兼容配置

保持向后兼容的旧版配置结构，用于 UI 层和持久化：

```rust
pub struct AppConfig {
    pub appearance: AppearanceConfig,
    pub terminal: TerminalConfig,
    pub hotkeys: HotkeysConfig,
    pub profiles: Vec<Profile>,
    pub groups: Vec<ProfileGroup>,
    pub version: u32,
    pub secret_redaction: bool,
    pub confirm_quit: bool,
    pub prevent_sleep: bool,
}
```

**持久化方法：**
- `load_app_config()` — 从 `$XDG_CONFIG_HOME/pureterm/config.toml` 读取
- `save_app_config()` — 将 TOML 写入磁盘

### ConfigService（service.rs）— 运行时配置服务

线程安全地管理运行时配置状态：

```rust
pub struct ConfigService {
    config_dir: PathBuf,
    config: RwLock<AppConfig>,
    theme_service: RwLock<ThemeService>,
    reload_tx: Option<mpsc::Sender<()>>,
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `load() -> Result<Self>` | 从默认路径加载 AppConfig + 用户主题 |
| `load_with_cli(cli) -> Result<Self>` | 加载配置（CLI 参数预留） |
| `start_watching(&mut self) -> Result<Receiver<()>>` | 启动文件监控，配置修改时发送通知 |
| `reload(&self) -> Result<()>` | 从磁盘重新加载配置 |
| `save(&self) -> Result<()>` | 将配置写入磁盘 |
| `get(&self) -> RwLockReadGuard<AppConfig>` | 获取配置读锁 |
| `update<F>(&self, f: F)` | 在写锁下更新配置 |
| `theme_service(&self)` | 获取主题服务读锁 |

### 配置选项类型（options.rs）

详细的配置选项类型定义：

| 类型 | 说明 |
|------|------|
| `Color` | RGBA 颜色（from_hex / to_hex / rgb / rgba） |
| `RepeatableString` | 多回退值字符串（字体族列表） |
| `FontStyle` | 字体风格枚举 |
| `FontFeature` | OpenType 特性（+liga / -calt） |
| `FontVariation` | 可变字体轴（wght=700） |
| `CursorStyle` | 光标样式（Block / Underline / Beam） |
| `MouseReporting` | 鼠标报告模式 |
| `ShellIntegration` | Shell 集成级别 |
| `Command` | 命令配置 |
| `KeyBinding` / `KeyRemap` | 快捷键绑定和重映射 |
| `LinkRule` | 链接匹配规则 |
| `WindowPadding` / `Size` / `WindowDecorations` | 窗口度量 |
| `Scrollbar` | 滚动条配置 |
| `RightClickAction` | 右键行为 |
| `PaletteGenerate` | 调色板生成模式 |
| `BoldColor` / `SyntheticStyle` | 粗体颜色 / 合成风格 |
| `GraphemeWidthMethod` | Grapheme 宽度计算方法 |

### 条件配置（conditional.rs）

支持基于主题、操作系统、环境变量的条件配置：

```rust
pub enum ConditionalBlock {
    If { condition, then_body, else_body },
    KeyValue(ConfigEntry),
}

pub enum Condition {
    ThemeEqual(String),
    ThemeNotEqual(String),
    OsEqual(String),
    OsNotEqual(String),
    EnvVar { name, value },
    Always,
}
```

**语法示例：**
```ini
if theme == light {
  background = "#ffffff"
} else {
  background = "#000000"
}
```

**ConditionalEvaluator** — 运行时求值：根据当前主题和操作系统选择正确的配置块。

### 配置加载器（loader.rs）

多源配置加载和合并：

```rust
pub struct ConfigLoader {
    loaded_files: HashSet<PathBuf>,  // 循环检测
    cli: Option<CliArgs>,
}
```

**配置源优先级（从高到低）：**
1. CLI 参数（`--option=value`）
2. 用户配置文件（`~/.config/pureterm/config.toml`）
3. 引用配置文件（`config-file = "path"`，支持循环检测）
4. 条件配置块
5. 默认值

**关键方法：**

| 方法 | 说明 |
|------|------|
| `load(&mut self) -> Result<Config>` | 从默认文件 + CLI 加载 |
| `load_with_cli(&mut self, cli) -> Result<Config>` | 加载并应用 CLI 参数 |
| `load_from_path(&mut self, path) -> Result<Config>` | 从指定路径加载 |

### 配置验证器（validator.rs）

配置验证，提供有用的错误和警告消息：

```rust
pub struct ConfigValidator;
```

**验证类型：**
- `validate_fonts` — 字体族非空、字体大小范围
- `validate_colors` — 颜色值格式、palette 完整性
- `validate_window` — 窗口尺寸、padding 合理性
- `validate_input` — 鼠标/剪贴板配置
- `validate_command` — shell/命令配置
- `validate_platform` — 平台特定设置合法性

**结果类型：**
- `ValidationResult { errors, warnings }` — 验证结果
- `ValidationError { field, message }` — 阻断性错误
- `ValidationWarning { field, message }` — 非阻断警告

### CLI 参数解析（cli.rs）

从命令行解析参数：

```rust
pub struct CliArgs {
    pub positional: Vec<String>,         // 位置参数
    pub options: HashMap<String, String>, // --option=value
    pub config_file: Option<PathBuf>,    // -c / --config
    pub help: bool,                       // -h / --help
    pub version: bool,                    // -v / --version
}
```

### 配置导入（import.rs）**NEW**

从其他终端模拟器导入配置到 PureTerm 的 `AppConfig`：

```rust
pub struct ImportResult {
    pub config: AppConfig,
    pub imported_count: usize,
    pub warnings: Vec<String>,
}
```

**支持的导入源：**

| 源 | 函数 | 说明 |
|----|------|------|
| iTerm2 | `import_from_iterm2()` | 解析 `com.googlecode.iterm2.plist`，通过 `plutil -convert json` 转换 |
| Alacritty | `import_from_alacritty()` | 解析 TOML 配置文件 |

**导入映射：**

| 设置 | iTerm2 | Alacritty |
|------|--------|-----------|
| 字体 | Normal Font → font_family | font.family → font_family |
| 字号 | Font Size → font_size | font.size → font_size |
| 光标 | Cursor Type (0/1/2) → block/vertical/underline | cursor.style.shape → block/underline/beam |
| 回滚 | Scrollback Lines | scrolling.history |
| 透明度 | Transparency → opacity | window.opacity |
| 响铃 | Silence Bell / Visual Bell | bell.duration |
| 配色 | 16 ANSI 颜色 + BG/FG/Cursor | primary + normal + bright 颜色 |
| 选择复制 | — | selection.save_to_clipboard |

**iTerm2 颜色格式**：`{Red Component, Green Component, Blue Component}` 浮点值 → hex
**Alacritty 路径搜索**：`~/.config/alacritty/alacritty.toml` → `alacritty.yml` → `~/.alacritty.toml`

### 旧版配置兼容（legacy.rs）**NEW**

保持与 pureterm-ui 等模块的向后兼容：

| 类型 | 说明 |
|------|------|
| `AppConfig` | 顶层旧版配置（appearance + terminal + hotkeys + profiles + groups） |
| `AppearanceConfig` | 外观配置（theme, tabs_location, opacity, corner_radius, glass_effect） |
| `TerminalConfig` | 终端配置（font_family, font_size, scrollback_lines, cursor_style, color_scheme） |
| `load_app_config()` | 从默认路径加载 AppConfig（TOML 反序列化） |
| `save_app_config()` | 将 AppConfig 保存到默认路径 |

### 共享类型（types.rs）**NEW**

被 legacy.rs 和 options.rs 共同使用的类型：

```rust
pub enum GlassEffectStyle {
    None, Regular, Clear, Light, Dark, Automatic,
}
```

## Re-export 映射（mod.rs 顶部）

```rust
pub use options::*;                          // Color, RepeatableString, 枚举, 默认值函数
pub use cli::CliArgs;                        // CLI 参数
pub use conditional::ConditionalConfig;      // 条件配置
pub use loader::{ConfigLoader, ConfigError}; // 加载器
pub use validator::ConfigValidator;          // 验证器
pub use legacy::AppConfig;                   // 旧版配置
pub use legacy::AppearanceConfig;            // 旧版外观
pub use legacy::TerminalConfig;              // 旧版终端
pub use types::GlassEffectStyle;             // 玻璃效果
pub use service::ConfigService;              // 配置服务
```

## 常见任务

### 添加新的配置项

1. 在 `options.rs` 中定义配置选项类型和默认值函数
2. 在 `mod.rs` 的 `Config` 结构体中添加 `#[serde(default)]` 字段
3. 在 `mod.rs` 的 `Config::default()` 实现中添加默认值
4. 在 `legacy.rs` 中为旧版兼容添加对应字段（如 UI 层需要）
5. 在 `validator.rs` 中添加验证规则（如需要）
6. 在 `loader.rs` 中添加加载逻辑（如需要）

### 修改配置加载逻辑

编辑 `loader.rs` 的 `ConfigLoader` 实现。配置合并顺序：defaults → file → conditional → CLI。

### 添加条件配置支持

在 `conditional.rs` 的 `Condition` 枚举中添加新的条件变体，在 `ConditionalEvaluator` 中实现匹配逻辑。

### 添加从新终端导入配置

1. 在 `import.rs` 中定义源终端的配置结构体（`#[derive(Deserialize)]`）
2. 实现 `import_from_xxx() -> Result<ImportResult, String>` 函数
3. 在 `general.rs`（设置面板）中添加导入按钮

### 修改运行时配置服务

编辑 `service.rs`。ConfigService 使用 `parking_lot::RwLock` 实现线程安全的读写，使用 `notify` crate 监控文件变化实现热重载。

## 父级导航

← [src/AGENTS.md](../AGENTS.md)
