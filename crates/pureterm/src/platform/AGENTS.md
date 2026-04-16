# pureterm/src/platform — 平台特定功能

平台相关的窗口扩展功能实现。

## 文件结构

```
mod.rs       # PlatformWindowExt trait 定义
macos.rs     # macOS 平台实现
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

macOS 平台特定功能：

```rust
#[cfg(target_os = "macos")]
impl PlatformWindowExt for Window {
    fn set_corner_radius(&self, radius: f32) {
        // 使用 NSWindow  APIs 设置圆角
    }

    fn set_glass_effect(&self, style: GlassEffectStyle, blur_radius: u32) {
        // 使用 NSVisualEffectView 设置玻璃效果
    }
}
```

### GlassEffectStyle

玻璃效果样式枚举：

```rust
pub enum GlassEffectStyle {
    None,
    Regular,     // 常规模糊
    Clear,       // 清晰
    Light,       // 浅色
    Dark,        // 深色
    Automatic,   // 根据系统主题自动选择
}
```

## 常见任务

### 添加新平台的实现

1. 创建 `platform/<os>.rs`
2. 实现 `PlatformWindowExt` trait
3. 在 `mod.rs` 中添加 `#[cfg(target_os = "...")] mod <os>;`

### 修改玻璃效果

编辑 `macos.rs` 的 `set_glass_effect()` 方法。

## 父级导航

← [pureterm/src/AGENTS.md](../AGENTS.md)