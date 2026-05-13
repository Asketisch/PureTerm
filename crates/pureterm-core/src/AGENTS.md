# pureterm-core/src — 基础核心库源码

工作区基础 crate 的所有源文件：配置管理、主题配色、快捷键、AI 集成、加密存储、触发器、插件、i18n、C API 等功能模块。

← [pureterm-core/AGENTS.md](../AGENTS.md)

## 文件结构

```
src/
├── lib.rs                   # crate 入口，re-export Config, ConfigService, CliArgs (24 行)
├── ai.rs                    # AI 集成 — OpenAI/Claude/Ollama LLM 接口 (748 行)
├── auto_profile.rs          # 自动配置文件切换 — 基于目录/主机名 (76 行)
├── capi.rs                  # C API 绑定 — FFI 接口（桩实现）(403 行)
├── clipboard.rs             # 剪贴板缓冲区管理 (279 行)
├── command_correction.rs    # AI 命令纠正建议 (133 行)
├── default_terminal.rs      # macOS 默认终端注册 (73 行)
├── dirfreq.rs               # 目录频率跟踪与持久化 (353 行)
├── error.rs                 # CoreError 错误枚举 (33 行)
├── hotkey.rs                # 快捷键定义与匹配 (134 行)
├── i18n.rs                  # 国际化支持 — 区域设置与翻译 (168 行)
├── notification.rs          # 桌面通知系统 (128 行)
├── plugin.rs                # 插件框架（预留）(100 行)
├── prevent_sleep.rs         # 系统休眠防止 — macOS caffeinate (73 行)
├── profile.rs               # 连接配置文件管理 (153 行)
├── remote.rs                # 远程控制 — Unix socket 命令 (351 行)
├── secret_redaction.rs      # API 密钥/秘密模式检测和视觉脱敏 (204 行)
├── terminfo.rs              # Terminfo 数据库与终端标识 (61 行)
├── theme.rs                 # 配色方案 + ThemeService (391 行)
├── theme_auto.rs            # 自动主题检测 — 系统深色/浅色 (98 行)
├── trigger.rs               # 输出触发器系统 (260 行)
├── vault.rs                 # 加密存储 — AES-256-GCM (179 行)
└── config/                  → config/AGENTS.md — 配置管理子系统
```

## 模块详情

### lib.rs — crate 入口

声明所有子模块并重导出核心类型：`pub use config::{AppConfig as Config, ConfigService, CliArgs}`。

### ai.rs — AI 集成（748 行）

| 类型 | 说明 |
|------|------|
| `AiConfig` | AI 后端配置：backend、api_key、model、base_url、max_tokens、temperature |
| `AiBackend` | 枚举：OpenAi、Claude、Ollama、Disabled |
| `AiMessage` | 消息结构：role（System/User/Assistant）、content |
| `AiRole` | System / User / Assistant |

支持三种后端的 HTTP API 调用，用于命令建议、AI 聊天、命令纠正等功能。默认 backend 为 Disabled。

### auto_profile.rs — 自动配置文件切换

| 类型 | 说明 |
|------|------|
| `AutoProfileRule` | 规则：pattern（匹配模式）、profile_id、match_type |
| `MatchType` | Directory（目录前缀）或 Hostname（主机名） |
| `AutoProfileSwitch` | 服务：rules 列表，基于上下文自动切换配置文件 |

### capi.rs — C API 绑定（403 行）

libpureterm 的 C API 桩实现，定义完整的 FFI 接口契约。

| 类型 | 说明 |
|------|------|
| `PTTerminal` / `PTDisplay` / `PTConfig` | 不透明指针类型（`*mut ()`） |
| `PTSize` | 终端尺寸（cols, rows） |
| `PTCursor` | 光标位置（x, y） |
| `PTCellAttrs` | 单元格属性（fg/bg_color、bold、italic、underline 等） |
| `PTModifiers` | 修饰键标志 |
| `PTKeyEvent` / `PTMouseEvent` | 按键和鼠标事件 |
| `PTError` | 错误码枚举 |

**注意**：当前为桩实现，所有函数体返回空指针/硬编码值。

### clipboard.rs — 剪贴板管理

| 类型 | 说明 |
|------|------|
| `ClipboardEntry` | 单个剪贴板条目：name、content、created_at、source |
| `ClipboardSource` | Selection / ExplicitCopy / Command |
| `ClipboardBuffer` | 命名缓冲区管理器，支持最大条目限制和系统剪贴板同步 |

### command_correction.rs — AI 命令纠正

| 类型 | 说明 |
|------|------|
| `Correction` | 纠正建议：original、suggested、explanation |
| `build_correction_prompt()` | 构建命令纠正提示（命令、退出码、stderr） |

当命令以非零退出码结束时，构建 AI 提示请求纠正建议。

### default_terminal.rs — macOS 默认终端

| 函数 | 说明 |
|------|------|
| `is_default_terminal()` | 检查 PureTerm 是否为默认终端（读取 LaunchServices 配置） |
| `set_default_terminal()` | 设置为默认终端（macOS System Services） |

仅 macOS 平台有效，Linux/Windows 为空操作。

### dirfreq.rs — 目录频率跟踪（353 行）

| 类型 | 说明 |
|------|------|
| `DirEntry` | 目录条目：path、visit_count、last_visited、alias |
| `DirFrequencyService` | 跟踪目录访问频率，JSON 持久化，自动保存（每 10 次访问） |

默认持久化路径 `~/.config/pureterm/dirfreq.json`，最大 500 条目。用于智能排序和快速跳转。

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

`pub type Result<T> = std::result::Result<T, CoreError>`。

### hotkey.rs — 快捷键系统

| 类型 | 说明 |
|------|------|
| `Modifiers` | 修饰键状态：ctrl、alt、shift、super_key |
| `KeyPattern` | 键 + 修饰键组合 |
| `Hotkey` | id + name + KeyPattern |
| `HotkeysConfig` | 快捷键配置集合 |
| `HotkeyService` | 快捷键查找和匹配 |

内置常量：`CTRL_SHIFT`、`CTRL`、`CTRL_ALT`、`NONE`。14 个内置快捷键（tab_new、tab_close、copy、paste 等）。

### i18n.rs — 国际化支持

| 类型 | 说明 |
|------|------|
| `I18n` | 服务：locale、translations（HashMap 嵌套） |

支持从 TOML 文件加载翻译，`t(key)` 方法查找翻译条目，未找到则返回 key 本身。

### notification.rs — 桌面通知

| 类型 | 说明 |
|------|------|
| `Notification` | 通知：title、body、priority、category |
| `NotificationPriority` | Low / Normal / High / Critical |
| `NotificationCategory` | CommandFinished / Bell / Trigger / Transfer / System / General |

用于长时间运行命令完成、触发器匹配等场景的通知。

### plugin.rs — 插件系统（预留）

| 类型 | 说明 |
|------|------|
| `Plugin` | trait：name、version、on_load、on_unload、register_services |
| `PluginContext` | 插件初始化上下文（预留扩展） |
| `ServiceRegistry` | 服务注册表：register_command |
| `CommandDescriptor` | 命令描述符 |

当前为占位实现，后续将扩展为完整的插件系统。

### prevent_sleep.rs — 系统休眠防止

| 类型 | 说明 |
|------|------|
| `PreventSleep` | 管理 `caffeinate` 子进程 |

- `prevent()` — 启动 `caffeinate -i` 防止空闲休眠
- `allow()` — 终止 caffeinate 进程，允许休眠

与 OSC 133 集成：CommandStart 时防止休眠，OutputEnd 时允许休眠。

### profile.rs — 配置文件管理

| 类型 | 说明 |
|------|------|
| `ProfileType` | Ssh / Serial / Local / Telnet |
| `Profile` | 连接配置：id、name、profile_type、group、options、color、icon |
| `ProfileOptions` | 所有会话类型的配置参数集合（shell、host、port 等） |
| `ProfileProvider` | trait：create_tab_params、get_description、quick_connect |
| `ProfileManager` | 注册和管理 ProfileProvider |

### remote.rs — 远程控制（351 行）

通过 Unix domain socket 进行远程控制命令。

| 类型 | 说明 |
|------|------|
| `RemoteCommand` | 枚举（JSON tag）：OpenTab、CloseTab、SendText、RunCommand、GetState、ListProfiles、SetTheme、ToggleFullscreen、Quit |
| `RemoteResponse` | 响应：Ok(data) / Error(msg) |

支持 `from_json()` / `to_json()` 序列化，socket 路径 `~/.config/pureterm/remote.sock`。

### secret_redaction.rs — 秘密检测和脱敏（204 行）

| 类型 | 说明 |
|------|------|
| `DetectedSecret` | 检测到的秘密：start、end、kind |
| `SecretKind` | AwsAccessKey、AwsSecretKey、GitHubToken、ApiKey、BearerToken、PrivateKey、PasswordInUrl |
| `detect_secrets(text)` | 扫描文本中的秘密模式 |

使用正则表达式匹配常见 API 密钥格式（AWS AKIA、GitHub ghp_、Bearer token 等），用于视觉脱敏防止意外泄露。

### terminfo.rs — Terminfo 数据库

| 常量/函数 | 说明 |
|------|------|
| `TERM_NAME` | `"pureterm"` |
| `TERM_COMPAT` | `"xterm-256color"`（实际使用的兼容 TERM） |
| `install_terminfo()` | 设置 TERM 环境变量 |
| `version_string()` | `PureTerm/<version>` — DA 响应标识 |
| `feature_attributes()` | `"0;95;PureTerm"` — DA2 功能集 |

### theme.rs — 配色方案（391 行）

| 类型 | 说明 |
|------|------|
| `ColorScheme` | 配色方案：name、foreground、background、cursor、selection_*、colors[16] |
| `ThemeService` | 管理内置和自定义配色方案 |

28 种内置配色（Catppuccin Mocha 默认）。方法：`foreground_rgb()`、`background_rgb()`、`cursor_rgb()`、`color_rgb(index)` 都返回 `[u8; 3]`。`load_user_themes(dir)` 从 `.toml` 文件加载自定义主题。

### theme_auto.rs — 自动主题切换

| 类型 | 说明 |
|------|------|
| `ColorMode` | Light / Dark |
| `ThemeAutoSwitch` | 自动切换：light_theme、dark_theme、current_mode |

- `detect_system_mode()` — macOS 检查 `AppleInterfaceStyle`，Linux 检查 GTK 设置
- `check_and_switch()` — 检测系统模式变化并切换配色

### trigger.rs — 输出触发器

| 类型 | 说明 |
|------|------|
| `Trigger` | 触发器规则：id、name、pattern、action、enabled、case_sensitive |
| `TriggerAction` | Notify、PlaySound、Highlight、RunCommand、CopyMatch、Bell |
| `TriggerMatch` | 匹配结果：trigger_id、matched_text、start、end、action |
| `TriggerService` | 触发器管理，正则匹配和动作执行 |

### vault.rs — 加密存储（179 行）

| 类型 | 说明 |
|------|------|
| `EncryptedValue` | 加密值：ciphertext（Base64）、nonce（Base64）、version |
| `VaultService` | AES-256-GCM 加密服务 |
| `VaultError` | 错误类型 |

使用 SHA-256 从主密码派生 32 字节密钥，每次加密生成随机 12 字节 nonce。

## 子模块链接

- [config/AGENTS.md](config/AGENTS.md) — 配置管理子系统（AppConfig、ConfigService、条件配置、验证器、CLI）

## 常见任务

### 添加新的配置项
1. 在对应 Config 结构体中添加字段，设置 `#[serde(default = ...)]`
2. 如需迁移逻辑，在 `ConfigService::load()` 中处理

### 添加新的内置配色
在 `theme.rs` 的 `builtin_color_schemes()` 中使用 `color_scheme!` 宏添加条目。

### 添加新的快捷键
1. 在 `hotkey.rs` 的内置快捷键列表中添加新条目
2. 在 `pureterm-ui` 的 `App::handle_key` 中添加处理逻辑

### 添加新的秘密检测模式
在 `secret_redaction.rs` 的 `SECRET_PATTERNS` 中添加新的 `(Regex, SecretKind)` 条目。

### 使用 vault 加密敏感数据
1. `VaultService::new(master_password)` 创建实例
2. `encrypt(plaintext)` 加密 -> `EncryptedValue`
3. 需要时调用 `decrypt(encrypted_value)` 解密
