# pureterm-ui/src/settings — 设置面板

egui-based 设置界面，用于配置终端外观、行为和快捷键。

## 文件结构

```
src/
└── mod.rs        # SettingsPanel + 相关组件（约 605 行）
```

## 模块依赖

```
settings/ → lib.rs（App、TerminalPane）
settings/ → pureterm-core（AppConfig、AppearanceConfig、TerminalConfig）
settings/ → pureterm-gui（TerminalRenderer）
```

## 核心类型

### SettingsPanel（mod.rs）

设置面板状态：

```rust
pub struct SettingsPanel {
    visible: bool,
    current_tab: SettingsTab,
    scroll_offset: f32,
}

pub enum SettingsTab {
    General,
    Appearance,
    Terminal,
    Hotkeys,
    Profiles,
    Advanced,
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `show(&mut self, ui: &mut Ui)` | 绘制设置面板 UI |
| `set_visible(&mut self, visible: bool)` | 显示/隐藏面板 |
| `is_visible(&self) -> bool` | 检查是否可见 |
| `current_tab(&self) -> SettingsTab` | 获取当前标签页 |

### 设置项结构

| 字段 | 类型 | 说明 |
|------|------|------|
| `appearance` | `AppearanceConfig` | 外观配置 |
| `terminal` | `TerminalConfig` | 终端配置 |
| `hotkeys` | `HotkeysConfig` | 快捷键配置 |

### 外观设置（Appearance）

| 配置项 | 说明 |
|--------|------|
| `tabs_location` | 标签页位置（Top/Bottom/Left/Right） |
| `theme` | 配色方案名称 |
| `frame` | 窗口边框样式 |
| `opacity` | 窗口透明度（0.0-1.0） |
| `show_tab_bar` | 是否显示标签页栏 |
| `show_title_bar` | 是否显示标题栏 |
| `corner_radius` | 窗口圆角半径 |
| `glass_effect` | 玻璃效果样式 |
| `background_blur` | 背景模糊程度 |

### 终端设置（Terminal）

| 配置项 | 说明 |
|--------|------|
| `font_family` | 字体名称 |
| `font_size` | 字体大小（像素） |
| `scrollback_lines` | 回滚缓冲区行数 |
| `renderer` | 渲染器类型 |
| `bell` | 响铃设置 |
| `ligatures` | 是否启用连字 |
| `cursor_style` | 光标样式（Block/Underline/Beam） |
| `cursor_blink` | 光标是否闪烁 |
| `copy_on_select` | 选择时自动复制 |
| `color_scheme` | 颜色方案 |

### 快捷键设置（Hotkeys）

| 配置项 | 说明 |
|--------|------|
| `tab_new` | 新建标签页 |
| `tab_close` | 关闭标签页 |
| `copy` | 复制 |
| `paste` | 粘贴 |
| `search` | 搜索 |
| `split_right` | 右分割 |
| `split_down` | 下分割 |

## UI 布局

```
┌─────────────────────────────────────────────┐
│  Settings                            [×]    │
├─────────────────────────────────────────────┤
│ [General] [Appearance] [Terminal] [Hotkeys]│
├─────────────────────────────────────────────┤
│                                             │
│  设置项内容区域                              │
│  （根据当前标签页显示不同内容）              │
│                                             │
│                                             │
└─────────────────────────────────────────────┘
```

### General 标签

- 启动选项
- 默认 shell
- 会话行为

### Appearance 标签

- 主题选择（下拉列表）
- 透明度滑块
- 圆角滑块
- 玻璃效果选项

### Terminal 标签

- 字体选择
- 字体大小
- 回滚行数
- 光标样式
- 连字开关

### Hotkeys 标签

- 快捷键列表
- 点击编辑快捷键

## 事件处理

SettingsPanel 通过 egui 事件与用户交互：

```rust
impl SettingsPanel {
    pub fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        // 处理 Escape 键关闭设置面板
        // 处理快捷键
    }
}
```

## 与其他模块的交互

```
SettingsPanel → AppConfig → ConfigService → 持久化
SettingsPanel → AppearanceConfig → TerminalRenderer → 重新配置
SettingsPanel → HotkeyService → App::handle_key → 生效
```

## 常见任务

### 添加新的设置项

1. 在对应配置结构体中添加字段
2. 在 `SettingsPanel::draw()` 中添加 UI 控件
3. 在保存时更新配置

### 修改设置面板布局

编辑 `mod.rs` 的 `show()` 方法，更改 UI 结构。

### 添加新的设置标签页

1. 在 `SettingsTab` 枚举中添加变体
2. 在 `show()` 方法中添加标签页内容

## 父级导航

← [pureterm-ui/AGENTS.md](../AGENTS.md)