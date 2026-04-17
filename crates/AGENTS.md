# Agent 使用指南 — 工作区总览

本文件是工作区级别的导航索引。各 crate 的详细 Agent 指南请查看对应的 AGENTS.md。

## Crate 导航

| Crate | 说明 | 详细指南 |
|-------|------|----------|
| [pureterm](pureterm/AGENTS.md) | 二进制入口点，事件循环、窗口管理、多窗格渲染、标签栏 GPU 渲染、CLI | [pureterm/AGENTS.md](pureterm/AGENTS.md) |
| [pureterm-core](pureterm-core/AGENTS.md) | 配置、主题（28 种内置）、快捷键、i18n、AI、加密、触发器、C API | [pureterm-core/AGENTS.md](pureterm-core/AGENTS.md) |
| [pureterm-terminal](pureterm-terminal/AGENTS.md) | VTE 解析器、屏幕网格（reflow + 分页存储）、grapheme 边界、256 色调色板、Tmux 集成 | [pureterm-terminal/AGENTS.md](pureterm-terminal/AGENTS.md) |
| [pureterm-session](pureterm-session/AGENTS.md) | Session trait + Local/SSH/Serial/Telnet + Shell 集成 + 文件传输 | [pureterm-session/AGENTS.md](pureterm-session/AGENTS.md) |
| [pureterm-ui](pureterm-ui/AGENTS.md) | Surface 抽象、SplitTree、标签页（Ghostty 风格 GPU 渲染）、分割面板、终端面板、命令面板、搜索、AI 聊天 | [pureterm-ui/AGENTS.md](pureterm-ui/AGENTS.md) |
| [pureterm-gui](pureterm-gui/AGENTS.md) | wgpu 多 pass 渲染、Sprite 光栅化、HarfBuzz 整形、ShaderToy、字体回退 | [pureterm-gui/AGENTS.md](pureterm-gui/AGENTS.md) |

## 依赖关系

```
pureterm (二进制)
  → pureterm-core, pureterm-terminal, pureterm-session, pureterm-ui, pureterm-gui

pureterm-ui → pureterm-core, pureterm-terminal, pureterm-session
pureterm-gui → pureterm-core, pureterm-terminal
pureterm-session → pureterm-core
pureterm-terminal（独立）
pureterm-core（独立）
```

## Agent 类型选择

### Explore Agent
用于理解代码架构和跨 crate 关系：
- 探索 Session trait 在 Local/SSH/Serial/Telnet 中的实现
- 理解 VTE 解析器与 Terminal 的集成
- 调查 wgpu 多 pass 渲染管线
- 理解 Surface 抽象与 TerminalPane 的关系
- 在提出变更前寻找现有模式

### Plan Agent
用于涉及多个 crate 或需要架构决策的实现任务：
- 添加新的会话类型
- 修改终端模拟引擎
- 渲染管线的变更（新增 pass、修改着色器）
- UI 层重构（Surface/Action 消息系统）

### General-Purpose Agent
用于范围明确的有针对性修复：
- 修复 API 兼容性问题（如 russh 版本更新）
- 修复单个 crate 中的特定 bug
- 为现有功能添加测试
- 单个模块内的重构

## 核心数据流

```
用户按键 → Surface::key_callback / TerminalPane::handle_key
         → encode_key (App Cursor/Alt/Ctrl/Shift) → Session::write
Session 输出 → broadcast channel → 后台线程 → output_buffer (16 MiB, 60fps 限速)
             → drain_output → Terminal::write
Terminal::write → VteParser → VtePerform → Grid/Cell (wrapped flag for reflow)
渲染：Grid snapshot → 各 RenderPass 生成顶点（BgColor→Deco→Text→Cursor→PostProcess）→ wgpu GPU
标签栏：GPU 原生渲染（不依赖 egui），44px 高度，主题色配色，等分标签 + 悬浮关闭 + "+" 按钮
多窗格：SplitTree compute_layout → 每窗格 ViewportRect（减标签栏高度）→ 合并顶点 → 标签栏覆盖 → draw_to_surface → 可选 egui 覆盖 → frame.present()
Resize：surface 重配 → atlas 纹理重建 → mark_dirty 强制重上传 → Grid reflow（while 补充空行）
输入时自动滚回底部；Bell 事件通过 take_bell() 通知 UI
```

## 版本兼容性注意

- `russh 0.60` 和 `russh-keys` 有严格的版本耦合 — 使用 workspace.dependencies 中的版本
- `vte 0.15` 是当前稳定版
- `wgpu 29` — workspace 和 egui-wgpu 使用统一版本（旧版 wgpu 会导致运行时 ID 冲突 panic）
- `portable-pty 0.8` — 用 `take_writer()` 而不是 `try_clone_writer()`
- `egui 0.34` — `context.run()` 已废弃，应使用 `run_ui()`；`tessellate()` 需传入 `pixels_per_point`
- `fontdb 0.21` — 用于 font_fallback.rs 的字体发现和回退链
- `rustybuzz 0.20` — HarfBuzz 文本整形（BiDi、Arabic、Indic 等）
- `glam 0.29` — GPU 矩阵数学（正交投影）
- `naga 24` — 着色器编译（GLSL → WGSL）
