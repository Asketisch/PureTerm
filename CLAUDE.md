# PureTerm - 纯 Rust 终端模拟器

纯 Rust 编写的 GPU 加速终端模拟器。

## 快速开始

```bash
cargo build --release
./target/release/pureterm
```

## 架构

6 个 crate 的工作区：`pureterm` (二进制入口) + `pureterm-core` + `pureterm-terminal` + `pureterm-session` + `pureterm-ui` + `pureterm-gui`

## 终端兼容性（已实现）

- **VTE 解析**：完整 CSI/ESC/OSC/DCS/APC/PM 序列处理
- **DEC 私有模式**：App Cursor、Auto Wrap、Origin、Column 132、Inverse、Mouse Reporting (1000/1002/1003/1006)、Focus In/Out、Alternate Screen (1047/1048/1049)、Bracketed Paste
- **ANSI 模式**：IRM (Insert Mode)、LNM
- **CSI 序列**：CUU/CUD/CUF/CUB、CUP、ED/EL、IL/DL/DCH、SU/SD、ECH、ICH、SGR、SM/RM、DECSTBM、CHA、VPA、DECSCUSR、CHT、CBT、CNL、CPL、TBC、DA1/DA2、DSR、DECSTR、XTWINOPS、REP
- **OSC 序列**：Title (0/2)、Current Dir (7)、Hyperlinks (8)、Clipboard (52)、Colors (4/10/11/12)、Shell Integration (133)
- **ESC 序列**：DECSC/DECRC、RIS、IND、RI、NEL、Screen Alignment
- **渲染特性**：Bold→Bright 映射 (palette 0-7)、Inverse 模式、Hidden 文本、Background image、Kitty 内联图片、多 pass 渲染（背景色→装饰→文字→光标→后处理）、窗口 resize 安全（atlas dirty 标记 + Grid reflow 行数修复）
- **标签栏**：Ghostty 风格原生 GPU 渲染，28pt 高度与 macOS 红绿灯按钮一致，DPI 感知（所有常量 × scale_factor），运行时查询红绿灯实际宽度，主题色驱动配色，关闭按钮在左侧（hover 显示），"+" 新建标签按钮（36px），严格等分布局，tab_bar_dirty 刷新机制
- **UI 形状渲染**：SDF（Signed Distance Field）GPU 圆角矩形和圆形，单管线 `shape_params` 字段分支渲染，`fwidth + smoothstep` 像素级抗锯齿

## Agent 指南（分级索引）

文档采用层级导航：本文件 → crates/ → 各 crate → src/ → 子目录，每层 AGENTS.md 指向更详细的下级。

- **项目导航**：[AGENTS.md](AGENTS.md) — 完整层级树
- **工作区总览**：[crates/AGENTS.md](crates/AGENTS.md) — 依赖关系、数据流、Agent 类型选择
- **各 Crate 指南**（每层指向 src/AGENTS.md，src/ 再指向子目录）：
  - [pureterm/AGENTS.md](crates/pureterm/AGENTS.md) → [src/AGENTS.md](crates/pureterm/src/AGENTS.md) — 二进制入口，事件循环
  - [pureterm-core/AGENTS.md](crates/pureterm-core/AGENTS.md) → [src/AGENTS.md](crates/pureterm-core/src/AGENTS.md) — 配置、主题、快捷键、AI、秘密脱敏
  - [pureterm-terminal/AGENTS.md](crates/pureterm-terminal/AGENTS.md) → [src/AGENTS.md](crates/pureterm-terminal/src/AGENTS.md) — VTE 解析、屏幕网格、CommandBlock
  - [pureterm-session/AGENTS.md](crates/pureterm-session/AGENTS.md) → [src/AGENTS.md](crates/pureterm-session/src/AGENTS.md) — Session trait + 5 种实现（含 Tmux）
  - [pureterm-ui/AGENTS.md](crates/pureterm-ui/AGENTS.md) → [src/AGENTS.md](crates/pureterm-ui/src/AGENTS.md) — Surface、标签页、命令面板、历史菜单
  - [pureterm-gui/AGENTS.md](crates/pureterm-gui/AGENTS.md) → [src/AGENTS.md](crates/pureterm-gui/src/AGENTS.md) — GPU 渲染、Sprite、HarfBuzz、ShaderToy

## 快捷键

| 快捷键 | 功能 |
|--------|------|
| `Ctrl+Shift+P` | 命令面板 |
| `Ctrl+Shift+O` | 配置文件选择器 |
| `Ctrl+,` | 设置面板 |
| `Ctrl+Shift+I` | 终端检查器 |
| `Ctrl+Shift+K` | 快速栏 |
| `Ctrl+F` | 搜索 |
| `Ctrl+Shift+T` | 新建标签页 |
| `Ctrl+Shift+W` | 关闭标签页 |
| `Ctrl+Shift+D` | 向右分屏 |
| `Ctrl+D` | 向下分屏 |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | 放大 / 缩小 / 重置缩放 |

## CLI 命令

```bash
pureterm --profile <name>       # 通过配置文件连接
pureterm --ssh <user@host>      # 快速 SSH 连接
pureterm --quake                # 下拉终端模式
pureterm list-fonts             # 列出可用字体
pureterm list-themes            # 列出可用主题
pureterm show-config            # 显示当前配置
pureterm version                # 显示版本信息
pureterm settings               # 打开设置面板
```

## 功能文档索引

- **Shell 集成**：[pureterm-session/src/shell_integration.rs](crates/pureterm-session/src/shell_integration.rs) → [src/AGENTS.md](crates/pureterm-session/src/AGENTS.md) — OSC 133 标记注入
- **终端检查器**：[pureterm-ui/src/inspector.rs](crates/pureterm-ui/src/inspector.rs) → [src/AGENTS.md](crates/pureterm-ui/src/AGENTS.md) — InspectorState 调试面板
- **C API**：[pureterm-core/src/capi.rs](crates/pureterm-core/src/capi.rs) → [src/AGENTS.md](crates/pureterm-core/src/AGENTS.md) — libpureterm FFI 接口
- **Tmux 集成**：[pureterm-terminal/src/tmux/](crates/pureterm-terminal/src/tmux/) → [tmux/AGENTS.md](crates/pureterm-terminal/src/tmux/AGENTS.md) — DCS 1000p 控制模式
- **Tmux Session**：[pureterm-session/src/tmux_session.rs](crates/pureterm-session/src/tmux_session.rs) → [src/AGENTS.md](crates/pureterm-session/src/AGENTS.md) — Tmux 控制模式 Session
- **多 pass 渲染**：[pureterm-gui/src/render_pass/](crates/pureterm-gui/src/render_pass/) → [render_pass/AGENTS.md](crates/pureterm-gui/src/render_pass/AGENTS.md) — 8 pass GPU 渲染架构
- **Surface 抽象**：[pureterm-ui/src/surface.rs](crates/pureterm-ui/src/surface.rs) → [src/AGENTS.md](crates/pureterm-ui/src/AGENTS.md) — Ghostty 风格的 Surface + Action 消息系统
- **字体回退**：[pureterm-gui/src/font_fallback.rs](crates/pureterm-gui/src/font_fallback.rs) → [src/AGENTS.md](crates/pureterm-gui/src/AGENTS.md) — 5 步字体回退链（fontdb）
- **Sprite 光栅化**：[pureterm-gui/src/sprite.rs](crates/pureterm-gui/src/sprite.rs) → [src/AGENTS.md](crates/pureterm-gui/src/AGENTS.md) — Block/Box/Braille/Powerline 软件 rasterizer
- **Grapheme 边界**：[pureterm-terminal/src/screen/grapheme.rs](crates/pureterm-terminal/src/screen/grapheme.rs) → [screen/AGENTS.md](crates/pureterm-terminal/src/screen/AGENTS.md) — UAX #29 光标移动和选词
- **秘密脱敏**：[pureterm-core/src/secret_redaction.rs](crates/pureterm-core/src/secret_redaction.rs) → [src/AGENTS.md](crates/pureterm-core/src/AGENTS.md) — API 密钥/令牌模式检测和视觉脱敏
- **命令纠正**：[pureterm-core/src/command_correction.rs](crates/pureterm-core/src/command_correction.rs) → [src/AGENTS.md](crates/pureterm-core/src/AGENTS.md) — AI 驱动的命令失败建议
- **命令历史**：[pureterm-ui/src/history_menu.rs](crates/pureterm-ui/src/history_menu.rs) → [src/AGENTS.md](crates/pureterm-ui/src/AGENTS.md) — 命令历史菜单（MAX_HISTORY=10000）
- **命令面板**：[pureterm-ui/src/command_palette.rs](crates/pureterm-ui/src/command_palette.rs) — `Ctrl+Shift+P` 触发，支持搜索和执行内置命令
- **快速栏**：[pureterm-ui/src/quick_bar.rs](crates/pureterm-ui/src/quick_bar.rs) — `Ctrl+Shift+K` 触发，支持 Command/Search/Filter 模式
- **CommandBlock**：[pureterm-terminal/src/block.rs](crates/pureterm-terminal/src/block.rs) → [src/AGENTS.md](crates/pureterm-terminal/src/AGENTS.md) — OSC 133 Shell 集成命令块模型
- **触发器系统**：[pureterm-core/src/trigger.rs](crates/pureterm-core/src/trigger.rs) → [src/AGENTS.md](crates/pureterm-core/src/AGENTS.md) — 11 种 action 的 regex trigger（Notify/Highlight/Bell/CopyMatch/RunCommand/SendText/CaptureToFile/Hyperlink/DirectoryTrack/PasswordMask/PlaySound），pane.drain_output 评估每行匹配；Settings → Triggers 页可视化编辑

## UI 设计语言

所有 UI 元素（标签栏、上下文菜单、按钮）必须遵循统一的圆角设计语言。

### 圆角规范

所有圆角半径以**逻辑点（pt）**定义，运行时乘以 `scale_factor` 转换为物理像素。

| 元素 | 形状 | 圆角半径（pt） |
|------|------|---------------|
| 标签页背景 | 圆角矩形 | 6 |
| 标签页颜色指示条 | 纯色矩形 | 0（底部细条） |
| 关闭按钮背景 | 圆形 | 直径 24pt |
| 新建标签按钮悬停 | 圆角矩形 | 4 |
| 标签栏整体背景 | 纯色矩形 | 0（全宽） |
| 底部分隔线 | 纯色矩形 | 0 |
| 右键菜单背景 | 圆角矩形 | 8 |
| 菜单项悬停高亮 | 圆角矩形 | 4 |
| 菜单分隔符 | 纯色矩形 | 0 |

### 渲染方式

- **GPU SDF 渲染**：通过 `Vertex.shape_params` 字段区分形状类型
  - `shape_type = 0`：字形纹理采样 / 纯色矩形（原有行为）
  - `shape_type = 1`：圆角矩形 SDF（`sd_rounded_box`）
  - `shape_type = 2`：圆形 SDF（`length(p) - 1.0`）
- **抗锯齿**：使用 `fwidth(d) + smoothstep(-aa, aa, d)` 实现像素级平滑边缘
- **DPI 感知**：所有圆角半径以逻辑点定义，运行时乘 `scale_factor`

### 配色规范

| 元素 | 颜色 | 说明 |
|------|------|------|
| 标签栏背景 | 终端背景色 × 0.85 | 略暗于终端背景 |
| 活跃标签 | 终端背景色（原色） | 与终端内容融为一体 |
| 悬停标签 | 背景色 × 0.92 + 0.05 | 微亮提示 |
| 关闭按钮默认 | `[0.25, 0.25, 0.28]` | 深灰色圆形 |
| 关闭按钮悬停 | `[0.4, 0.15, 0.15]` | 红色高亮 |
| 新建按钮悬停 | `[0.3, 0.3, 0.32]` | 浅灰 |
| 菜单背景 | `[0.18, 0.18, 0.22, 0.95]` | 半透明深色 |
| 菜单项悬停 | `[0.25, 0.5, 0.85]` | 蓝色高亮 |

### 新增 UI 元素要求

添加新的 UI 元素时必须：
1. 使用 `build_rounded_rect_vertices` 或 `build_circle_vertices` 替代 `build_rect_vertices`
2. 圆角半径遵循上表规范（或选择最接近的值）
3. 配色遵循上表规范，保持视觉一致性
4. 所有尺寸以逻辑点定义，乘 `scale_factor`

### 外观配置项

| 配置项 | 类型 | 默认值 | 说明 |
|--------|------|--------|------|
| `pane_dim_opacity` | f32 | 0.0 | 非聚焦面板暗化透明度 (0.0-0.8) |
| `focus_pane_on_hover` | bool | false | 鼠标悬停时自动聚焦面板 |
| `input_mode` | enum | PinnedToBottom | 输入模式：PinnedToBottom/PinnedToTop/Waterfall |
