---
gap: GAP-73
title: macOS 标题栏融合（红绿灯融入界面顶栏）
category: UI/外壳
plan_type: feature
priority: P1
effort: S
status: done
closed: 2026-10-22
---

# macOS 标题栏融合（红绿灯融入界面顶栏）

## 背景 / 动机

GAP-72 上线赛博朋克预设后,暴露一个此前不显眼的问题:窗口顶部那条 **macOS
原生标题栏**(系统深灰 `#1D1D1D`)和 PureTerm 自己的顶栏/终端底色是两套颜色,
在深色主题下割裂感很强 —— 尤其 cyberpunk 的深靛底(`#05070F`)与系统灰条
并排,像两个不同的应用。

macOS 生态里(Alacritty / iTerm2 / Ghostty / WezTerm / VS Code)的通行做法是
**让窗口内容铺满标题栏区域**,系统红绿灯浮在应用自己的顶栏上,两者共用
同一套背景色 —— 也就是「红绿灯与主题融为一体」。

## 目标

1. 窗口内容铺到标题栏下,红绿灯直接落在**应用自己的顶栏**上(底色 = 主题色)。
2. 保留 macOS 原生红绿灯的行为(关闭/最小化/全屏),不自己画按钮。
3. 窗口仍可拖动、顶栏仍可点击(见「实测结论」)。
4. 可切回原生标题栏(`[window].titlebar = "native"`)。
5. 非 macOS 平台行为不变。

## 变更点

### A. 标题栏样式模块

新增 `crates/pureterm-settings/src/theme/titlebar.rs`(放在 settings crate 而
不是主 crate:设置窗口是**独立进程**,两边要用同一份判定逻辑;主 crate 已
依赖 settings crate,反向会成环):

- `TitleBarStyle { Native, Overlay }` + `parse` / `as_str` /
  `platform_default` / `effective` / `needs_traffic_light_inset`;
- `TRAFFIC_LIGHT_INSET = 78.0`(3 个 12px 按钮 + 间距 + 边距);
- `TITLE_BAR_HEIGHT = 28.0`(与顶栏同高,红绿灯垂直居中);
- `platform_specific(style)` → `iced::window::settings::PlatformSpecific`:
  macOS 下 overlay 时置 `title_hidden` / `titlebar_transparent` /
  `fullsize_content_view` 三件套;其它平台返回 `default()`(无红绿灯)。

`parse` 对未知值回落 `Native`(坏配置不改变既有行为);缺省值为**平台默认**
—— macOS = overlay,其它 = native。

### B. 配置

```toml
[window]
titlebar = "overlay"   # overlay(默认,macOS)| native
```

主应用走 `AppConfig::load_titlebar`(缺省 → 平台默认);独立设置进程直接读
同一个键(缺省同样 → 平台默认)。

### C. 主应用接线

- 6 处 `window::Settings` 全部带上 `platform_specific`;
- `view_top_bar(id)`:
  - overlay 时顶栏行首插入 78px 空白(红绿灯位置),顶栏底色是主题色
    → 红绿灯与主题同色;
- **没有标签栏**的窗口(detached / secondary / 冷启动占位符 / 进程内设置
  窗口)用 `view_titlebar_strip()` 补一条 28px 的**主题色**顶条,内容不再
  被红绿灯压住。

### C.1 拖动窗口:用系统的,不要自己接管

最初实现里自加了 `Message::StartWindowDrag` → `iced::window::drag(id)` 并在
顶栏挂了 `on_press`。**实测后删掉了** —— 原因:

- 透明标题栏 + 全屏内容视图下,**系统自己**就会在顶部 28px 区域拖动窗口
  (实测位移与鼠标位移完全一致:拖 (130,90) → 窗口位移 (130,7),y 被屏幕
  下边界截断);
- 该区域的**点击又能正常送达应用**(实测:⌘T 建出 Tab 2 后点第 1 个标签 →
  标题变回 Tab 1),说明标题栏没有吞掉内容的事件;
- 自挂的 `on_press` 反而抢走事件:拖动手柄处**拖不动**(自定义
  `iced::window::drag` 在这种时序下不生效),留下一个「按了没反应」的死区。

结论:顶栏区域保持「点=点控件、拖=拖窗口」的系统原生分工,不自己接管。

### D. 设置窗口

- 独立设置进程(`pureterm-settings`):窗口带上 `platform_specific`,视图顶部
  补同样的主题色顶条 + `Message::StartWindowDrag`;
- 进程内设置窗口(⌘,):由主应用包顶条(见上)。

## 验证 / Test Plan

**Rust 单测**(`titlebar.rs`):

- `parse` 精确匹配 + 未知值/大小写不一致 → `Native`;
- `as_str` 往返;
- `effective()` 按平台门控(非 macOS 上 overlay 退化为 native);
- `platform_specific()` 与样式一致(macOS)。

**E2E**(`tests/e2e/test_display.py`):

- `wait_for_window` / `activate_pureterm` 支持按 **unix id** 定位进程 ——
  同一台机器上可能同时有多个 `pureterm` 进程(不同沙箱各起一个),按名字
  取会拿到别的实例的窗口;
- `test_background_color_matches_config`:置前后取屏,均值明显偏暗(被遮挡)
  时重试;
- `test_cyberpunk_palette_renders`:每轮**重发**打印命令直到屏幕上真的出现
  霓虹色(shell 可能还没就绪,只发一次会丢)。

**人工验收**:

- [ ] 红绿灯落在顶栏上,底色与顶栏一致(不再有系统灰条);
- [ ] 拖动顶栏空白处能移动窗口;点标签/按钮不触发拖动;
- [ ] 窗口圆角保留(透明窗口 + 全屏内容视图下 macOS 仍会裁圆角);
- [ ] 无标签栏的窗口(⌘, 打开的设置窗口、detached 窗口)内容不被红绿灯压住;
- [ ] `[window].titlebar = "native"` 可切回系统原生标题栏。

## 优先级 / 范围

- **优先级**:P1(消除深色主题下的标题栏割裂)
- **范围**:macOS 标题栏样式 + 顶栏占位 + 拖动接管
- **不做**:自绘红绿灯按钮(保留系统原生行为);双击标题栏缩放(未实现,绿按钮仍可全屏);
  Windows / Linux 的标题栏定制
