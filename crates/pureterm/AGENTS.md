# pureterm — 二进制入口点

应用程序的 main 入口，负责将所有 crate 组装到一起并驱动事件循环。

src/ 级别模块详解：[src/AGENTS.md](src/AGENTS.md)

## 关键依赖

依赖工作区中其他 6 个 crate：`pureterm-alacritty-runtime`、`pureterm-core`、`pureterm-terminal`、`pureterm-session`、`pureterm-ui`、`pureterm-gui`

默认运行时使用 `pureterm-alacritty-runtime` 的 Alacritty 单窗口终端。旧 `PureTermApp`、wgpu UI、多标签和分屏路径通过 `--legacy-ui` 或 `PURETERM_LEGACY=1` 保留。

外部依赖：默认路径使用 Alacritty/glutin/OpenGL；legacy 路径使用 `winit`（窗口）、`wgpu`（渲染）、`egui` + `egui-wgpu` + `egui-winit`（UI）、`tokio`（异步）、`pollster`（block_on）。

## 文件结构

```
src/                  # 主源代码
├── AGENTS.md         # src/ 目录详解
├── main.rs           # 主入口（CLI + 事件循环）
├── cli.rs            # CLI 参数解析
├── app.rs            # PureTermApp 应用状态
├── event_handler.rs  # 事件处理分发
├── key_input.rs      # 键盘输入处理
├── overlay.rs        # 覆盖层渲染
├── pane_ops.rs       # 窗格操作
├── tab_ops.rs        # 标签页操作
├── accessibility.rs  # 无障碍支持
├── quake_mode.rs     # Quake 模式（dropdown terminal）
├── tray.rs           # 系统托盘图标
├── multi_window.rs   # 多窗口管理
├── single_instance.rs # 单实例锁定
├── global_hotkeys.rs # 全局快捷键
└── platform/         # 平台特定功能
    ├── mod.rs        # PlatformWindowExt trait
    └── macos.rs      # macOS 实现（玻璃效果、圆角占位）
```

## CLI 子命令

| 命令 | 说明 |
|------|------|
| `pureterm list-fonts` | 列出可用字体 |
| `pureterm list-themes` | 列出可用主题 |
| `pureterm show-config` | 显示当前配置（JSON 格式） |
| `pureterm version` | 显示版本信息 |
| `pureterm settings` | 打开设置面板 |
| `pureterm --profile <name>` | 通过配置文件连接 |
| `pureterm --ssh <user@host>` | 快速 SSH 连接 |
| `pureterm --quake` | 下拉终端模式 |
| `pureterm --legacy-ui` | 使用迁移前的 PureTerm UI/runtime |

## 崩溃报告

使用 `sentry` crate 实现本地崩溃报告（不发送网络）：
- 初始化时设置 `dsn: None`（离线模式）
- 崩溃信息存储在本地目录
- 启动时注册 panic hook 捕获崩溃信息（位置、消息）

## 关键类型

### `PureTermApp`（main.rs）

winit `ApplicationHandler` 的实现，持有整个应用状态：

| 字段 | 类型 | 说明 |
|------|------|------|
| `window` | `Option<Arc<Window>>` | winit 窗口 |
| `renderer` | `Option<TerminalRenderer>` | wgpu 渲染器 |
| `surface` | `Option<wgpu::Surface>` | 渲染表面 |
| `app` | `Option<App>` | UI 应用状态 |
| `panes` | `HashMap<u64, TerminalPane>` | 窗格注册表（ID → TerminalPane） |
| `split_root` | `Option<SplitContainer>` | 当前标签页的分屏树根 |
| `focused_pane_id` | `Option<u64>` | 当前聚焦窗格 ID |
| `context_menu` | `ContextMenuState` | 右键菜单状态 |
| `color_scheme` | `ColorScheme` | 当前配色（也用于标签栏主题色） |
| `tab_bar` | `TabBarState` | 标签栏渲染状态（布局、悬浮、dirty 标志、DPI 缩放缓存） |
| `traffic_light_w` | `f32` | 运行时计算的红绿灯区域宽度（物理像素） |
| `tab_list` | `Vec<TabInfo>` | 标签页信息列表 |
| `modifiers` | `ModifiersState` | 修饰键状态 |
| `mouse_pos` | `(f32, f32)` | 最后已知鼠标位置（像素） |
| `clipboard_text` | `String` | 剪贴板内容 |
| `divider_drag` | `Option<DividerDrag>` | 活跃的分隔线拖拽状态 |
| `hovered_divider` | `Option<DividerInfo>` | 悬浮的分隔线（用于光标高亮） |
| `egui_backend` | `Option<EguiBackend>` | egui-wgpu 后端（设置面板） |
| `settings_panel` | `SettingsPanel` | 设置面板状态 |
| `appearance` | `AppearanceConfig` | 外观配置（用于窗口大小变化时重新配置渲染器） |

### `DividerDrag`

| 字段 | 类型 | 说明 |
|------|------|------|
| `container_id` | `u64` | 被拖拽分隔线所属的容器 ID |
| `child_index` | `usize` | 分隔线左侧/上方子元素索引 |
| `direction` | `Direction` | Vertical（左右）或 Horizontal（上下） |
| `start_pos` | `(f32, f32)` | 拖拽起始像素位置 |
| `total_size` | `f32` | 容器在拖拽方向上的总可用尺寸 |

## 核心流程

### 默认启动（Alacritty runtime）
1. `main.rs` 解析 PureTerm CLI 子命令。
2. 无子命令、无 legacy-only 标志时加载 PureTerm 配置。
3. 将字体大小、scrollback、前景/背景色映射到 `PureTermRuntimeConfig`。
4. 调用 `pureterm_alacritty_runtime::run_with_config()` 启动 Alacritty `Processor` / `WindowContext` / `Display`。

### Legacy 启动（`--legacy-ui` / `PURETERM_LEGACY=1`）
1. 创建 winit `Window`
2. 异步初始化 `TerminalRenderer`（wgpu instance/adapter/device/queue），传入 `config.font_size * scale_factor` 作为字体像素大小
3. 从 `AppearanceConfig` 创建 `TransparencyConfig`
4. 配置 surface（设置透明度 alpha_mode 和圆角）
5. 初始化 `EguiBackend`（用 device、surface format、window）
6. 创建初始 `TerminalPane`，注册到 `panes` HashMap
7. 通过 `ConfigService` 加载配置，打开初始标签页
8. macOS: 通过 API 查询红绿灯按钮 frame 计算实际宽度（`traffic_light_width()`），缓存到 `traffic_light_w`

### 事件循环（`window_event`）
- `CloseRequested` → 退出
- `Resized` → 重新配置 surface（传入当前 appearance 配置，重建图集纹理并标记 dirty）+ `resize_all_panes()` + 更新标签栏布局
- `RedrawRequested` → drain 所有窗格 → dirty check（含 tab_bar_dirty）→ 多窗格 snapshot/render → 标签栏覆盖渲染（重置 dirty）→ 可选 egui 覆盖
- `ModifiersChanged` → 记录修饰键状态
- `KeyboardInput` → 设置面板可见时路由给 egui，否则转发给 `handle_key_input`

### 多窗格渲染（`RedrawRequested`）
1. `drain_output()` 所有窗格
2. `compute_layout()` 获取所有窗格的 `PaneRect`（减去标签栏高度 `tab_bar.height`，已缩放为物理像素）
3. 对每个窗格 `build_vertices(snapshot, ..., viewport)` 生成背景+字形顶点
4. 合并所有窗格顶点/索引，添加分隔线、焦点边框、上下文菜单顶点
5. `render_tab_bar_overlay()` 绘制现代圆角标签栏（所有尺寸使用 TabBarState 缓存的物理像素值）
6. `draw_to_surface()` 渲染终端内容（返回 frame 不 present）
7. 若设置面板可见 → `egui_backend.run()` + `render()` 用 `LoadOp::Load` 覆盖 → `frame.present()`
8. 否则直接 `frame.present()`

### 键盘处理（`handle_key_input`）
将 winit `NamedKey` 映射为字符串名称（Escape、Enter、Tab、Backspace、方向键、Home/End、PageUp/PageDown、Insert、Delete、F1-F12），连同 Ctrl+字母和文本输入一起转发给聚焦窗格。

### 鼠标事件处理
- `CursorMoved` → 分隔线拖拽（增量 `adjust_ratio`）→ 标签栏悬浮检测（tab / "+" 按钮）→ 上下文菜单悬浮 → 分隔线悬浮检测 → 窗格鼠标事件
- `MouseInput::Pressed(Left)` → 标签栏点击检测（切换标签 / 关闭标签 / "+" 新建标签）→ 上下文菜单点击 → 分隔线命中检测（`divider_at_point`）→ 窗格点击
- `MouseInput::Pressed(Right)` → 聚焦窗格 → 显示上下文菜单
- `MouseInput::Released` → 清除分隔线拖拽状态 → 窗格释放
- `MouseWheel` → 窗格滚动

### 分屏操作（`handle_split`）
接受 `SplitSide`（Right/Left/Down/Up），推导 `Direction`，调用 `split_pane_directional()`，然后 `equalize()` 等分，最后 `resize_all_panes()`。

### PaneAction 处理
`handle_pane_action` 处理来自 `TerminalPane` 的动作：
- `CopyText(String)` → 更新剪贴板
- `PasteFromClipboard` → 从剪贴板粘贴
- `ShowContextMenu` → 显示上下文菜单
- `OpenUrl` → 打开 URL（平台特定）
- `SplitRight/Left/Down/Up` → 调用 `handle_split`
- `EditTabTitle/EditTabColor` → TODO

### MenuAction 处理
`handle_menu_action` 处理来自上下文菜单的动作，包括 Copy、Paste、SelectAll、Clear、Search、4 方向分屏、关闭窗格/标签页、缩放、OpenSettings（切换 `settings_panel.visible`）。

## 常见任务

### 添加新的窗口事件处理
在 `window_event` 方法中添加 match 分支。修改 `RedrawRequested` 逻辑时注意保持 drain_output → dirty check → multi-pane snapshot → render 的顺序。

### 修改启动流程
编辑 `resumed` 方法。当前只创建 `LocalSession`，如需支持其他会话类型需在此处根据配置选择。

### 修改标签栏
- **布局**：编辑 `update_tab_bar_layout()` — 所有常量乘 scale_factor 缓存到 TabBarState，标签严格均分剩余宽度
- **渲染**：编辑 `render_tab_bar_overlay()` — 使用 TabBarState 缓存的物理像素值，不直接使用常量
- **常量**：编辑 `pureterm-ui/src/tab_bar.rs` — 以逻辑点定义（TAB_BAR_HEIGHT=28 等），运行时缩放
- **红绿灯宽度**：编辑 `platform/macos.rs` 的 `traffic_light_width()` — 运行时查询 macOS API 获取物理像素宽度
- **交互**：编辑鼠标事件处理中的 `CursorMoved` / `MouseInput::Pressed(Left)` 分支 — 悬浮检测和点击处理
- **刷新**：所有标签状态变更处设置 `tab_bar.tab_bar_dirty = true`

### 调试渲染不刷新问题
检查 `RedrawRequested` 中每个窗格的 `drain_output()` 是否被调用、`is_dirty()` 是否返回 true、`snapshot()` 数据是否正确。窗口 resize 后文字消失通常是图集纹理未标记 dirty 导致的——确认 `configure_surface()` 调用了 `self.atlas.mark_dirty()`。

### 修改鼠标处理
编辑鼠标事件处理逻辑。注意分隔线拖拽优先于窗格点击，`pixel_to_cell_for_pane()` 用于坐标转换（考虑窗格偏移）。

### 添加平台特定功能
使用 `#[cfg(target_os = "...")]` 条件编译。现有实现支持 macOS、Linux、Windows。

### 添加新的 PaneAction/MenuAction
1. 在 `pureterm-ui` 中添加枚举变体
2. 在 `handle_pane_action` / `handle_menu_action` 中添加 match 分支
3. 如需多窗格操作，确保操作后调用 `resize_all_panes()` + `window.request_redraw()`
