# PureTerm 功能差异 Todo 列表

> 基于 Alacritty v0.26.1-dev 运行时，对比 4 个开源终端项目：Alacritty、Ghostty、iTerm2、Warp

## 开源参考仓库

| 仓库 | 路径 | 关键特性 |
|------|------|----------|
| Alacritty | `/Users/admin/Code/Github/alacritty-master/` | GPU 渲染、PTY 处理、配置系统 |
| Ghostty | `/Users/admin/Code/Github/ghostty-main/` | 8 通道渲染、独立渲染线程、Shell 集成、字体系统 |
| iTerm2 | `/Users/admin/Code/Github/iTerm2-master/` | 30+ 触发器、智能选择、语义历史、Toolbelt、密码管理、DVR |
| Warp | `/Users/admin/Code/Github/warp-master/` | AI 助手、Rust 终端、WarpUI |

---

## 类别 1：终端仿真

### 1.1 OSC 133 Shell 集成
- **优先级**: P0
- **状态**: ✅ 已实现（MVP）— OSC 133 解析 + BlockManager + Shell 脚本注入
- **描述**: 命令块检测、提示标记、输出边界、防止睡眠追踪
- **实现文件**:
  - `crates/alacritty_terminal/src/event.rs` — ShellMarkKind/ShellMark 事件类型
  - `crates/alacritty_terminal/src/event_loop.rs` — scan_osc133() 预扫描器 + 7 个单元测试
  - `crates/pureterm-alacritty-runtime/src/alacritty/shell_block.rs` — BlockManager/CommandBlock/导航方法 + 3 个单元测试
  - `crates/pureterm-alacritty-runtime/src/alacritty/shell_integration.rs` — Shell 检测/ShellEnv/Zsh+Bash+Fish 脚本 + 3 个单元测试
- **已实现**: OSC 133 A/B/C/D 标记解析、exit_code + duration 参数、BlockManager 命令块追踪、Shell 自动检测与脚本注入、PTY 环境变量注入
- **待完善**: 提示符导航快捷键、滚动补偿、Visual prompt markers、Nushell/Elvish 脚本

### 1.2 DECSCL 设备控制
- **优先级**: P2
- **状态**: 未实现
- **参考**: Ghostty `src/terminal/modes.zig`

### 1.3 标题栈 (CSI 22/23t)
- **优先级**: P2
- **状态**: 未实现
- **参考**: Ghostty `src/terminal/Terminal.zig` - 22t 处理

### 1.4 BiDi 双向文本
- **优先级**: P2
- **状态**: 未实现（已知差距）
- **参考**:
  - Ghostty: `src/unicode/` 目录
  - Archive: `archive/pureterm-terminal/src/screen/` 曾有 BiDi 支持

---

## 类别 2：渲染与图形

### 2.1 多通道渲染管线
- **优先级**: P0
- **状态**: ✅ 已实现 — wgpu 8 通道渲染管线 + 独立渲染线程 (feature flag: `wgpu-renderer`)
- **描述**: 替换 Alacritty OpenGL 单通道渲染器为 wgpu 8 通道管线，支持 Metal/Vulkan/DX12 跨平台
- **实现文件**:
  - `crates/pureterm-alacritty-runtime/src/alacritty/renderer/render_pass/` — 8 个渲染通道 (bg_color, bg_image, cell_text, decorations, cursor, kitty_image, debug_overlay, post_process)
  - `crates/pureterm-alacritty-runtime/src/alacritty/renderer/render_thread.rs` — 独立渲染线程 + crossbeam_channel 消息传递
  - `crates/pureterm-alacritty-runtime/src/alacritty/renderer/glyph_atlas.rs` — WgpuGlyphAtlas: crossfont → wgpu R8Unorm Skyline 装箱
  - `crates/pureterm-alacritty-runtime/src/alacritty/renderer/vertex_builders.rs` — FrameBuilder: 可复用 Vec 避免 per-frame 分配
  - `crates/pureterm-alacritty-runtime/src/alacritty/renderer/shaders/` — 8 个 WGSL 着色器
- **已实现**: 8 通道按序渲染、独立渲染线程 (mailbox 模式)、crossfont → wgpu 字形桥接、动态 GPU 缓冲增长、光标闪烁、FPS 统计
- **待完善**: 删除 OpenGL 旧代码、彩色 Emoji BGRA 图集、Kitty 图片渲染、ShaderToy 自定义着色器
- **参考**:
  - Ghostty: `src/renderer/generic.zig` (8 个渲染通道)
  - Ghostty: `src/renderer/Thread.zig` (独立渲染线程，mailbox 模式)

### 2.2 彩色 Emoji / BGRA 字体图集
- **优先级**: P1
- **状态**: ✅ 已实现 — 双图集架构（灰度 R8 + 彩色 RGBA），per-vertex atlas_selector，WGSL 条件采样
- **描述**: Emoji 通过独立的 Rgba8UnormSrgb 图集渲染全彩色，文本仍走灰度 R8 图集
- **实现文件**:
  - `crates/pureterm-alacritty-runtime/src/alacritty/renderer/glyph_atlas.rs` — AtlasKind 枚举、双图集（灰度+彩色）、Skyline 装箱、LRU 淘汰
  - `crates/pureterm-alacritty-runtime/src/alacritty/renderer/render_pass/cell_text.rs` — TextVertex.atlas_selector、双纹理 bind group layout
  - `crates/pureterm-alacritty-runtime/src/alacritty/renderer/shaders/cell_text.wgsl` — 双纹理声明、条件采样片段着色器
  - `crates/pureterm-alacritty-runtime/src/alacritty/renderer/vertex_builders.rs` — 彩色字形白色顶点颜色 + atlas_selector 设置
  - `crates/pureterm-alacritty-runtime/src/alacritty/renderer/render_thread.rs` — FrameData 彩色图集字段、双格式 upload_atlas
  - `crates/pureterm-alacritty-runtime/src/alacritty/display/mod.rs` — 彩色图集数据流提取
- **已实现**: 双图集路由（BitmapBuffer::Rgba → 彩色图集，Rgb → 灰度图集）、独立 Skyline 装箱、WGSL if-else 条件采样、1x1 透明占位纹理
- **参考**:
  - Ghostty: `font/Atlas.zig` - `format: .bgra` 彩色 emoji
  - Ghostty: `font/face/coretext.zig` / `font/face/freetype.zig` - 彩色字形光栅化
  - Ghostty: `src/renderer/cell.zig` - `CellText.atlas: enum { grayscale, color }`

### 2.3 HarfBuzz 文本整形
- **优先级**: P1
- **状态**: PureTerm 只有 14 个硬编码连字对（→、≠ 等）
- **参考**:
  - Ghostty: `font/shaper/harfbuzz.zig` (~400 行)
  - Ghostty: `font/shaper/Cache.zig` - 整形结果缓存
- **关键文件**:
  - Ghostty: `src/font/shaper/harfbuzz.zig`
  - Ghostty: `src/font/shaper/run.zig`

### 2.4 背景图像 GPU 渲染
- **优先级**: P1
- **状态**: 图像可加载但未渲染到 GPU
- **参考**:
  - Ghostty: `renderer/generic.zig` → bg_image 管线
  - Ghostty: `renderer/image.zig` → 背景层图像处理
- **关键文件**:
  - Ghostty: `src/renderer/generic.zig` - bg_image 管线
  - Ghostty: `src/config/Config.zig` - background-image-fit/position/repeat

### 2.5 自定义着色器 (ShaderToy)
- **优先级**: P1
- **状态**: 未实现
- **参考**:
  - Ghostty: `renderer/shadertoy.zig` (~400 行)
  - GLSL → SPIR-V → MSL/GLSL 编译
- **关键文件**:
  - Ghostty: `src/renderer/shadertoy.zig` - 自定义着色器链与 ping-pong 纹理

### 2.6 Sixel 图形渲染
- **优先级**: P2
- **状态**: Alacritty 有部分解析但未完整 GPU 渲染
- **参考**:
  - iTerm2: `sources/VT100/VT100SixelParser.m`
  - Ghostty: `src/terminal/kitty/graphics.zig` (Kitty 协议，非 Sixel)
- **关键文件**:
  - iTerm2: `sources/VT100/VT100SixelParser.m`

### 2.7 字体图集矩形装箱
- **优先级**: P1
- **状态**: ✅ 已实现 — WgpuGlyphAtlas 使用 Skyline 算法，2048x2048 固定 R8 图集，LRU 淘汰
- **实现文件**: `crates/pureterm-alacritty-runtime/src/alacritty/renderer/glyph_atlas.rs`
- **参考**: Ghostty `font/Atlas.zig` 使用 Jukka Jylanki Skyline 算法

### 2.8 字体回退链
- **优先级**: P1
- **状态**: 缺少码点回退发现
- **参考**:
  - Ghostty: `font/CodepointResolver.zig` (~300 行，5 步回退链)
  - Ghostty: `font/CodepointMap.zig` (~200 行)
- **关键文件**:
  - Ghostty: `src/font/CodepointResolver.zig`
  - Archive: `archive/pureterm-gui/src/font_fallback.rs` (5 步 CJK/emoji 链)

---

## 类别 3：窗口与标签页管理

### 3.1 多标签支持
- **优先级**: P0
- **状态**: ✅ 已实现（Phase 1 MVP）— macOS 原生标签 + CloseTab action + SSH 标签创建
- **描述**: macOS 原生 NSTabGroup 标签系统，每标签独立 Terminal/PTY，键盘快捷键完整支持
- **实现文件**:
  - `crates/pureterm-alacritty-runtime/src/alacritty/config/bindings.rs` — `CloseTab` action 枚举 + Cmd+W 绑定
  - `crates/pureterm-alacritty-runtime/src/alacritty/input/mod.rs` — `CloseTab` 处理逻辑
  - `crates/pureterm-alacritty-runtime/src/alacritty/event.rs` — `new_ssh_tab()` SSH 标签创建
  - `crates/pureterm-alacritty-runtime/src/alacritty/cli.rs` — `TerminalOptions.command` 公开化
- **已实现**: Cmd+T 创建标签、Cmd+W 关闭标签、Cmd+Shift+]/[ 切换标签、Cmd+1-9 选择标签、SSH 标签创建、标签标题 OSC 同步
- **限制**: 仅 macOS（依赖 NSTabGroup 原生标签），Linux/Windows 跨平台标签未实现，计划后续通过自定义 GPU 标签栏实现
- **待完善**: 自定义 GPU 标签栏（Phase 2）、标签拖拽排序、标签颜色/图标

### 3.2 分割窗格
- **优先级**: P1
- **状态**: Alacritty 运行时未实现
- **参考**:
  - iTerm2: 多窗格支持
  - Ghostty: `datastruct/split_tree.zig` 分割树
  - Archive: `archive/pureterm-ui/src/split/`
- **关键文件**:
  - Ghostty: `src/datastruct/split_tree.zig`
  - Archive: `archive/pureterm-ui/src/split/`

### 3.3 GPU 标签栏渲染
- **优先级**: P1
- **状态**: 未实现
- **参考**:
  - Ghostty: GPU 原生标签栏样式
  - Archive: `archive/pureterm-ui/src/tab_bar.rs` - Ghostty 风格 GPU 标签栏
- **关键文件**:
  - Archive: `archive/pureterm-ui/src/tab_bar.rs`

### 3.4 状态栏
- **优先级**: P2
- **状态**: 未实现（只有标签栏）
- **参考**:
  - iTerm2: `sources/StatusBar/` - git 分支轮询、cwd、hostname
  - Warp: `app/src/` 状态栏
  - Archive: `archive/pureterm-ui/src/status_bar.rs`
- **关键文件**:
  - iTerm2: `sources/StatusBar/iTermGitPoller.m` - git 分支/提交显示
  - iTerm2: `sources/StatusBar/Components/` - 状态栏组件

---

## 类别 4：会话管理

### 4.1 SSH 会话后端
- **优先级**: P0
- **状态**: ✅ 已实现（MVP）— ControlMaster socket 复用、AES-256-GCM 加密凭证、SSH Profile 配置、SshAction 键盘绑定
- **描述**: 支持 ~/.ssh/config Host 别名连接和 PureTerm SSH Profile 两种方式
- **实现文件**:
  - `crates/pureterm-alacritty-runtime/src/alacritty/ssh/mod.rs` — SSH 模块入口
  - `crates/pureterm-alacritty-runtime/src/alacritty/ssh/session.rs` — SshSessionManager 连接池
  - `crates/pureterm-alacritty-runtime/src/alacritty/ssh/pty.rs` — SshPty EventedPty 实现
  - `crates/pureterm-alacritty-runtime/src/alacritty/ssh/credential.rs` — 加密凭证存储
  - `crates/pureterm-alacritty-runtime/src/alacritty/ssh/config.rs` — SshProfile 配置
  - `crates/pureterm-alacritty-runtime/src/alacritty/ssh/command.rs` — SSH 命令构建器
  - `crates/pureterm-alacritty-runtime/src/alacritty/config/ssh.rs` — SshConfig TOML 解析
- **已实现**: ControlMaster socket 复用、~/.ssh/config 集成、AES-256-GCM + Argon2id 加密、SshAction (Connect/OpenSettings/ListProfiles)
- **待完善**: SSH Tab UI（实际标签页创建）、SSH 设置界面、命令面板集成
- **参考**:
  - Warp: `crates/warp_terminal/` SSH
  - Archive: `archive/pureterm-session/src/ssh/`

### 4.2 Serial 会话后端
- **优先级**: P1
- **状态**: 未实现
- **参考**: Archive: `archive/pureterm-session/src/serial_session.rs`
- **关键文件**:
  - Archive: `archive/pureterm-session/src/serial_session.rs`

### 4.3 Telnet 会话后端
- **优先级**: P2
- **状态**: 未实现
- **参考**: Archive: `archive/pureterm-session/src/telnet_session.rs`

### 4.4 Tmux 控制模式
- **优先级**: P2
- **状态**: 未实现
- **参考**:
  - Ghostty: `terminal/tmux/*.zig`
  - Archive: `archive/pureterm-terminal/src/tmux/`
- **关键文件**:
  - Ghostty: `src/terminal/tmux/tmux.zig`
  - Ghostty: `src/terminal/tmux/` (4 文件)

### 4.5 Workgroups（保存/恢复会话）
- **优先级**: P2
- **状态**: 只有基础标签恢复，无完整 workgroup
- **参考**:
  - iTerm2: `sources/Workgroups/` - 窗口布局序列化
  - iTerm2: `sources/Workgroups/iTermWorkgroupInstance.swift`
- **关键文件**:
  - iTerm2: `sources/Workgroups/iTermWorkgroupInstance.swift` - 保存状态
  - iTerm2: `sources/Workgroups/iTermWorkgroupModel.swift` - 模型

### 4.6 会话录制 (DVR)
- **优先级**: P3
- **状态**: 只有会话日志，无回放
- **参考**:
  - iTerm2: `sources/DVR/` - 即时回放
  - iTerm2: `sources/DVR/DVRBuffer.m`、`DVREncoder.m`、`DVRDecoder.m`
- **关键文件**:
  - iTerm2: `sources/DVR/DVR.h`、`sources/DVR/DVR.m`

---

## 类别 5：文本选择与搜索

### 5.1 智能选择（基于精度）
- **优先级**: P1
- **状态**: ✅ 已实现 — Precision 5 级正则规则 + 双击触发 + TOML 配置
- **描述**: 双击时先尝试 smart selection（URL/邮箱/路径/Git SHA 等），无匹配 fallback 到词边界
- **实现文件**:
  - `crates/alacritty_terminal/src/smart_selection.rs` — SmartSelectionService/Precision/SmartRule/SmartMatch + 9 条默认规则 + 10 个测试
  - `crates/alacritty_terminal/src/selection.rs` — range_semantic 集成 smart selection + range_smart/byte_offset_to_point
  - `crates/alacritty_terminal/src/term/search.rs` — smart_selection_line_text() 行文本提取
  - `crates/pureterm-alacritty-runtime/src/alacritty/config/smart_selection.rs` — TOML 配置
  - `crates/pureterm-alacritty-runtime/src/alacritty/window_context.rs` — 运行时配置注入
- **已实现**: 9 条默认规则（URL/SSH URL/邮箱/引号字符串/文件路径/C++ namespace/dotted include/Git SHA/IPv4）、5 级精度排序、TOML `[[smart_selection.rules]]` 自定义规则、双击触发 + 词边界 fallback
- **待完善**: 右键上下文菜单集成（actions 触发）、evaluate_all 用于悬停高亮、规则热重载

### 5.2 语义历史（悬停预览）
- **优先级**: P1
- **状态**: 只有 OSC 8 超链接，无悬停预览
- **参考**:
  - iTerm2: `sources/SemanticHistory/iTermSemanticHistoryController.m`
  - iTerm2: `sources/SemanticHistory/iTermPathFinder.m` - 文件路径检测
  - iTerm2: Quick Look 集成
- **关键文件**:
  - iTerm2: `sources/SemanticHistory/iTermSemanticHistoryController.m` (45KB)
  - iTerm2: `sources/SemanticHistory/iTermURLActionFactory.m`

### 5.3 Tail-Find（自动滚动搜索）
- **优先级**: P2
- **状态**: 基础搜索，无 tail-find
- **参考**:
  - iTerm2: `sources/SearchingFiltering/TailFindController.swift`
  - iTerm2: `sources/SearchingFiltering/iTermDropDownFindViewController.m`
- **关键文件**:
  - iTerm2: `sources/SearchingFiltering/TailFindController.swift`
  - iTerm2: `sources/SearchingFiltering/iTermFindDriver.m`

### 5.4 复制模式（Vim 风格）
- **优先级**: P2
- **状态**: 基础鼠标选择，无 vi 模式
- **参考**:
  - iTerm2: `sources/CopyMode/iTermCopyModeHandler.m`
  - Alacritty: `alacritty_terminal/src/vi_mode.rs` (部分 vi 模式存在)
- **关键文件**:
  - iTerm2: `sources/CopyMode/iTermCopyModeHandler.m` (24KB)
  - Alacritty: `alacritty_terminal/src/vi_mode.rs`

---

## 类别 6：自动化与脚本

### 6.1 触发器系统
- **优先级**: P0
- **状态**: ✅ 已实现（MVP）— 11 种动作已接入事件循环，3 个内置默认规则，TOML 配置支持
- **实现文件**:
  - `crates/pureterm-alacritty-runtime/src/alacritty/trigger.rs` — TriggerService / TriggerAction / LineTracker / 模板展开 / 12 个单元测试
  - `crates/pureterm-alacritty-runtime/src/alacritty/trigger_dispatch.rs` — 11 种动作分发（Highlight/Bell/Notify/SendText/CaptureToFile/Hyperlink 等）
  - `crates/pureterm-alacritty-runtime/src/alacritty/config/trigger.rs` — TOML 配置解析
- **已实现动作**: Bell、Highlight（Cell bg）、Notify（macOS 通知）、CopyMatch、RunCommand、SendText（PTY 写回）、CaptureToFile（文件轮转）、Hyperlink、DirectoryTrack、PasswordMask、PlaySound
- **待完善**: Hyperlink/PasswordMask 单元格操作、TOML 热重载、Settings UI 编辑界面

### 6.2 协同处理（外部脚本集成）
- **优先级**: P1
- **状态**: 只有 MiddlewareStack（login script / zmodem）
- **参考**:
  - iTerm2: `sources/Tasks/Coprocess.m` - 外部脚本运行与终端 I/O
- **关键文件**:
  - iTerm2: `sources/Tasks/Coprocess.m`
  - Archive: `archive/pureterm-session/src/coprocess.rs`

### 6.3 代码片段系统
- **优先级**: P2
- **状态**: 未实现
- **参考**:
  - iTerm2: `sources/Snippets/` - 文本片段系统
  - iTerm2: `sources/Toolbelt/iTermToolSnippets.m` - toolbelt 片段视图
- **关键文件**:
  - iTerm2: `sources/Snippets/iTermSnippetsModel.m`
  - iTerm2: `sources/Toolbelt/iTermToolSnippets.m`

### 6.4 输入广播（向所有窗格输入）
- **优先级**: P3
- **状态**: 只在 tmux_session 同步输入
- **参考**:
  - iTerm2: `sources/InputBroadcasting/iTermBroadcastInputHelper.m`
- **关键文件**:
  - iTerm2: `sources/InputBroadcasting/iTermBroadcastInputHelper.m`

---

## 类别 7：安全特性

### 7.1 密码管理器集成
- **优先级**: P2
- **状态**: SecretRedaction 只掩码显示，无密码获取
- **参考**:
  - iTerm2: `sources/PasswordManager/` - 1Password/LastPass/Keychain
  - iTerm2: `sources/PasswordManager/iTermPasswordManagerWindowController.m`
- **关键文件**:
  - iTerm2: `sources/PasswordManager/iTermPasswordManagerWindowController.m` (70KB)
  - iTerm2: `sources/PasswordManager/KeychainPasswordDataSource.swift`
  - iTerm2: `sources/PasswordManager/OnePasswordDataSource.swift`
  - iTerm2: `sources/PasswordManager/LastPassDataSource.swift`

### 7.2 密码触发器（掩码）
- **优先级**: P1
- **状态**: 未实现
- **参考**:
  - iTerm2: `sources/Triggers/PasswordTrigger.m`
- **关键文件**:
  - iTerm2: `sources/Triggers/PasswordTrigger.h`、`PasswordTrigger.m`

### 7.3 注释/标记
- **优先级**: P3
- **状态**: 未实现
- **参考**:
  - iTerm2: `sources/Annotations/`
  - iTerm2: `sources/Triggers/MarkTrigger.m`
- **关键文件**:
  - iTerm2: `sources/Annotations/PTYAnnotation.m`

---

## 类别 8：生产力特性

### 8.1 Toolbelt（侧边面板）
- **优先级**: P2
- **状态**: 未实现（只有快速栏、命令面板）
- **参考**:
  - iTerm2: `sources/Toolbelt/` - 命令历史、粘贴历史、片段、配置、目录浏览器、笔记
  - Archive: `archive/pureterm-ui/src/toolbelt.rs`
- **关键文件**:
  - iTerm2: `sources/Toolbelt/iTermToolbeltView.m` (35KB)
  - iTerm2: `sources/Toolbelt/ToolCommandHistoryView.m`
  - iTerm2: `sources/Toolbelt/ToolPasteHistory.m`
  - iTerm2: `sources/Toolbelt/ToolDirectoriesView.m`

### 8.2 命令面板
- **优先级**: P1
- **状态**: 基础实现存在
- **参考**: Archive: `archive/pureterm-ui/src/command_palette.rs`

### 8.3 自动 Profile 切换
- **优先级**: P2
- **状态**: 基础目录跟踪，无自动 profile 切换
- **参考**:
  - iTerm2: `sources/AutomaticProfileSwitching/`
- **关键文件**:
  - iTerm2: `sources/AutomaticProfileSwitching/`

### 8.4 SSH 文件浏览器 (SFTP GUI)
- **优先级**: P2
- **状态**: 基础 SFTP 传输，无图形浏览器
- **参考**:
  - iTerm2: `SSHFilePanel.swift` (84KB) - 图形 SFTP 浏览器
  - Archive: `archive/pureterm-ui/src/sftp_browser.rs`
  - Archive: `archive/pureterm-session/src/ssh/sftp.rs`
- **关键文件**:
  - iTerm2: `SSHFilePanel.swift`

---

## 类别 9：平台集成

### 9.1 Touch Bar 支持
- **优先级**: N/A（macOS 专用，与跨平台目标不符）
- **参考**: iTerm2: `sources/TouchBar/`

### 9.2 无障碍/VoiceOver
- **优先级**: P1
- **状态**: Alacritty 运行时未完整实现
- **参考**:
  - Ghostty: `Surface.zig` 完全 VoiceOver 支持
  - iTerm2: `sources/Accessibility/`
  - Archive: `archive/pureterm/src/accessibility.rs`
- **关键文件**:
  - Ghostty: `src/Surface.zig` - 文本快照无障碍
  - Ghostty: `src/Accessibility.zig`

### 9.3 检查器/调试面板
- **优先级**: P2
- **状态**: 基础框架存在但不完整
- **参考**:
  - Ghostty: `inspector/widgets/` - 页面内存、键盘历史、渲染器信息、屏幕网格
  - Archive: `archive/pureterm-ui/src/inspector.rs`
- **关键文件**:
  - Ghostty: `src/inspector/Inspector.zig`
  - Ghostty: `src/inspector/widgets/screen.zig`
  - Ghostty: `src/inspector/widgets/termio.zig`

---

## 优先级汇总

### P0 - 关键（必须实现）
1. ~~OSC 133 Shell 集成~~ ✅ 已实现（MVP）
2. ~~多标签支持~~ ✅ 已实现（Phase 1 MVP）
3. ~~SSH 会话后端~~ ✅ 已实现（MVP）
4. ~~触发器系统~~ ✅ 已实现（MVP）
5. ~~多通道渲染管线~~ ✅ 已实现（wgpu 8 通道 + 独立渲染线程）

### P1 - 高优先级（应该有）
6. ~~智能选择~~ ✅ 已实现（9 条规则 + TOML 配置）
7. ~~彩色 Emoji / BGRA 字体图集~~ ✅ 已实现（双图集 + 条件采样）
8. 语义历史 / 悬停预览
9. HarfBuzz 文本整形
10. 背景图像 GPU 渲染
11. 自定义着色器 (ShaderToy)
12. 复制模式（Vi 风格）
13. 协同处理
14. 字体回退链
15. 字体图集矩形装箱

### P2 - 中优先级（最好有）
16. Sixel 图形
17. 分割窗格
18. GPU 标签栏
19. 状态栏
20. Tail-Find 搜索
21. 代码片段系统
22. 密码管理器集成
23. 密码触发器
24. Serial 会话后端
25. 自动 Profile 切换
26. SFTP 浏览器 GUI
27. Workgroups
28. Tmux 控制模式
29. BiDi 支持
30. 检查器面板

### P3 - 低优先级（未来）
31. 输入广播
32. 注释/标记
33. 会话录制 (DVR)
34. Telnet 会话后端

---

## 关键实现参考文件

1. `/Users/admin/Code/Github/iTerm2-master/sources/Triggers/Trigger.m` - 基础触发器系统架构
2. `/Users/admin/Code/Github/ghostty-main/src/renderer/Thread.zig` - 独立渲染线程与 mailbox
3. `/Users/admin/Code/Github/ghostty-main/src/termio/shell_integration.zig` - OSC 133 shell 集成
4. `/Users/admin/Code/CNB/PureTerm/archive/pureterm-core/src/trigger.rs` - Legacy 触发器实现参考
5. `/Users/admin/Code/CNB/PureTerm/archive/pureterm-ui/src/terminal/pane.rs` - 带智能选择的终端窗格
6. `/Users/admin/Code/Github/ghostty-main/src/font/shaper/harfbuzz.zig` - HarfBuzz 文本整形
7. `/Users/admin/Code/Github/iTerm2-master/sources/SemanticHistory/iTermSemanticHistoryController.m` - 语义历史 (45KB)
8. `/Users/admin/Code/Github/iTerm2-master/sources/Toolbelt/iTermToolbeltView.m` - Toolbelt (35KB)
9. `/Users/admin/Code/Github/iTerm2-master/sources/PasswordManager/iTermPasswordManagerWindowController.m` - 密码管理 (70KB)