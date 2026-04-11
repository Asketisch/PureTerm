# pureterm-core — 配置、主题、快捷键、插件（无 UI 依赖）

工作区的基础 crate，提供配置管理、配色方案、快捷键绑定、配置文件和插件系统。

## 关键依赖

`serde` + `serde_json` + `toml`（序列化）、`dirs`（路径）、`uuid`（ID）、`thiserror`（错误）、`parking_lot`（并发）。

## 文件结构

```
src/
├── lib.rs           # 模块导出
├── config.rs        # AppConfig + ConfigService
├── error.rs         # CoreError 枚举
├── hotkey.rs        # 快捷键系统
├── plugin.rs        # 插件框架（预留）
├── profile.rs       # 连接配置文件
└── theme.rs         # 配色方案 + ThemeService
```

## 模块详情

### config.rs — 配置管理

| 类型 | 说明 |
|------|------|
| `AppConfig` | 顶层配置结构（appearance + terminal + hotkeys + profiles + groups） |
| `AppearanceConfig` | 外观配置：tabs_location、theme、frame、opacity、show_tab_bar、show_title_bar |
| `TerminalConfig` | 终端配置：font_family/font_size、scrollback_lines、renderer、bell、ligatures、cursor_style、cursor_blink、copy_on_select、color_scheme |
| `ConfigService` | 基于 `RwLock<AppConfig>` 的线程安全配置服务 |

`ConfigService` 关键方法：
- `load()` — 从 `$XDG_CONFIG_HOME/pureterm/config.toml` 读取，不存在则创建默认
- `save()` — 写入磁盘
- `get()` — 读锁获取 `AppConfig`
- `update(FnOnce(&mut AppConfig))` — 写锁更新

### theme.rs — 配色方案

| 类型 | 说明 |
|------|------|
| `ColorScheme` | 配色方案：name、foreground、background、cursor、selection_*、colors[16] |
| `ThemeService` | 管理内置和自定义配色方案 |

内置配色（9 种）：Monokai Pro、Monokai、Solarized Dark、Solarized Light、Dracula、One Dark、GitHub Dark、Nord、Catppuccin Mocha（默认）。

`ColorScheme` 颜色方法：`foreground_rgb()`、`background_rgb()`、`cursor_rgb()`、`color_rgb(index)` — 都返回 `[u8; 3]`。

### hotkey.rs — 快捷键

| 类型 | 说明 |
|------|------|
| `Modifiers` | 修饰键状态：ctrl、alt、shift、super_key |
| `KeyPattern` | 键 + 修饰键组合 |
| `Hotkey` | id + name + KeyPattern |
| `HotkeyService` | 快捷键查找和匹配 |

内置快捷键（13 个）：tab_new (Ctrl+Shift+T)、tab_close (Ctrl+Shift+W)、copy (Ctrl+Shift+C)、paste (Ctrl+Shift+V)、search (Ctrl+F)、split_vertical (Ctrl+Shift+D)、split_horizontal (Ctrl+D)、zoom_in (Ctrl+=)、zoom_out (Ctrl+-)、zoom_reset (Ctrl+0)、prev_tab (Ctrl+Shift+Left)、next_tab (Ctrl+Shift+Right)、clear (Ctrl+Shift+L)。

### profile.rs — 连接配置

| 类型 | 说明 |
|------|------|
| `ProfileType` | 枚举：Ssh、Serial、Local、Telnet |
| `Profile` | 连接配置：id、name、profile_type、group、options、color、icon |
| `ProfileOptions` | 所有会话类型的配置参数集合 |
| `ProfileProvider` | trait：提供配置到 tab 参数的转换 |
| `ProfileManager` | 注册和管理 ProfileProvider |

### error.rs — 错误类型

```rust
enum CoreError {
    Config(String),
    ProfileNotFound(String),
    ThemeNotFound(String),
    HotkeyConflict(String),
    Io(io::Error),
    Serialization(toml::de::Error),
    Other(String),
}
```

### plugin.rs — 插件系统（预留）

定义 `Plugin` trait、`PluginManager`、`ServiceRegistry`、`CommandDescriptor`。目前为占位实现。

## 常见任务

### 添加新的配置项
1. 在对应 Config 结构体（`AppearanceConfig`/`TerminalConfig`）中添加字段，设置 `#[serde(default = ...)]`
2. 如需迁移逻辑，在 `ConfigService::load()` 中处理

### 添加新的内置配色
在 `theme.rs` 的 `builtin_color_schemes()` 中使用 `color_scheme!` 宏添加条目。16 个 ANSI 颜色：索引 0-7 普通色，8-15 亮色。

### 添加新的快捷键
1. 在 `hotkey.rs` 的内置快捷键列表中添加新条目
2. 在 `pureterm-ui` 的 `App::handle_key` 中添加对应的处理逻辑

### 添加新的配置文件类型
1. 在 `ProfileType` 枚举中添加变体
2. 在 `ProfileOptions` 中添加对应字段
3. 实现 `ProfileProvider` trait 并注册到 `ProfileManager`
