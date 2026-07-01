# GAP-09 Notification System（OSC 9/99/777 + Bell）—— 手动 GUI E2E runbook

> ✅ **已 flip（2026-06-28）** — GAP-09 已翻 🚧→✅（全 6 阶段代码闭环 + 610 pureterm 单测绿 + runbook + 四处同步）。下方场景作为**非阻塞 follow-up**，跑完勾选即可；**非 CI gate。**
>
> **Automated harness:** scanner 18（`alacritty_terminal::tests_osc_notify`，含跨读 pin `split_across_reads_is_dropped`）+ store 9 + `should_toast` 2 = **29 GAP-09 单测绿**；`cargo test -p pureterm` 全量无回归。**Review ✅（2026-06-29）**：修 BUG-1（节流铃铛仍弹 toast / 写审计 —— `NotificationStore::post()` 返回值三处调用点门控）/ BUG-2（跨 PTY 读通知丢失的误导性注释订正 + pin 测试）/ BUG-3（`EventBus` Mutex 中毒加固）。

> CI 自动 harness 已覆盖**逻辑不变量**:
> - `NotificationStore::post` unread/attention/bell 节流 / OSC 99 same-id replace / ring cap 淘汰
> - `ChannelEventListener` 转发 `Event::Notification` / `Event::Bell`
> - `should_toast` 焦点/可见性门控（desktop toast 逻辑，待 handler 接线后 GUI 验证）
>
> CI 无法证明**真实视觉**: amber ring、状态栏 🔔、⌘⇧M 面板、Dock 角标、macOS 桌面 toast。

## 前置

```bash
./build.sh app          # 重打包 dist/PureTerm.app
open dist/PureTerm.app
```

确认 `~/.config/pureterm/alacritty.toml`（或 pureterm 配置）含：

```toml
[notification]
enabled = true
dock_badge = true
ring = true
bell_passthrough = true
```

## 场景 1 —— OSC 9 触发 attention ring + 状态栏计数

1. 在活跃 pane 执行：`printf '\033]9;Hello from OSC9\007'`
2. 观察：该 pane **amber 边框**；状态栏出现 **🔔 1**。
3. 点击该 pane（或已聚焦则切走再切回）→ ring 消失，🔔 变 0。

## 场景 2 —— Bell (0x07) + 节流

1. 执行：`printf '\007'` 三次快速连敲。
2. 预期：ring 脉冲，但 **~1s 内只记 1 条** bell 通知（`bell_throttle_ms = 1000`）。

## 场景 3 —— ⌘⇧M 通知面板 + 聚焦源 pane

1. 在 pane A 发 OSC 9；切到 pane B。
2. `⌘⇧M` 打开通知面板 → 列表含 pane A 条目（来源 pane、时间戳、body）。
3. 点击条目 → 焦点回到 pane A，该条 unread 清除。

## 场景 4 —— Dock 角标（macOS only）

1. 在后台 split 的 pane 发 OSC 9（当前焦点在另一 pane）。
2. 观察 Dock 上 PureTerm 图标角标为 **1**。
3. 聚焦源 pane 或面板 Clear → 角标清除。

## 场景 5 —— EventBus `notification.*`

1. 终端：`pureterm events tail --type notification`
2. GUI 发 OSC 9 / 开闭 ⌘⇧M 面板 / Clear all。

**预期** 依次见 `notification.posted` / `panel_opened` / `panel_closed` / `all_dismissed`。

## 场景 6 —— desktop toast（macOS only）

> `fire_toast()` 已在 Notification/Bell/CLI handler 接线；门控 `should_toast` —— pane 不可见（或 app 失焦）才弹。

1. 失焦 PureTerm（切到其他 app），或让通知来自非活跃 split pane。
2. 在不可见 pane 发 OSC 9：`printf '\033]9;Build complete\007'`
3. 预期：macOS 通知中心弹 toast（需 `./build.sh app` bundle id + 首次授权 `.alert|.sound|.badge`）。
4. OSC 99 显式 occasion：`printf '\033]99;o=unfocused;Build done\033\\'`（仅 app 失焦弹）/ `o=invisible`（pane 不可见弹）。

## 场景 7 —— `pureterm notify` CLI

1. 另开终端（PureTerm 失焦）：`pureterm notify "deploy finished" -t CI`
2. 预期：GUI store +1、Dock 角标 +1、（失焦时）桌面 toast。
3. `pureterm events tail --type notification` 见一条 `source:"cli"` 的 `notification.posted`。
4. `pureterm notify --pane-id 3 "hi"`（指定 pane，默认活跃 pane）。

## 通过判据（非阻塞 follow-up）

- [ ] 场景 1：OSC 9 → ring + 🔔 + 聚焦清除
- [ ] 场景 2：Bell 节流
- [ ] 场景 3：⌘⇧M 面板 + 点击聚焦
- [ ] 场景 4：Dock 角标（macOS）
- [ ] 场景 5：`events tail --type notification`
- [ ] 场景 6：desktop toast（macOS）
- [ ] 场景 7：`pureterm notify` CLI

## 已知边界（非 DoD）

- **`FocusNotificationPane` v1**：点击面板条目仅清该 pane unread，**跨 tab 聚焦**待补（同 tab 内 active 已清）。
- **Linux dbus 通知**：未实现（用户决定 macOS-only）。
- **Feed 待审批 Dock 叠加**：属 #21 Dock Badge 独立 scope。
