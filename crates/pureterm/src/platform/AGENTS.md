# pureterm/src/platform — 平台特定功能

平台相关的窗口扩展功能实现。

## 文件结构

```
mod.rs       # PlatformWindowExt trait 定义
macos.rs     # macOS 平台实现（透明标题栏、红绿灯按钮、DPI 感知宽度查询）
```

## 模块依赖

```
platform/ → pureterm-core（GlassEffectStyle）
```

## PlatformWindowExt trait

平台特定窗口扩展 trait：

```rust
pub trait PlatformWindowExt {
    /// 设置窗口圆角半径
    fn set_corner_radius(&self, radius: f32);

    /// 设置窗口玻璃效果（macOS）
    fn set_glass_effect(&self, style: GlassEffectStyle, blur_radius: u32);
}
```

## macOS 实现

### 透明标题栏（Ghostty 风格）

`apply_transparent_titlebar(ns_window)`:
- `titlebarAppearsTransparent = YES` — 标题栏透明
- `titleVisibility: hidden` — 隐藏标题文字
- `styleMask |= NSFullSizeContentView` — 内容延伸到标题栏区域
- `setMovableByWindowBackground: true` — 允许拖拽移动窗口
- 红绿灯按钮保持原位，无背景色

### 红绿灯按钮宽度查询

`traffic_light_width(window) -> f32`:
- 通过 macOS API 查询三个标准窗口按钮（close=0, miniaturize=1, zoom=2）的实际 frame
- 计算最右按钮的右边缘坐标，加上 20pt 安全间距
- 乘以 `window.scale_factor()` 返回物理像素宽度
- 窗口创建时调用一次，缓存到 `PureTermApp::traffic_light_w`
- 用于标签栏布局（标签起始 x）和点击检测（跳过红绿灯区域）

### 公共 API

| 函数 | 说明 |
|------|------|
| `apply_platform_styling(window, corner_radius, glass_effect)` | 应用透明标题栏 + 确保红绿灯可见 |
| `reposition_traffic_lights_on_resize(window)` | no-op（macOS 自动管理） |
| `traffic_light_width(window) -> f32` | 返回红绿灯区域物理像素宽度（macOS）/ 0.0（其他） |

## 常见任务

### 添加新平台的实现

1. 创建 `platform/<os>.rs`
2. 实现 `PlatformWindowExt` trait
3. 在 `mod.rs` 中添加 `#[cfg(target_os = "...")] mod <os>;`

### 修改透明标题栏行为

编辑 `macos.rs` 的 `apply_transparent_titlebar()` 方法。

### 调整红绿灯区域宽度

编辑 `macos.rs` 的 `traffic_light_width()` 方法（调整安全间距）。

## 父级导航

← [pureterm/AGENTS.md](../AGENTS.md)
