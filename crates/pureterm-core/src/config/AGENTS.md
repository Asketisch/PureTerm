# pureterm-core/src/config — 配置管理

配置文件系统，支持条件配置、验证、多配置源加载。

## 文件结构

```
src/
├── mod.rs         # ConfigService + AppConfig + 顶层配置结构 (971 行)
├── options.rs     # 配置选项定义 (886 行)
├── conditional.rs # 条件配置解析 (504 行)
├── loader.rs      # 配置文件加载器 (416 行)
├── validator.rs   # 配置验证器 (280 行)
└── cli.rs         # CLI 参数解析 (149 行)
```

## 模块依赖

```
config/ → lib.rs (AppConfig, AppearanceConfig, TerminalConfig)
config/ → theme.rs (ThemeService)
config/ → hotkey.rs (HotkeyService)
```

## 核心类型

### ConfigService（mod.rs）

配置服务，线程安全地管理应用配置：

```rust
pub struct ConfigService {
    config: RwLock<AppConfig>,
    path: PathBuf,
    watchers: Vec<ConfigWatcher>,
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `load() -> Result<Self>` | 从 `$XDG_CONFIG_HOME/pureterm/config.toml` 加载配置 |
| `save(&self) -> Result<()>` | 将配置写入磁盘 |
| `get(&self) -> Guard<AppConfig>` | 获取配置读锁 |
| `update<F>(&self, f: F) -> Result<()>` | 在写锁下更新配置 |

### AppConfig（mod.rs）

顶层配置结构：

```rust
pub struct AppConfig {
    pub appearance: AppearanceConfig,
    pub terminal: TerminalConfig,
    pub hotkeys: HotkeysConfig,
    pub profiles: Vec<Profile>,
    pub groups: Vec<Group>,
    pub version: String,
}
```

### 配置选项（options.rs）

详细的配置选项定义：

| 类型 | 说明 |
|------|------|
| `AppearanceOptions` | 外观选项：theme、tabs_location、frame、opacity、corner_radius、glass_effect |
| `TerminalOptions` | 终端选项：font_family、font_size、scrollback_lines、renderer、bell、ligatures |
| `PerformanceOptions` | 性能选项：vsync、hardware_acceleration、texture_cache_size |
| `AccessibilityOptions` | 无障碍选项：screen_reader、high_contrast、cursor_style |

### 条件配置（conditional.rs）

支持基于平台、环境变量的条件配置：

```rust
pub struct ConditionalConfig {
    pub os: Option<String>,           // "macos", "linux", "windows"
    pub arch: Option<String>,        // "x86_64", "aarch64"
    pub env: Option<HashMap<String, String>>,  // 环境变量条件
    pub features: Vec<String>,       // 功能特性条件
}
```

**条件匹配逻辑：**
- `os` 匹配 `std::env::consts::OS`
- `arch` 匹配 `std::env::consts::ARCH`
- `env` 检查指定环境变量是否存在或值匹配

### 配置加载器（loader.rs）

多源配置加载和合并：

```rust
pub struct ConfigLoader {
    sources: Vec<ConfigSource>,
    defaults: AppConfig,
}
```

**配置源优先级（从高到低）：**
1. CLI 参数
2. 用户配置 (`~/.config/pureterm/config.toml`)
3. 默认配置

### 配置验证器（validator.rs）

配置验证和迁移：

```rust
pub struct ConfigValidator {
    rules: Vec<ValidationRule>,
    migrations: Vec<ConfigMigration>,
}
```

**验证类型：**
- `StringValidator` — 字符串格式验证
- `NumericValidator` — 数值范围验证
- `EnumValidator` — 枚举值验证
- `PathValidator` — 文件路径验证

## 常见任务

### 添加新的配置项

1. 在 `options.rs` 中定义配置选项类型
2. 在 `mod.rs` 的 `AppConfig` 中添加字段
3. 在 `loader.rs` 中添加加载逻辑（如需要）
4. 在 `validator.rs` 中添加验证规则（如需要）

### 修改配置加载逻辑

编辑 `loader.rs` 的 `load()` 方法。配置合并顺序：defaults → file → CLI。

### 添加条件配置支持

在 `conditional.rs` 中添加新的条件类型，并在 `loader.rs` 的 `apply_conditions()` 中实现匹配逻辑。

## 父级导航

← [pureterm-core/AGENTS.md](../../AGENTS.md)