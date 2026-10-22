# GAP-74 手动验收清单

> 自动化覆盖：标题栏样式解析与平台门控（单测）、窗口创建接入
> `platform_specific` / `decorations`、E2E 显示层像素断言。下列是需要
> **人眼**确认的部分,以及**必须跨平台人工跑**的部分。

## 准备

```bash
cargo build --release --bin pureterm --bin pureterm-settings
```

配置 `~/.config/pureterm/alacritty.toml`：

```toml
[appearance]
preset = "cyberpunk"     # 或 classic

[window]
titlebar = "native"      # native | overlay | unified
```

> 改完 `titlebar` **要重启**才生效(窗口装饰在窗口创建时决定)。

## 1. unified:自绘标题栏

- [ ] 窗口顶部是一条**与应用同色**的标题栏(不是系统灰条);
- [ ] 左侧显示窗口标题(应用名 + 标签页名);
- [ ] 右侧三个按钮 `─` `□` `✕` **水平/垂直居中**(不贴左缘、不贴顶边);
- [ ] 点 `─` → 窗口最小化;
- [ ] 点 `□` → 窗口最大化 / 还原;
- [ ] 点 `✕` → 窗口关闭;
- [ ] 按住标题栏中间空白处拖动 → 窗口跟随移动,**位移与鼠标 1:1**(不越拖越慢);
- [ ] 拖动区不影响标签栏的点击(点标签仍能切页);
- [ ] **已知代价**:窗口边缘没有系统的鼠标拖拽边框(不能用鼠标拖边改大小);
- [ ] **Windows 专项**:确认 Aero Snap / Win11 贴靠布局**已失效**(隐藏系统装饰的
      必然结果)—— Windows 用户应优先用 `native`。

## 2. unified:工具型窗口仍保留系统边框

- [ ] ⌘, 打开进程内设置窗口 → **有系统标题栏和关闭按钮**(不被剥掉);
- [ ] 独立 `pureterm-settings` 进程 → 同上;
- [ ] 取色器弹窗 → 同上;
- [ ] 主终端窗口**没有**系统红绿灯(unified 下系统装饰被隐藏)。

## 3. overlay(macOS 默认)

- [ ] 红绿灯落在**主题色顶栏**上,不是独立的系统灰条;
- [ ] 红绿灯没压住第一个标签页(左侧留了 78px);
- [ ] 拖顶栏空白处能移动窗口(系统原生行为);
- [ ] 点标签 / `+` / `⚙` 正常响应。

## 4. native(Windows / Linux 默认)

- [ ] macOS:恢复系统原生标题栏(红绿灯回到独立灰条上);
- [ ] Windows:标题栏底色 = 应用顶栏底色(与界面连成一条),文字色 = 主文本色;
- [ ] Windows:边框色 = 应用发丝线色;Win11 22H2+ 有 Mica 毛玻璃;
- [ ] Windows:原生 resize / snap / 关闭按钮 / 阴影全部保留;
- [ ] Windows:切到 cyberpunk 后标题栏颜色跟着变(节流 2 秒,最多延迟 2 秒);
- [ ] Linux:标题栏仍由 WM 绘制(这是 WM 的能力边界,应用改不了);
- [ ] macOS 上 `unified` 不是首选(没有红绿灯、按钮按 Windows 约定在右侧)。

## 5. 回退与容错

- [ ] 非法值(如 `titlebar = "foo"`)→ 回落 native,不崩溃;
- [ ] 删掉 `titlebar` 键 → 用平台默认(macOS=overlay,其它=native);
- [ ] 设置窗口里的「标题栏」下拉显示的当前值 = 实际运行的模式;
- [ ] 下拉切到 unified → 保存 → 重启 → 生效。

## 6. Windows / Linux 专项(**必须在对应系统上人工跑**)

> **已自动验证的部分**:两个目标的**可编译性**都过了
> (`cargo check --target x86_64-pc-windows-msvc -p pureterm-settings` /
> `--target x86_64-unknown-linux-gnu -p pureterm-settings`,均 0 error)。
> 下面这些是**编译查不出来、必须真机看**的部分。

- [ ] Windows 10 22H2 / 11:标题栏染色生效,颜色与 cyberpunk 预设一致;
- [ ] Windows:切换外观预设(classic ↔ cyberpunk)后,标题栏颜色跟着变
      (节流 2 秒,最多延迟 2 秒);
- [ ] Windows:多开几个窗口,每个窗口的标题栏都被染色;
- [ ] Linux(X11 + GNOME / KDE):unified 模式下自绘标题栏正常,三个按钮可用;
- [ ] Linux:确认已知代价 —— 窗口边缘**没有**系统的鼠标拖拽边框。

## 回滚

把 `[window].titlebar` 改回 `"native"`(或删掉该键)并重启 —— 无需重装。
