# pureterm/src -- 二进制入口源代码

应用程序的 main 入口，负责将所有 crate 组装到一起并驱动事件循环。

## 关键依赖

依赖工作区中其他 5 个 crate：`pureterm-core`、`pureterm-terminal`、`pureterm-session`、`pureterm-ui`、`pureterm-gui`

外部依赖：`winit`（窗口）、`wgpu`（渲染）、`egui` + `egui-wgpu` + `egui-winit`（UI 覆盖层）、`clap`（CLI）、`arboard`（剪贴板）、`objc2` + `objc2-foundation`（macOS 平台 API）。

## 文件结构

```
src/                          # 主源代码
├── main.rs             (183) # 主入口，初始化日志/sentry/CLI，创建 PureTermApp 进入事件循环
├── app.rs              (318) # PureTermApp struct（winit ApplicationHandler），持有窗口/渲染器/面板/标签栏等所有运行时状态
├── cli.rs              (168) # CLI 参数解析（clap），支持子命令和连接选项
├── event_handler.rs   (1549) # 事件处理核心，包含 window_event、redraw、keyboard、mouse 的完整处理逻辑
├── global_hotkeys.rs   (125) # 全局快捷键注册（系统范围，应用未聚焦时也能工作）
├── key_input.rs        (209) # 键盘输入映射和编码，路由到聚焦面板
├── multi_window.rs     (104) # 多窗口管理（WindowState 注册/注销/聚焦）
├── overlay.rs          (296) # 覆盖层渲染（上下文菜单、标签栏的 GPU 顶点生成）
├── pane_ops.rs         (331) # 窗格操作（创建、关闭、分割、Profile 连接、PaneAction 处理）
├── quake_mode.rs       (101) # Quake 下拉终端模式（屏幕顶部滑出的终端窗口）
├── single_instance.rs   (97) # 单实例锁定（TCP 端口绑定 + IPC 转发参数）
├── tab_ops.rs          (238) # 标签操作（创建、关闭、切换、MenuAction 处理）
├── tray.rs              (74) # 系统托盘图标（跨平台占位）
├── accessibility.rs    (321) # 无障碍支持（VoiceOver、AccessibilitySnapshot、macOS NSAccessibility）
└── platform/                 # 平台特定功能
    ├── mod.rs           (17) # PlatformWindowExt trait 定义
    └── macos.rs        (252) # macOS 平台实现（透明标题栏、红绿灯按钮、圆角、玻璃效果）
```

## 模块详情

### main.rs
主入口点。按顺序执行：初始化 env_logger（debug 模式默认 debug 级别，release 默认 warn）、初始化 sentry（dsn: None 离线模式，捕获 panic 到本地文件）、解析 CLI 参数、处理子命令（list-fonts/list-themes/show-config/version/settings）、加载 ConfigService 和 ThemeService、处理 `--ssh` 快速连接、创建 winit EventLoop、构造 PureTermApp、调用 `event_loop.run_app()`。

### app.rs
定义 `PureTermApp` struct 及其核心方法。持有窗口/渲染器/面板注册表/标签栏/上下文菜单/egui 后端等全部运行时状态。关键方法：
- `focused_pane()` / `focused_pane_mut()` -- 获取当前聚焦面板
- `show_context_menu()` -- 显示右键菜单并测量宽度
- `compute_layout()` -- 计算所有面板矩形（减去标签栏高度）
- `update_tab_bar_layout()` -- 更新标签栏布局（均分宽度、缩放缓存）
- `resize_all_panes()` -- 根据布局调整所有面板尺寸
- `screen_size()` / `pixel_to_cell_for_pane()` -- 坐标转换辅助

还定义了 `RenameModalState`（标签页重命名模态框）、`ColorModalState`（标签页颜色编辑模态框）、`DividerDrag`（分隔线拖拽状态）。

### cli.rs
使用 clap 定义 CLI 参数：`--profile`（通过名称连接配置文件）、`--ssh`（快速 SSH 连接，解析 user@host:port）、`--new-tab`（IPC 新标签页）、`--quake`（下拉终端模式）。子命令：List、Settings、ListFonts、ListThemes、ShowConfig、Version。

辅助函数：`parse_ssh_spec()` 将 SSH 字符串解析为 Profile、`profile_to_connect_params()` 将 Profile 转为 ConnectParams（支持 Local/SSH/Serial/Telnet 四种会话类型）、`profile_title()` 生成标签页标题。

### event_handler.rs
最大文件，实现 `ApplicationHandler` trait for `PureTermApp`。核心方法：
- `resumed()` -- 首次启动时创建窗口/渲染器/surface/egui 后端/初始面板/标签页，应用平台样式，设置 VoiceOver 和 IME
- `window_event()` -- 事件路由核心：
  - `Focused` -- 转发 DEC Focus In/Out (mode 1004) 到面板
  - `CloseRequested` -- 退出确认对话框（confirm_quit + has_active）
  - `Resized` -- 重新配置 surface + resize_all_panes + macOS 红绿灯重定位
  - `RedrawRequested` -- 完整渲染管线：drain_output -> pending_marks -> dirty check -> 多面板 snapshot -> build_vertices -> 分隔线/焦点边框 -> 标签栏/上下文菜单覆盖层 -> draw_to_surface -> 可选 egui 覆盖层
  - `KeyboardInput` -- 映射 winit NamedKey 到字符串，调用 handle_key_input
  - `CursorMoved` -- 分隔线拖拽 -> 标签栏悬浮 -> 上下文菜单悬浮 -> 分隔线悬浮 -> 面板鼠标事件
  - `MouseInput` -- 左键（标签栏/菜单/分隔线/面板点击）、右键（上下文菜单）、中键（面板）
  - `MouseWheel` -- 面板滚动
  - `DroppedFile` -- 拖放文件到面板
  - `Ime` -- CJK 输入法 preedit/commit
- `about_to_wait()` -- 光标闪烁定时重绘
- `user_event()` -- PTY 输出线程唤醒（request_redraw）

### global_hotkeys.rs
系统范围快捷键管理。定义 `GlobalHotkeyManager`（注册/检查热键）、`HotkeyCombo`（Ctrl+Alt+Shift+Super+Key）、`GlobalHotkeyAction`（ToggleVisibility/ShowQuake/NewTab/CommandPalette）。默认注册 Ctrl+` 显示 Quake、Ctrl+Shift+` 切换可见性。当前为框架代码，平台特定注册待实现。

### key_input.rs
键盘输入路由，作为 `PureTermApp` 的 impl 块。`handle_key_input()` 方法按优先级处理：
1. Popup 导航（autocomplete/history 的上下/Enter/Escape）
2. Ctrl+Space 显示 autocomplete、Ctrl+R 显示 history、Ctrl+/ 消除 correction
3. Enter 时记录命令到历史
4. 全局复制/粘贴（macOS: Cmd+C/V，其他: Ctrl+Shift+C/V）
5. Ctrl+Shift+P 打开 profile selector
6. 全选（macOS: Cmd+A，其他: Ctrl+Shift+A）
7. 应用热键服务
8. 缩放快捷键（Ctrl+=/-/0）
9. 委托到面板 handle_key（macOS 上阻止无法识别的 Cmd 组合转发到终端）

### multi_window.rs
多窗口管理框架。`MultiWindowManager` 管理 `HashMap<WindowId, WindowState>`，支持窗口注册/注销/聚焦/Quake 标记。当前为占位实现，未来支持多窗口场景。

### overlay.rs
GPU 覆盖层顶点生成，作为 `PureTermApp` 的 impl 块。两个关键方法：
- `render_context_menu_overlay()` -- 渲染上下文菜单（圆角矩形背景 8pt 半径、分隔线、悬浮高亮 4pt、文本、边框）
- `render_tab_bar_overlay()` -- 渲染 Ghostty 风格标签栏（背景 x0.85 暗化、底部分隔线、标签页圆角矩形 6pt、关闭按钮圆形 24pt、颜色指示条、"+" 新建按钮、文本截断）

还包含 `parse_hex_color()` 辅助函数（十六进制颜色字符串转 [u8; 3]）。

### pane_ops.rs
窗格生命周期操作，作为 `PureTermApp` 的 impl 块。核心方法：
- `create_pane()` / `create_pane_with_session()` -- 创建 TerminalPane 并启动 Session（支持 Local/SSH/Serial/Telnet/Tmux），设置 OSC 52 剪贴板、OSC 133 mark 回调（prevent_sleep）、PTY 输出 waker
- `handle_pane_action()` -- 处理 PaneAction（Copy/Paste/ShowContextMenu/OpenUrl/Split*/EditTabTitle/EditTabColor）
- `connect_with_profile()` -- 从 Profile 创建面板并打开新标签页
- `handle_split()` -- 分屏操作（SplitSide -> Direction -> create_pane -> split_pane_directional -> equalize -> resize）
- `handle_close_pane()` -- 关闭面板并从 split_root 移除

### quake_mode.rs
Quake 下拉终端模式。`QuakeMode` struct 持有 enabled/visible/height(0.0-1.0)/animation_ms/screen_index/always_on_top。`toggle()` 返回 QuakeAction::Show/Hide。`apply_window_attrs()` 设置窗口尺寸和位置（屏幕顶部）。Ctrl+` 全局热键触发。

### single_instance.rs
单实例锁定，使用 TCP 端口 19538 绑定实现。`try_claim()` 尝试绑定端口（成功则为主实例）。`send_to_running()` 通过 IPC 发送参数到运行中实例（`\0` 分隔）。`check_incoming()` 非阻塞接收其他实例的消息。空消息视为"bring-to-front"请求。

### tab_ops.rs
标签页操作，作为 `PureTermApp` 的 impl 块。核心方法：
- `handle_menu_action()` -- 处理上下文菜单动作（Copy/Paste/SelectAll/Clear/Search/Split*/ClosePane/NewTab/CloseTab/Zoom*/OpenSettings/EditTabTitle/EditTabColor）
- `handle_new_tab()` -- 创建新终端面板并打开标签页
- `handle_close_tab()` -- 关闭活动标签页
- `close_tab_by_id()` -- 通过 ID 关闭标签页，清理面板，切换到新活动标签页（若无剩余标签则自动创建新标签）

### tray.rs
系统托盘图标框架。`SystemTray` struct 管理 visible/tooltip，支持 show/hide。`TrayEvent`（Click/RightClick/DoubleClick）映射到 `TrayAction`（ToggleWindow/ShowMenu/OpenNewTab/Quit）。当前为占位实现，平台特定创建待实现。

### accessibility.rs
无障碍支持，分跨平台和 macOS 平台两部分：
- `AccessibilitySnapshot` -- 终端内容快照（lines、cursor 位置、word_at_cursor、full_text）
- `AccessibilityAnnouncer` -- VoiceOver 播报（cursor 位置/行号/选区文本），支持 enabled 开关
- macOS 平台模块（`platform`）：
  - `setup_accessibility()` -- 通过 objc2 设置 NSView 的 AXTextArea role、AXTerminal subrole、label
  - `update_accessibility_value()` -- 更新 AX value 为当前终端文本
  - `announce()` -- 通过 NSWorkspace accessibilityAnnounce 播报
- 非 macOS 平台为空实现
- 包含单元测试（word_at_cursor、full_text、announcer）

## 子模块

- [platform/AGENTS.md](platform/AGENTS.md) -- 平台特定窗口扩展（PlatformWindowExt trait、macOS 透明标题栏/红绿灯按钮/圆角/玻璃效果）

## 核心流程

### 启动（`resumed`）
1. 创建 winit `Window`
2. 异步初始化 `TerminalRenderer`（wgpu），传入 `config.font_size * scale_factor`
3. 从 `AppearanceConfig` 创建 `TransparencyConfig`
4. 配置 surface（透明度 + 圆角）
5. 初始化 `EguiBackend`（设置 CJK 字体 + 应用主题）
6. 创建初始 `TerminalPane`，注册到 `panes` HashMap
7. 打开初始标签页，隐藏欢迎页面
8. macOS: 设置 VoiceOver、启用 IME、查询红绿灯宽度

### 渲染管线（`RedrawRequested`）
1. `drain_output()` 所有面板
2. 处理 pending OSC 133 marks（prevent_sleep）
3. 检测外观/主题变更，标记面板 dirty
4. dirty check（含 tab_bar_dirty）
5. `compute_layout()` 计算面板矩形
6. 多面板 snapshot + `build_vertices()`
7. 渲染分隔线、焦点边框、标签栏、上下文菜单、防休眠指示器
8. `draw_to_surface()` 渲染终端内容
9. 可选 egui 覆盖层（设置面板/profile selector/欢迎页/模态框/autocomplete/history/correction）
10. `frame.present()`

### 事件路由优先级（鼠标）
1. 分隔线拖拽（CursorMoved 中增量更新）
2. 标签栏悬浮/点击（标签页 / "+" 按钮 / 关闭按钮）
3. 上下文菜单悬浮/点击
4. 分隔线命中检测（开始拖拽）
5. 面板鼠标事件（Left/Right/Middle/Scroll）

## 常见任务

### 添加新的窗口事件处理
在 `event_handler.rs` 的 `window_event` 方法中添加 match 分支。修改 `RedrawRequested` 逻辑时注意保持 drain_output -> pending_marks -> dirty check -> multi-pane snapshot -> overlay -> present 的顺序。

### 修改标签栏
- **布局**：编辑 `app.rs` 的 `update_tab_bar_layout()` -- 标签严格均分剩余宽度
- **渲染**：编辑 `overlay.rs` 的 `render_tab_bar_overlay()` -- 使用 TabBarState 缓存的物理像素值
- **交互**：编辑 `event_handler.rs` 中 `CursorMoved` / `MouseInput::Pressed(Left)` 分支

### 修改启动流程
编辑 `event_handler.rs` 的 `resumed` 方法。

### 添加新的 PaneAction/MenuAction
1. 在 `pureterm-ui` 中添加枚举变体
2. 在 `pane_ops.rs` 的 `handle_pane_action()` 或 `tab_ops.rs` 的 `handle_menu_action()` 中添加 match 分支
3. 如需多面板操作，确保操作后调用 `resize_all_panes()` + `window.request_redraw()`

### 添加新的覆盖层 UI
1. 在 `overlay.rs` 中添加渲染方法（使用 `build_rounded_rect_vertices` / `build_circle_vertices`）
2. 在 `event_handler.rs` 的 `RedrawRequested` 中调用
3. 如需 egui，在 egui 闭包中添加面板渲染

### 添加平台特定功能
在 `platform/` 目录下添加新平台文件，实现 `PlatformWindowExt` trait。使用 `#[cfg(target_os = "...")]` 条件编译。

## 父级导航

<- [pureterm/AGENTS.md](../AGENTS.md)
