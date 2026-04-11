# pureterm — 二进制入口点

应用程序的 main 入口，负责将所有 crate 组装到一起并驱动事件循环。

## 关键依赖

依赖工作区中其他 5 个 crate：`pureterm-core`、`pureterm-terminal`、`pureterm-session`、`pureterm-ui`、`pureterm-gui`

外部依赖：`winit`（窗口）、`wgpu`（渲染）、`egui` + `egui-wgpu` + `egui-winit`（UI）、`tokio`（异步）、`pollster`（block_on）。

## 文件结构

```
src/
└── main.rs          # 唯一源文件
```

## 关键类型

### `PureTermApp`（main.rs）

winit `ApplicationHandler` 的实现，持有整个应用状态：

| 字段 | 类型 | 说明 |
|------|------|------|
| `window` | `Option<Arc<Window>>` | winit 窗口 |
| `renderer` | `Option<TerminalRenderer>` | wgpu 渲染器 |
| `surface` | `Option<wgpu::Surface>` | 渲染表面 |
| `app` | `Option<App>` | UI 应用状态 |
| `terminal_pane` | `Option<TerminalPane>` | 终端面板 |
| `color_scheme` | `ColorScheme` | 当前配色 |
| `modifiers` | `ModifiersState` | 修饰键状态 |

## 核心流程

### 启动（`resumed`）
1. 创建 winit `Window`
2. 异步初始化 `TerminalRenderer`（wgpu instance/adapter/device/queue）
3. 配置 surface
4. 创建 `LocalSession` 并启动
5. 创建 `TerminalPane`（终端 + session）
6. 通过 `ConfigService` 加载配置，打开初始标签页

### 事件循环（`window_event`）
- `CloseRequested` → 退出
- `Resized` → 重新配置 surface + 调整终端/pane 尺寸
- `RedrawRequested` → drain_output → is_dirty → snapshot → render
- `ModifiersChanged` → 记录修饰键状态
- `KeyboardInput` → 转发给 `handle_key_input`

### 键盘处理（`handle_key_input`）
将 winit `NamedKey` 映射为字符串名称（Escape、Enter、Tab、Backspace、方向键、Home/End、PageUp/PageDown、Insert、Delete、F1-F12），连同 Ctrl+字母和文本输入一起转发给 `TerminalPane`。

## 常见任务

### 添加新的窗口事件处理
在 `window_event` 方法中添加 match 分支。修改 `RedrawRequested` 逻辑时注意保持 drain_output → dirty check → snapshot → render 的顺序。

### 修改启动流程
编辑 `resumed` 方法。当前只创建 `LocalSession`，如需支持其他会话类型需在此处根据配置选择。

### 调试渲染不刷新问题
检查 `RedrawRequested` 中 `pane.drain_output()` 是否被调用、`is_dirty()` 是否返回 true、`snapshot()` 数据是否正确。
