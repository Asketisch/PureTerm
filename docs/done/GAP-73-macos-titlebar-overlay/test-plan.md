# GAP-73 手动验收清单

> 自动化覆盖：标题栏样式解析（单测）、窗口创建接入 `platform_specific`、
> E2E 显示层像素断言。下列是需要**人眼**确认的部分。

## 准备

```bash
cargo build --release --bin pureterm --bin pureterm-settings
```

配置 `~/.config/pureterm/alacritty.toml`：

```toml
[appearance]
preset = "cyberpunk"     # 或 classic

[window]
titlebar = "overlay"     # overlay(默认) | native
```

## 1. 红绿灯与主题融为一体

- [ ] 窗口顶部**没有**系统原生深灰标题栏；
- [ ] macOS 红绿灯（红/黄/绿）直接落在**应用自己的顶栏**上，底色与顶栏一致；
- [ ] 红绿灯没有压住第一个标签页（顶栏左侧留了 78px）；
- [ ] 顶栏顶边的青→紫→品红渐变细线延伸到窗口最左/最右；
- [ ] 窗口圆角保留（没有变成直角）。

## 2. 交互

- [ ] 拖动顶栏 → 窗口跟随移动（**系统原生**行为：透明标题栏下顶部 28px
      区域由系统负责拖动；实现里刻意不自己接管，见 spec §C.1）；
- [ ] 点标签 / `+` / `⚙` → 正常响应（实测点标签能切换标签页）；
- [ ] 拖动与点击不冲突：点=点控件，拖=拖窗口；
- [ ] 双击顶栏 → 无反应（未实现双击缩放，绿按钮仍可全屏；此项为已知缺口）。

## 3. 各类窗口

- [ ] 进程内设置窗口（⌘,）→ 顶部是**主题色**顶条，红绿灯落在上面，内容不被压住；
- [ ] 独立 `pureterm-settings` 进程 → 同上；
- [ ] detached / secondary 窗口（无标签栏）→ 顶部有 28px 主题色顶条，终端内容从顶条下方开始。

## 4. 可回退

- [ ] `[window].titlebar = "native"` → 恢复系统原生标题栏（红绿灯回到独立灰条上）；
- [ ] 非法值（如 `titlebar = "foo"`）→ 回落 native，不崩溃；
- [ ] 非 macOS 平台 → 行为不变（无红绿灯，`overlay` 自动退化为 native）。

## 5. 性能

- [ ] 启动耗时与改动前持平（`scripts/bench_startup.py --rounds 5`）；
- [ ] 拖动窗口无卡顿（拖动由 winit 原生处理，不是我们逐帧算位置）。

## 回滚

把 `[window].titlebar` 改回 `"native"`（或删掉该键）并重启 —— 无需重装。
