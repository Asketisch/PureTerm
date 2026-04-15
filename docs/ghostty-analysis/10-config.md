# 10 - 配置系统

## 功能概述

完整的配置管理：100+ 配置选项、文件加载/保存、CLI 参数解析、主题系统、条件配置（light/dark）、验证、序列化、C API。

## Ghostty 源码文件

| 文件 | 行数 | 功能 |
|------|------|------|
| `config/Config.zig` | ~11000 | **主配置结构**，100+ 选项 |
| `config/CApi.zig` | ~200 | C API 导出 |
| `config/c_get.zig` | ~200 | C 兼容的值获取 |
| `config/ClipboardCodepointMap.zig` | ~100 | 剪贴板码点映射 |
| `config/RepeatableStringMap.zig` | ~100 | 可重复键值映射 |
| `config/command.zig` | ~200 | 命令类型（shell/direct） |
| `config/conditional.zig` | ~100 | 条件配置（light/dark 主题） |
| `config/edit.zig` | ~100 | 配置文件编辑 |
| `config/ErrorList.zig` | ~50 | 错误收集 |
| `config/file_load.zig` | ~150 | 默认文件路径解析 |
| `config/formatter.zig` | ~200 | 值格式化 |
| `config/formatter_file.zig` | ~200 | 文件格式化 |
| `config/io.zig` | ~100 | IO 类型 |
| `config/key.zig` | ~50 | 配置键枚举（comptime 生成） |
| `config/path.zig` | ~200 | 路径类型 |
| `config/string.zig` | ~100 | 字符串解析 |
| `config/theme.zig` | ~200 | 主题发现和加载 |
| `config/url.zig` | ~300 | 默认 URL 正则 |
| `config/Wasm.zig` | ~50 | WASM 配置 API |

## 配置选项完整清单（100+ 项）

### 字体配置
```
font-family, font-family-bold, font-family-italic, font-family-bold-italic
font-style, font-style-bold, font-style-italic, font-style-bold-italic
font-synthetic-style (packed struct: bold/italic/bold_italic)
font-feature, font-size, font-thicken, font-thicken-strength
font-variation, font-variation-bold/italic/bold-italic
font-codepoint-map, font-shaping-break
```

### 颜色/主题
```
theme, background, foreground, palette, palette-generate, palette-harmonious
cursor-color, cursor-opacity, cursor-style, cursor-style-blink, cursor-text
selection-foreground, selection-background, selection-clear-on-typing/copy
selection-word-chars, minimum-contrast, bold-color, faint-opacity
search-foreground/background, search-selected-foreground/background
background-image, background-image-opacity/position/fit/repeat
alpha-blending
```

### 渲染/度量
```
adjust-cell-width/height, adjust-font-baseline
adjust-underline/strikethrough/overline position/thickness
adjust-cursor thickness/height, adjust-box-thickness, adjust-icon-height
grapheme-width-method, freetype-load-flags
custom-shader, custom-shader-animation
```

### 窗口/布局
```
window-padding-x/y, window-padding-balance, window-padding-color
window-vsync, window-decorations, window-title-font-family
window-subtitle, window-theme, window-colorspace
window-height/width, window-position-x/y, window-save-state
window-step-resize, window-new-tab-position, window-show-tab-bar
window-titlebar-background/foreground
window-inherit-working-directory/font-size
maximize, fullscreen, title, class
```

### 背景/不透明度
```
background-opacity, background-opacity-cells, background-blur
unfocused-split-opacity/fill, split-divider-color, split-preserve-zoom
```

### 鼠标/键盘/输入
```
mouse-hide-while-typing, mouse-shift-capture, mouse-reporting
mouse-scroll-multiplier
clipboard-read/write, clipboard-trim-trailing-spaces
clipboard-paste-protection, clipboard-paste-bracketed-safe
clipboard-codepoint-map
keybind, key-remap, scroll-to-bottom, click-repeat-interval
right-click-action, vt-kam-allowed
```

### 命令/进程
```
command, initial-command, wait-after-command
abnormal-command-exit-runtime, notify-on-command-finish/...
env, input
```

### 滚动/回滚
```
scrollback-limit, scrollbar
```

### 链接
```
link, link-url, link-previews
```

### Shell 集成
```
shell-integration, shell-integration-features
```

### Quick Terminal (Quake 风格)
```
quick-terminal-position/size/animation-duration/autohide/space-behavior/screen/...
```

### macOS 专用
```
macos-non-native-fullscreen, macos-window-buttons
macos-titlebar-style/proxy-icon, macos-dock-drop-behavior
macos-option-as-alt, macos-window-shadow, macos-hidden
macos-auto-secure-input/secure-input-indication
macos-applescript, macos-icon/custom-icon/icon-frame/icon-ghost-color/icon-screen-color
macos-shortcuts, auto-update/auto-update-channel
```

### GTK/Linux 专用
```
gtk-opengl-debug, gtk-single-instance, gtk-titlebar
gtk-tabs-location, gtk-titlebar-hide-when-maximized
gtk-toolbar-style, gtk-titlebar-style, gtk-wide-tabs, gtk-custom-css
linux-cgroup/memory-limit/processes-limit/hard-fail
x11-instance-name
```

### 其他
```
config-file, config-default-files
confirm-close-surface, quit-after-last-window-closed/...
initial-window, undo-timeout, focus-follows-mouse
copy-on-select, title-report, image-storage-limit
term, enquiry-response, async-backend
desktop-notifications, progress-style, app-notifications
bell-features/audio-path/audio-volume, osc-color-report-format
command-palette-entry, resize-overlay/position/duration
working-directory, language
```

## 配置加载流程

```
1. default()           → 创建默认值
2. loadDefaultFiles()  → 加载 XDG/AppSupport 配置文件
3. loadCliArgs()       → 解析命令行参数
4. loadRecursiveFiles()→ 加载引用的配置文件（检测循环）
5. finalize()          → 验证、主题加载、默认值填充
```

### 条件配置

```ini
# 根据系统主题切换
if theme == light {
  theme = "light-theme"
} else {
  theme = "dark-theme"
}

# 根据操作系统
if os == macos {
  font-size = 13
}
```

## 纯 Rust 实现要点

1. **用 serde 反序列化**：`serde + toml` 或自定义格式
2. **每个配置选项一个字段**：用 `#[serde(default)]` 处理默认值
3. **条件配置用标记**：解析时记录条件块
4. **验证在 `finalize()` 中完成**：clamp、默认值、主题加载
5. **主题文件用简单解析器**：key = value 格式

### Rust 结构体框架

```rust
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    // 字体
    pub font_family: RepeatableString,
    pub font_size: f32,
    // 颜色
    pub background: Color,
    pub foreground: Color,
    pub palette: Palette,
    // 窗口
    pub window_padding_x: WindowPadding,
    pub window_padding_y: WindowPadding,
    pub background_opacity: f64,
    // 键绑定
    pub keybind: Keybinds,
    pub key_remap: KeyRemapSet,
    // 命令
    pub command: Option<Command>,
    pub shell_integration: ShellIntegration,
    // ... 100+ 字段
}
```
