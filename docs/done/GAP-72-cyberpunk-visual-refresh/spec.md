---
gap: GAP-72
title: 赛博朋克视觉改版（外壳 + 终端配色）
category: UI/外壳
plan_type: feature
priority: P1
effort: M
status: done
closed: 2026-10-22
---

# 赛博朋克视觉改版（外壳 + 终端配色）

## 背景 / 动机

PureTerm 的界面此前是「能用但没有体系」的状态:

- 主应用 `theme()` 硬编码返回 `Theme::Dark`,**外壳与设置窗口各用各的调色板**
  (设置窗口是 Catppuccin 系 `0x2b292d` / `0xd3869b`,主应用是 iced 内置 Dark);
- **128 处硬编码色值**散落在 `iced_app.rs`(75 处)与 `sidebar/` / `status_bar/`;
- 终端 ANSI 16 色是 Alacritty 经典灰阶,`[colors.normal]` / `[colors.bright]`
  写了也不生效(内核 `Term::new(TermConfig::default())` 从不读用户色板);
- 想换个配色只能改代码。

目标:加一层**语义化调色板 + 外观预设机制**,并提供一套「霓虹赛博朋克」预设,
在**不新增 GPU 通道**的前提下让整窗(外壳 + 终端)观感统一。

## 竞品参考

| 竞品 | 做法 | 对 PureTerm 的启发 |
|------|------|---------------------|
| **Windows Terminal** | 内置多套主题(配色 scheme JSON),运行时可切 | 预设 = 一整套语义色,不是零散色值 |
| **iTerm2** | 预设 + 独立的 GUI 控件色板 | GUI 与终端配色可分治,但要能成套切换 |
| **Ghostty / Kitty** | 主题文件 + 实时预览 | 改主题应立刻可见 |

## 现状证据(改造前)

- `iced_app.rs:15230` `theme()` → 硬编码 `Theme::Dark`;
- `crates/pureterm/src/iced_app.rs` 内联 `Color::from_rgba` **75 处**;
- `crates/pureterm/src/status_bar/chip.rs` 依赖 `Theme::Palette` 派生色,
  注释写着「将来应用加入浅色/System 主题时,chip 会自动跟随」—— 前提一直没成立;
- `crates/pureterm/src/terminal_shader/helpers.rs::named_color_default()` 是
  ANSI 16 色的硬编码回退板;
- `crates/pureterm/src/app_config/loaders_misc.rs` 无 `[appearance]` 段;
- `tests/e2e/test_display.py::test_background_color_matches_config` 断言
  背景色等于配置里的 `[colors.primary].background`。

## 目标

1. 新增 `[appearance].preset = "classic" | "cyberpunk"`,**classic 保持默认**,
   老用户零回归;cyberpunk 需显式开启。
2. 调色板下沉为**语义色表**(单一真源),外壳 / 状态栏 / 终端 / 设置窗口共用。
3. 终端 ANSI 16 色可由预设接管,注入 `Colors` 的 0..15 槽位,
   具名色与 `38;5;N` 索引色同时生效。
4. 硬编码色值归零(字面量只留在调色板模块内)。
5. 视觉稿以自包含 HTML 交付,与实现色值同源。

## 变更点

### A. 调色板与预设(唯一真源)

新增 `crates/pureterm-settings/src/theme/palette/`:

| 文件 | 内容 |
|------|------|
| `palette.rs` | 类型:`ThemePreset` / `ShellTokens` / `StatusBarTokens` / `StatusSemantics` / `TerminalTokens` / `UiPalette` + `PICKER_SWATCHES` |
| `palette/classic.rs` | classic 色值 —— **逐值等于改造前的字面量** |
| `palette/cyberpunk.rs` | cyberpunk 色值(深靛底 + 青/品红/紫) |
| `palette/tests.rs` | 单测:预设解析回落、classic 等价性、cyberpunk 与 HTML 视觉稿一致 |

放在 `pureterm-settings` 的原因:设置窗口是**独立进程**,主应用与它必须共用
同一份色值;主 crate 已依赖 settings crate,反向依赖会成环。

`ShellTokens` 共 40+ 个语义字段(底/面、强调、状态、文本、高亮、浮层、agent),
全部 `Copy`,无锁、无共享可变状态。

### B. 配置

```toml
[appearance]
preset = "classic"          # classic | cyberpunk
terminal_palette = "follow" # follow(跟随 preset) | user(保留自己的 [colors.*])
```

- 加载器:`AppConfig::load_appearance`(`app_config/loaders_ui.rs`),
  缺表 / 畸形 → `Default`(classic),**坏配置永不导致启动失败**;
- 默认配置模板 `crates/pureterm/default-config.toml` 与示例
  `extra/pureterm.toml` 同步补段。

### C. 终端配色注入(零 GPU 成本)

`crates/pureterm/src/terminal_shader/overrides.rs`(新增):

- `ANSI_SLOTS`:`NamedColor::Black..BrightWhite` 的 16 个槽位(判别值 0..15,
  与 256 色调色板前 16 项**同槽**);
- `inject_ansi_palette()`:往 `Colors` 的 0..15 写色 → 具名色与 `38;5;N` 同时生效;
- `TermColorOverrides`:光标 / 选区 / 搜索命中 / 折叠遮罩 / vi 光标。

`program.rs` 在既有的「注入配置前景色」之后追加注入;叠加层常量改为读
`term_overrides`(缺省值与改造前逐值一致)。

接线规则(`ui_theme::terminal_wiring`):

| preset | `terminal_palette` | 终端前景/背景 | ANSI 16 | 叠加层 |
|--------|--------------------|---------------|---------|--------|
| classic | 任意 | 用户 `[colors.primary]` | 不注入 | classic 值(= 改造前) |
| cyberpunk | follow(默认) | 预设 | 预设 | 预设 |
| cyberpunk | user | 用户 `[colors.primary]` | 不注入 | 预设 |

### D. 外壳接线

- `PureTermApp` 新增 `palette` / `preset` / `term_wiring` 三字段;
- `theme()` → `palette.app.to_iced_theme()`(不再硬编码 `Theme::Dark`);
- 状态栏 chip 改由 `StatusBarTokens` 供色(`chip_bg/chip_text/chip_border/bar_bg/bar_border`
  签名从 `&Theme` 改为 `&StatusBarTokens`);
- 左右侧栏视图新增 `tokens: ShellTokens` 参数;
- 浮层统一化:backdrop 变暗 + 面板 `overlay_bg` + 1px `overlay_border` 描边 + 外发光;
- 发光(`Shadow`)**只用于**激活 tab / 焦点输入框 / 浮层面板三处,chip 与列表项一律无阴影;
- **激活标签霓虹条**:2px 强调色 + 外发光,用 `iced::widget::Stack` **叠加**在
  标签行底部(不占高度 —— 标签栏垂直空间只有 20px,用 `column` 会被压扁);
- `neon_active_tab` 开关:霓虹条与「激活标签标题用强调色」**只在 cyberpunk
  开启**,classic 关闭 → 默认路径逐像素一致(经典观感完全不变)。

### E. 设置窗口

- `pages/appearance.rs` 新增「外观预设」下拉(radio + `PickMapping`),
  写回 `[appearance].preset`,切换时**立即套用**(`theme::set_tokens`)以便当场预览;
- 主题样式函数改读进程级 `theme::tokens()`(样式回调拿不到我们的语义色表)。

### F. 视觉稿

`docs/preview/cyberpunk-ui.html` —— 自包含单文件(无外部依赖 / 无网络请求),
含顶栏 / 双侧栏 / 终端 / 状态栏 / 命令面板浮层 6 个区块 + 色板卡 + 实现说明;
右上角可在 CYBERPUNK / CLASSIC 之间切换(radio + CSS `:checked`,另附 JS 兜底)。

## 验证 / Test Plan

**Rust 单测**(`cargo nextest run -p pureterm-settings -p pureterm`):

- `palette/tests.rs`:预设解析(未知 / 空 / 大小写不一致 → classic);
  classic 与改造前字面量逐值一致(防回归);cyberpunk 与 HTML 视觉稿一致;
  cyberpunk 16 色互不相同且 bright 比 normal 亮;
- `terminal_shader/overrides.rs`:ANSI 槽位 0..15 对齐、注入后具名色与索引色同槽、
  RGBA→`Rgb` 夹紧;classic 叠加层常量与改造前一致;
- `ui_theme`:classic 保留用户终端色 / cyberpunk 覆盖 / `terminal_palette=user` 逃生阀;
- `status_bar/chip.rs`:四组 chip 色互不相同、半透明。

**E2E**(`tests/e2e/test_display.py`):

- `test_background_color_matches_config` → 按 preset 期望背景色(classic / cyberpunk);
- `test_cyberpunk_palette_renders`(新增)→ 真实启动 + 往终端打印**具名色**
  文本(`\e[36m` / `\e[35m`)+ 抓屏,断言画面出现 cyberpunk 的霓虹青/霓虹品红。
  之所以锚定**终端字形**而不是外壳像素:字形内部是纯色,是「ANSI 16 色确实
  被注入」的**直接**证据(外壳的强调色多为半透明混合,像素判据不稳)。
  容差说明:透明窗口经 macOS 合成 + 屏幕色彩描述文件转换后,`#00E5FF` 实测落点
  约 (105,226,251)、`#FF2E97` 约 (235,70,150) → 青色用「高 G/B、低 R」判据,
  品红用逐通道容差;classic 的 `#06989A` / `#75507B` 都不满足,不会误判。

**性能**:不新增 wgpu pipeline / render target / 纹理;终端每帧仅多 16 次
`Option<Rgb>` 写入;preset 解析只在启动发生一次。

手动验收清单见 [test-plan.md](test-plan.md)。

## 优先级 / 范围

- **优先级**:P1(界面观感与产品辨识度)
- **范围**:外壳 + 终端 ANSI 配色 + 预设机制
- **不做**:扫描线 / CRT / 文本辉光等 shader 特效(需额外 pass,另立特性);
  运行中热切 preset(避免半途换肤的视觉撕裂);社区主题机制适配。

## 风险

| 风险 | 缓解 |
|------|------|
| cyberpunk 覆盖用户 `[colors.*]` 导致意外丢配置 | 启动日志明确提示;`terminal_palette = "user"` 逃生阀;改回 `classic` 立即恢复 |
| 霓虹色在浅色终端背景下不可读 | cyberpunk 强制深色背景;classic 保留 |
| Iced `Shadow` 发光绘制开销 | 发光仅限 3 类元素;chip / 列表项无阴影 |
| 两进程调色板漂移 | 单一真源 + 同源单测;HTML 视觉稿 hex 也在单测里比对 |
| `iced_app.rs` 继续膨胀(已 15.6k 行) | 本特性不向其加新逻辑,新代码进 `ui_theme/` / `terminal_shader/overrides.rs` |
