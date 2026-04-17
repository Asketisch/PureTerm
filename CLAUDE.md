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
- **标签栏**：Ghostty 风格原生 GPU 渲染，44px 高度适配 macOS 标题栏区域，主题色驱动配色，悬浮显示关闭按钮，"+" 新建标签按钮（28px），等分布局（排除红绿灯和加号按钮区域）

## Agent 指南（分级索引）

- **工作区总览**：[crates/AGENTS.md](crates/AGENTS.md) — 依赖关系、数据流、Agent 类型选择
- **各 Crate 详细指南**：
  - [pureterm/AGENTS.md](crates/pureterm/AGENTS.md) — 二进制入口，事件循环
  - [pureterm-core/AGENTS.md](crates/pureterm-core/AGENTS.md) — 配置、主题、快捷键、插件
  - [pureterm-terminal/AGENTS.md](crates/pureterm-terminal/AGENTS.md) — VTE 解析、屏幕网格、grapheme、调色板
  - [pureterm-session/AGENTS.md](crates/pureterm-session/AGENTS.md) — Session trait + 4 种实现
  - [pureterm-ui/AGENTS.md](crates/pureterm-ui/AGENTS.md) — Surface、SplitTree、标签页、终端面板
  - [pureterm-gui/AGENTS.md](crates/pureterm-gui/AGENTS.md) — 多 pass GPU 渲染、Sprite、HarfBuzz、ShaderToy

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

- **Shell 集成**：[pureterm-session/AGENTS.md](crates/pureterm-session/AGENTS.md) — OSC 133 标记注入
- **终端检查器**：[pureterm-ui/AGENTS.md](crates/pureterm-ui/AGENTS.md) — InspectorState 调试面板
- **C API**：[pureterm-core/AGENTS.md](crates/pureterm-core/AGENTS.md) — libpureterm FFI 接口
- **Tmux 集成**：[pureterm-terminal/AGENTS.md](crates/pureterm-terminal/AGENTS.md) — DCS 1000p 控制模式
- **多 pass 渲染**：[pureterm-gui/src/render_pass/AGENTS.md](crates/pureterm-gui/src/render_pass/AGENTS.md) — 8 pass GPU 渲染架构
- **Surface 抽象**：[pureterm-ui/AGENTS.md](crates/pureterm-ui/AGENTS.md) — Ghostty 风格的 Surface + Action 消息系统
- **字体回退**：[pureterm-gui/AGENTS.md](crates/pureterm-gui/AGENTS.md) — 5 步字体回退链（fontdb）
- **Sprite 光栅化**：[pureterm-gui/AGENTS.md](crates/pureterm-gui/AGENTS.md) — Block/Box/Braille/Powerline 软件 rasterizer
- **Grapheme 边界**：[pureterm-terminal/src/screen/AGENTS.md](crates/pureterm-terminal/src/screen/AGENTS.md) — UAX #29 光标移动和选词
