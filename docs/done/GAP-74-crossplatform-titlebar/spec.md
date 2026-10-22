---
gap: GAP-74
title: 跨平台标题栏适配（Windows 主题化 / Linux 自绘标题栏）
category: UI/外壳
plan_type: feature
priority: P1
effort: M
status: done
closed: 2026-10-22
---

# 跨平台标题栏适配（Windows 主题化 / Linux 自绘标题栏）

## 背景 / 动机

GAP-73 把 macOS 的红绿灯融进了界面顶栏,深色主题下不再割裂。但那套机制
依赖 macOS 独有的「透明标题栏 + 全屏内容视图 + 隐藏标题文字」三件套,
**Windows / Linux 上没有等价物**:

- **Windows**:标题栏由 DWM 绘制,应用改不了它的底色 —— 默认是系统深灰,
  和 cyberpunk 的深靛(`#05070F`)并排同样割裂;
- **Linux**:标题栏归 WM 所有(GTK/Qt/KDE 各不相同),应用连它的**颜色**
  都碰不到,更别说形状。

所以「其他平台的适配」不是把 macOS 那套抄过去,而是给每个平台选一条
**能做到的路**,并且把代价讲清楚。

## 目标

1. `[window].titlebar` 从两态扩成三态,Windows / Linux 上有路可走;
2. Windows:在**保留原生标题栏**的前提下,把它的颜色染成应用主题色;
3. 跨平台:提供「应用自绘标题栏」模式,让 Linux 也能得到同款观感;
4. 自绘标题栏的三个窗口按钮与拖动**真的可用**(不是摆设);
5. 设置窗口 / 取色器等工具型窗口始终保留系统边框(有系统关闭按钮);
6. 现有 macOS 行为**零回归**。

## 平台能力矩阵

| 模式 | macOS | Windows | Linux |
|------|-------|---------|-------|
| `native`(默认,非 macOS) | 系统原生标题栏 | 系统原生标题栏 **+ DWM 染成主题色** | 系统 / WM 标题栏(不可染色) |
| `overlay`(默认,macOS) | 红绿灯浮在界面顶栏上 | → 退化为 `native` | → 退化为 `native` |
| `unified` | 可用(隐藏系统标题栏,应用自绘) | 可用 | **唯一能得到同款观感的途径** |

## 变更点

### A. 标题栏样式扩成三态

`crates/pureterm-settings/src/theme/titlebar.rs`:`TitleBarStyle` 由
`{Native, Overlay}` 扩成 `{Native, Overlay, Unified}`。

- `parse()` 精确匹配三个值,未知值(含大小写不一致)回落 `Native`;
- `effective()` 做**平台门控**:非 macOS 上 `overlay` 退化为 `native`,
  `unified` 全平台保留;
- `draws_own_titlebar()` → `effective() == Unified`,决定要不要关掉系统装饰;
- `platform_specific()` 从自由函数改成**方法**(要拿 `self` 才能算平台门控);
- `platform_default()`:macOS = `Overlay`,其它 = `Native`。

> 三态带来的一个坑:配置里写 `overlay` 在 Linux 上会**静默降级**而不是报错。
> 这是刻意的 —— 同一份配置要在多平台间复制,不能因为平台不同就拒绝启动。

### B. Windows:把系统标题栏染成主题色

新增 `crates/pureterm-settings/src/theme/caption.rs`。标题栏仍由 DWM 绘制
(保住原生 resize / snap / 关闭按钮 / 阴影),但底色、字色、边框色改成应用
主题色 —— 于是系统标题栏与界面顶栏**连成一条**,割裂感同样消失。

裸 FFI,只引 4 个 Win32 入口(不引入 `windows` crate,免得为一个特性拉进
一个大依赖、也免去 Cargo.lock 变动):

| 属性 | ID | 作用 |
|------|----|------|
| `DWMWA_CAPTION_COLOR` | 35 | 标题栏底色 → 应用顶栏底色(`elevated`) |
| `DWMWA_TEXT_COLOR` | 36 | 标题文字色 → 应用主文本色 |
| `DWMWA_BORDER_COLOR` | 34 | 窗口边框色 → 应用发丝线色 |
| `DWMWA_USE_IMMERSIVE_DARK_MODE` | 20 | 深色标题栏 |
| `DWMWA_SYSTEMBACKDROP_TYPE` | 38 | Win11 22H2+ 的 Mica 毛玻璃 |

- 颜色转换:`Color`(0..1 浮点)→ `COLORREF`(0x00BBGGRR);
- 遍历:`EnumWindows` + `GetWindowThreadProcessId` 过滤出本进程的窗口
  (回调拿不到闭包状态,进程 PID 用 `AtomicU32` 传递);
- 未知 / 不支持的属性值系统会直接忽略并返回 `E_INVALIDARG`,所以可以
  **无条件全量下发**,不必按 Windows 版本分支;
- 非 Windows 平台是**空操作**(返回 `false`),标题栏归 WM 所有,应用
  改不了。

调用时机:主应用在 `TerminalWakeup` 里**节流 2 秒**调一次
(`apply_caption_theme`),覆盖后创建的窗口;设置进程启动时也调一次。
节流是因为 `EnumWindows` 要遍历所有顶层窗口,不能每帧做。

### C. 应用自绘标题栏(`unified`)

新增 `crates/pureterm-settings/src/theme/titlebar_view.rs` —— 一个泛型消息
的组件(`TitleBarMessages<M>`,主应用与设置进程各用自己的消息枚举):

- 左侧:窗口标题(应用名 + 标签页名);
- 中间:空白**拖动区**;
- 右侧:`─` `□` `✕` 三个 38px 宽的窗口按钮(最小化 / 最大化 / 关闭)。

放在 settings crate 的原因与调色板一样:**设置窗口是独立进程**,两边要用
同一份组件实现。

#### C.1 拖动:自己算位移,不用 `iced::window::drag`

GAP-73 在 macOS 上踩过:`iced::window::drag(id)` 在消息回调里调用**不生效**
(系统透明标题栏下顶部 28px 本来就是系统自己在拖)。但 `unified` 模式下
系统装饰被隐藏了,没有系统拖动可用,只能自己算:

1. 按下拖动区 → 记下窗口位置 + 光标位置;
2. 每次全局光标移动 → 用**屏幕坐标**算增量 → `iced::window::move_to`。

**必须用屏幕坐标**:窗口一动,窗口**内容**坐标系就跟着漂 —— 实测按内容
坐标算只走了 42% 的位移(窗口移动 → 增量基准变化 → 越拖越偏)。改成
`screen_now = window_pos + content_pos` 后,位移与鼠标 1:1 跟随。

### D. 接线

- 6 处 `window::Settings` 带上 `decorations: !style.draws_own_titlebar()`
  与 `platform_specific: style.platform_specific()`;
- 新增消息:`TitleBarDragStart` / `TitleBarDragReady` / `TitleBarMinimize` /
  `TitleBarMaximize` / `TitleBarClose`;
- `view()` 在 `unified` 时于标签栏**上方**插入自绘标题栏;
- 拖动由 `GlobalCursorMoved` 驱动、`GlobalMouseRelease` 结束。

> `iced::window::position(id)` 返回的是 `Task<Option<Point>>`(不是 future),
> 要用 `.map(...)` 接;`Point + Point` 不存在,屏幕坐标要手工
> `Point::new(x1 + x2, y1 + y2)`。

### E. 设置窗口保留系统边框

**工具型窗口**(进程内设置窗口 ⌘, / 独立 `pureterm-settings` 进程 /
取色器弹窗)**一律 `decorations: true`**:

- 它们没有画自绘标题栏 —— 若跟着 `unified` 关掉系统装饰,窗口就既没有
  系统标题栏、也没有关闭按钮(实测确认过这个问题,已修);
- 工具窗需要系统的关闭按钮与窗口菜单;
- Windows 上它们的标题栏**同样会被染成主题色**(DWM 染色是按进程遍历的,
  不过滤窗口类型)。

`unified` 只作用于**主终端窗口**(主窗口 / 新窗口 / 分屏弹出的窗口)。

设置窗口在 `overlay` 模式下仍然走 GAP-73 的「78px 红绿灯占位 + 主题色顶条」。

### F. 设置界面

Appearance 页新增「标题栏」下拉(native / overlay / unified)+ 一行说明文字
(中英双语 i18n),写 `[window].titlebar`,提示**重启生效**(窗口装饰在创建时
决定,改配置不能热切)。

- 缺省值取**平台默认**(macOS=overlay,其它=native),与主应用
  `AppConfig::load_titlebar` 的缺省保持一致 —— 否则设置页显示的模式会和
  实际运行的模式对不上。

## 实测结论(macOS)

成图:[`docs/preview/titlebar-unified.png`](../../preview/titlebar-unified.png)
(左侧标题 + 右侧 `─ □ ✕`,与下方标签栏同色融合)。

- `unified` 模式:自绘标题栏渲染正常,`─` `□` `✕` **三个按钮都真的可用**
  (─ → `AXMinimized=true`;□ → 窗口尺寸 1024×768 → 全屏 1676×1084;
  ✕ → 窗口关闭、进程退出);
- 拖动:位移与鼠标 1:1 跟随;
- 按钮字形**居中**(修掉了 iced 0.14 的一个坑,见下);
- `overlay` 模式回归通过:红绿灯(红 `236,104,101` / 绿 `102,196,102`)
  仍坐在主题色顶栏 `[22,28,50]` 上(逻辑 y 9–22,条高 0–28);
- `native` 模式零回归:系统原生标题栏(`[31,31,31]` 系统深灰)独立存在,
  红绿灯落在其中(逻辑 y 9–22),应用内容从逻辑 y≥33 才开始
  (顶栏 `[22,28,50]` → 标签栏 `[28,51,73]` → 终端 `[6,8,15]`)
  —— 与改动前完全一致;
- `unified` 模式下主窗口**没有**红绿灯、设置窗口**有**红绿灯(像素探针确认)。

### iced 0.14 的 Button 布局坑

`iced::widget::Button` 的内容会被摆到按钮盒子的**左上角**
(`layout::padded` → `content.move_to((padding.left, padding.top))`),
既不水平居中也不垂直居中。后果:38px 宽的窗口按钮里,字形贴着左缘,
按「右缘」去点会落空。

修法:把内容换成满尺寸的 `container` 并显式 `align_x/align_y(Center)`;
文字**不写死颜色** —— iced 的 `Button::draw` 会把样式的 `text_color` 通过
`renderer::Style` 传给内容,`Text` 在没有显式颜色时正好回退到它,hover
变亮才生效。

## 已知缺口 / 代价

1. **`unified` 失去系统的鼠标拖拽边框**:隐藏系统装饰后,窗口没有系统那圈
   几像素的鼠标拖拽区,只能用键盘或程序化方式改变窗口大小。`native`
   模式没有这个问题 —— 这是 Linux 用户自己的取舍。
2. **Windows 上更不建议用 `unified`**:隐藏系统装饰同样会失去
   **Aero Snap / Win11 贴靠布局**(拖到屏幕边缘吸附、鼠标悬停顶端的
   贴靠布局菜单),因为那些都依赖系统标题栏。Windows 的默认档是
   `native`(DWM 染色)—— 观感照样统一,功能一点不丢。**`unified` 在
   Windows 上是给「宁可不吸附也要那条自绘标题栏」的人准备的。**
3. **macOS 上 `unified` 不是首选**:它没有红绿灯,窗口按钮按 Windows 约定
   排在右侧(`─ □ ✕`),观感不如 `overlay`。macOS 的默认档是 `overlay`。
4. **Windows / Linux 代码路径没有在真实系统上跑过**:
   - **可编译性已用交叉目标验证**:Windows 与 Linux **两个目标都通过**
     (`cargo check --target x86_64-pc-windows-msvc -p pureterm-settings`
     / `--target x86_64-unknown-linux-gnu -p pureterm-settings`,均 0 error,
     新增模块一条告警都没有)。这覆盖了 `caption.rs` 的 DWM FFI 与
     `titlebar.rs` 的**非 macOS 分支**(`effective()` / `platform_default()` /
     `platform_specific()`);本轮的 Windows 代码就是在这条检查下从
     **4 个编译错误**(Rust 2024 的 `unsafe extern` 块、`unsafe_op_in_unsafe_fn`、
     `WNDENUMPROC` 应为可空函数指针)修到通过的;
   - **主 crate 的 Windows 交叉编译在本机跑不通**:第三方 C 依赖
     (`ring` / `aws-lc-sys`)的构建脚本需要 Windows C 工具链,与本轮改动无关;
   - **实际观感**(DWM 染色效果、Linux 下 WM 的配合)仍需在真实系统上人工确认
     (见 test-plan §6);
   - `caption.rs` 的 DWM 签名与属性 ID 依据官方文档人工复核
     (DWMWA_USE_IMMERSIVE_DARK_MODE=20、BORDER_COLOR=34、CAPTION_COLOR=35、
     TEXT_COLOR=36、SYSTEMBACKDROP_TYPE=38;COLORREF=0x00BBGGRR)。
5. **Linux 标题栏无法染色**:WM 说了算,应用碰不到。`unified` 是唯一出路。
6. 双击标题栏缩放未实现(沿用 GAP-73 的已知缺口)。
7. **CI 的 `clippy -D warnings` 门禁在 HEAD 上就是红的**:vendored 的
   `alacritty_terminal` 有 19 个 doc 格式告警(`empty line after doc
   comment` / `doc list item without indentation` / `missing_safety_doc`),
   而项目规则禁止为同步上游而无关格式化 vendored 源码。本轮只保证
   **不引入新告警**(新增模块的 dead_code 用 `#[allow(dead_code)]` 标注,
   原因写在 `theme/mod.rs` 的注释里),并顺手修掉了
   `pureterm-alacritty-runtime/build.rs` 里 `PathBuf` 导入在非 Windows
   平台未使用的问题(它同样会让 CI 的 clippy job 直接失败)。

## 验证 / Test Plan

**Rust 单测**(`titlebar.rs`,10 项):`parse` 精确匹配 / 非法值回落 /
往返一致 / `effective()` 平台门控 / `draws_own_titlebar` / `platform_specific`
与样式一致。

**E2E**:见 [`test-plan.md`](test-plan.md)。

## 优先级 / 范围

- **优先级**:P1(Windows / Linux 用户同样受深色主题割裂困扰)
- **范围**:标题栏三态 + Windows DWM 染色 + 跨平台自绘标题栏 + 设置界面
- **不做**:Windows 上自绘标题栏(DWM 染色风险更低、保留原生 resize/snap);
  自绘红绿灯;双击缩放;Linux 下的 WM 协议适配
