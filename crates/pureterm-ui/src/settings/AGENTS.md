# pureterm-ui/src/settings — 设置面板

egui-based 设置界面，用于配置终端外观、行为、配置文件和快捷键。

## 文件结构

```
src/
├── mod.rs         # SettingsPanel + SettingsTab + show() 分发逻辑 (132 行)
├── appearance.rs  # 外观设置标签页：主题/透明度/圆角/玻璃效果/标签栏 (133 行)
├── general.rs     # 通用设置标签页：导入配置/默认终端/安全/退出行为 (187 行)
├── hotkeys.rs     # 快捷键设置标签页：快捷键列表展示 (34 行)
├── profiles.rs    # 配置文件设置标签页：SSH/Local/Serial/Telnet 编辑器 (293 行)
└── terminal.rs    # 终端设置标签页：字体/回滚/光标/连字/复制 (76 行)
```

## 模块依赖

```
settings/ → pureterm-core::config（AppConfig、AppearanceConfig、TerminalConfig、GlassEffectStyle）
settings/ → pureterm-core::hotkey（HotkeysConfig、HotkeyService）
settings/ → pureterm-core::profile（Profile、ProfileOptions、ProfileType）
settings/ → pureterm-core::theme（ThemeService）
settings/ → pureterm-core::config::import（import_from_iterm2、import_from_alacritty）
settings/ → pureterm-core::default_terminal（is_default_terminal、set_default_terminal）
settings/ → egui（Ui、ComboBox、Slider、RichText）
```

## 核心类型

### SettingsPanel（mod.rs）

设置面板主结构，管理标签页状态和配置编辑：

```rust
pub struct SettingsPanel {
    pub visible: bool,
    pub selected_tab: SettingsTab,
    editing_profile: Option<usize>,
    new_profile: Option<Profile>,
    new_profile_type: ProfileType,
    import_status: Option<String>,
    is_default: bool,
}

pub enum SettingsTab {
    General,
    Appearance,
    Terminal,
    Profiles,
    Hotkeys,
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `new() -> Self` | 创建面板（默认隐藏，General 标签） |
| `show(&mut self, ui, config) -> bool` | 绘制设置面板 UI，返回配置是否被修改 |

**show() 分发逻辑：**
1. 顶部标签栏：`[General] [Appearance] [Terminal] [Profiles] [Hotkeys] [Close]`
2. 中央面板：根据 `selected_tab` 分发到对应子模块函数

### ProfileEditResult（mod.rs）

配置文件编辑操作结果：

```rust
pub enum ProfileEditResult {
    Save,   // 用户点击了保存
    Cancel, // 用户点击了取消
    None,   // 尚未采取任何操作
}
```

## 各标签页详细说明

### General 标签页（general.rs）

`general_panel(ui, import_status, is_default, config) -> bool`

包含 4 个分区：

| 分区 | 功能 |
|------|------|
| Import Settings | iTerm2 / Alacritty 配置导入按钮 + 状态消息 |
| Default Terminal | 检测/设置 PureTerm 为默认终端 + 打开系统设置 |
| Security | 密钥脱敏开关（AWS Keys / GitHub Tokens / Bearer tokens / Private keys） |
| Quit Behavior | 退出确认开关 + 阻止系统睡眠开关（macOS caffeinate） |

### Appearance 标签页（appearance.rs）

`appearance_panel(ui, appearance) -> bool`

| 配置项 | 控件 | 说明 |
|--------|------|------|
| 界面主题 | ComboBox | 深色/浅色，同步设置 egui 主题 |
| 终端配色 | ComboBox | 从 ThemeService.list_schemes() 获取可用主题列表 |
| 显示标签栏 | Checkbox | 控制标签栏可见性 |
| 显示标题栏 | Checkbox | 控制标题栏可见性 |
| 标签位置 | ComboBox | top / bottom |
| 透明度 | Slider (0.1-1.0) | 窗口透明度 |
| 圆角半径 | Slider (0.0-24.0) | 窗口圆角 |
| 玻璃效果 | ComboBox | 无/常规/透明/浅色/深色/自动 |
| 背景模糊 | Slider (0-100) | 背景模糊程度 |

### Terminal 标签页（terminal.rs）

`terminal_panel(ui, terminal) -> bool`

| 配置项 | 控件 | 说明 |
|--------|------|------|
| Font | TextEdit | 字体族名称 |
| Font path | TextEdit | 可选自定义字体路径 |
| Font size | Slider (8-32) | 字体大小 |
| Scrollback lines | Slider (100-100000) | 回滚缓冲区行数 |
| Cursor style | ComboBox | block / underline / beam |
| Cursor blink | Checkbox | 光标闪烁 |
| Font ligatures | Checkbox | 启用连字 |
| Copy on select | Checkbox | 选择时自动复制 |

### Profiles 标签页（profiles.rs）

`profiles_panel(ui, editing_profile, new_profile, new_profile_type, config) -> (bool, Option<Profile>)`

**配置文件列表：**
- 每个配置文件显示：颜色指示条 + 名称 + 类型 + 主机/用户信息
- 操作按钮：Edit / Delete

**新建配置文件：**
- 类型选择：SSH / Local / Serial / Telnet
- 点击 "+ Add" 进入编辑器

**配置文件编辑器（profile_editor）：**

| 字段 | 控件 | 说明 |
|------|------|------|
| Name | TextEdit | 配置文件名称 |
| Type | Label（只读） | 创建后不可更改 |
| Color | TextEdit + 色块预览 | hex 颜色值 |

**类型特定字段：**

| 类型 | 字段 |
|------|------|
| SSH/Telnet | Host, Port, User(仅SSH), Auth Method(仅SSH: password/publickey/agent) |
| Serial | Path, Baud Rate |
| Local | Shell, CWD |

**辅助函数：**
- `parse_color_string(s) -> [u8; 3]` — 解析 hex 颜色字符串

### Hotkeys 标签页（hotkeys.rs）

`hotkeys_panel(ui, hotkeys)` — 无返回值（只读展示）

- 使用 HotkeyService 遍历所有快捷键
- 每行显示：快捷键名称 + 修饰键+键位（Ctrl+Alt+Shift+Key）
- 当前为只读列表，自定义快捷键支持待实现

## UI 布局

```
┌─────────────────────────────────────────────────────────────────┐
│  [General] [Appearance] [Terminal] [Profiles] [Hotkeys] [Close]│
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─ General ──────────────────────────────────────────────┐    │
│  │ Import Settings:  [Import from iTerm2] [Alacritty]     │    │
│  │ Default Terminal: PureTerm is [not] the default        │    │
│  │ Security:         [x] Redact secrets                   │    │
│  │ Quit Behavior:    [x] Confirm quit  [x] Prevent sleep  │    │
│  └────────────────────────────────────────────────────────┘    │
│                                                                 │
│  ┌─ Appearance ───────────────────────────────────────────┐    │
│  │ 界面主题: [深色 ▾]    终端配色: [dark ▾]               │    │
│  │ [x] 显示标签栏  [x] 显示标题栏  标签位置: [top ▾]      │    │
│  │ 透明度: ───●──────  圆角半径: ──●────────              │    │
│  │ 玻璃效果: [自动 ▾]   背景模糊: ────●────              │    │
│  └────────────────────────────────────────────────────────┘    │
│                                                                 │
│  ┌─ Terminal ─────────────────────────────────────────────┐    │
│  │ Font: [JetBrains Mono]  Font path: [(default)]         │    │
│  │ Font size: ──●───  Scrollback: ──────●────             │    │
│  │ Cursor style: [block ▾]  [x] Cursor blink             │    │
│  │ [x] Font ligatures  [x] Copy on select                │    │
│  └────────────────────────────────────────────────────────┘    │
│                                                                 │
│  ┌─ Profiles ─────────────────────────────────────────────┐    │
│  │ ● My Server (SSH)         Host: example.com:22         │    │
│  │                              [Edit] [Delete]           │    │
│  │ Type: [SSH ▾] [+ Add]                                  │    │
│  └────────────────────────────────────────────────────────┘    │
│                                                                 │
│  ┌─ Hotkeys ──────────────────────────────────────────────┐    │
│  │ New Tab          Ctrl+Shift+T                          │    │
│  │ Close Tab        Ctrl+Shift+W                          │    │
│  │ Copy             Ctrl+Shift+C                          │    │
│  │ Custom keybinding support coming soon.                 │    │
│  └────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

## 与其他模块的交互

```
SettingsPanel → AppConfig → ConfigService → 持久化
SettingsPanel → AppearanceConfig → TerminalRenderer → 重新配置
SettingsPanel → HotkeyService → App::handle_key → 生效
SettingsPanel → import::import_from_iterm2 / import_from_alacritty → 配置导入
SettingsPanel → default_terminal::set_default_terminal → 系统注册
SettingsPanel → ThemeService::list_schemes → 可用主题列表
```

## 常见任务

### 添加新的设置项

1. 在 `pureterm-core/src/config/legacy.rs` 对应配置结构体中添加字段
2. 在对应标签页文件中添加 egui 控件
3. 控件的 `.changed()` 返回值合并到函数返回的 `changed` 标志

### 添加新的设置标签页

1. 在 `mod.rs` 的 `SettingsTab` 枚举中添加变体
2. 创建新的标签页文件（如 `new_tab.rs`），实现 `pub(crate) fn new_tab_panel(ui, ...) -> bool`
3. 在 `mod.rs` 中声明 `mod new_tab;`
4. 在 `show()` 方法的 `match selected_tab` 中添加分发分支
5. 在标签栏 `ui.horizontal` 中添加按钮

### 修改设置面板布局

编辑 `mod.rs` 的 `show()` 方法，更改标签栏或面板分发结构。

### 添加新的导入源

1. 在 `pureterm-core/src/config/import.rs` 中实现导入函数
2. 在 `general.rs` 的 Import Settings 分区中添加导入按钮

## 父级导航

← [src/AGENTS.md](../AGENTS.md)
