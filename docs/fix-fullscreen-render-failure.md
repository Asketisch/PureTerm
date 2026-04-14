# 终端启动全屏渲染失败根因分析

## 1. Shader 坐标转换 bug（根本原因）

`@builtin(position)` 在 WGSL fragment shader 中是**帧缓冲像素坐标**（0 到 screen_width），不是裁剪空间（-1 到 1）。之前代码错误地对已经是像素坐标的值再做一次裁剪空间→像素转换：

```wgsl
// 错误：double transform，500px → 512512
let pixel_x = (in.position.x + 1.0) * 0.5 * screen_w;
```

当 `corner_radius > 0`（默认 16.0）时，所有像素坐标变成巨大数值，全部命中圆角 `discard` 检查，整个屏幕被丢弃变空白。

**正确写法：** 直接用 `in.position.x` / `in.position.y`。

**关键知识：** vertex shader 输出裁剪空间坐标，但 GPU 光栅化后 `@builtin(position)` 已自动转换为帧缓冲坐标，fragment 阶段不需要再转换。

**涉及文件：** `crates/pureterm-gui/shaders/terminal.wgsl`

## 2. PTY 输出不触发重绘

后台线程通过 `Arc<Mutex<Vec<u8>>>` 缓冲 PTY 数据，但没有唤醒 winit 事件循环。`about_to_wait()` 为空，事件循环休眠后无法感知新数据到达。

**修复：** 通过 `EventLoopProxy::send_event(())` 从输出线程唤醒事件循环，在 `user_event()` 中调用 `request_redraw()`。

**防范：** 任何后台线程需要触发 UI 更新时，必须通过 `EventLoopProxy` 唤醒事件循环，不能依赖轮询。

**涉及文件：** `crates/pureterm/src/main.rs`、`crates/pureterm-ui/src/terminal/pane.rs`

## 3. Surface Occluded 首帧丢失

macOS 窗口创建初期 `get_current_texture()` 返回 `Occluded`，但 `pane.clear_dirty()` 在 `draw_to_surface()` 之前已被调用，dirty 标记被清除后不会重试渲染。

**修复：** `draw_to_surface` 返回 None 时，重新 `mark_dirty()` 所有 pane 并 `request_redraw()`。

**防范：** 渲染失败时不要丢弃状态，确保能重试。dirty 标记应在渲染成功提交后才清除，或失败时恢复。

**涉及文件：** `crates/pureterm/src/main.rs`、`crates/pureterm-ui/src/terminal/pane.rs`
