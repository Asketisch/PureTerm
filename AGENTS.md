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
- **渲染特性**：Bold→Bright 映射 (palette 0-7)、Inverse 模式、Hidden 文本、Background image

## Agent 指南（分级索引）

- **工作区总览**：[crates/AGENTS.md](crates/AGENTS.md) — 依赖关系、数据流、Agent 类型选择
- **各 Crate 详细指南**：
  - [pureterm/AGENTS.md](crates/pureterm/AGENTS.md) — 二进制入口，事件循环
  - [pureterm-core/AGENTS.md](crates/pureterm-core/AGENTS.md) — 配置、主题、快捷键、插件
  - [pureterm-terminal/AGENTS.md](crates/pureterm-terminal/AGENTS.md) — VTE 解析、屏幕网格
  - [pureterm-session/AGENTS.md](crates/pureterm-session/AGENTS.md) — Session trait + 4 种实现
  - [pureterm-ui/AGENTS.md](crates/pureterm-ui/AGENTS.md) — egui 组件层
  - [pureterm-gui/AGENTS.md](crates/pureterm-gui/AGENTS.md) — wgpu GPU 渲染
