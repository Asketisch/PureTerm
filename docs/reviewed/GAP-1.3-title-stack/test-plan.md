# GAP-1.3 标题栈 / OSC 动态标题(CSI 22t/23t + OSC 0/2)—— 手动 GUI E2E runbook

> ✅ **已通过(2026-06-25)** — GAP-1.3 已 flip ✅。下列场景作为回归用例保留。

> CI 自动 harness(`crates/pureterm/src/terminal_context.rs` + `iced_app.rs` 单测,**461 绿**)已覆盖**逻辑不变量**:
> - `ChannelEventListener::send_event` 转发 `Event::Title` / `Event::ResetTitle`(此前被 `_ => return` 丢弃),未映射事件仍 drop;
> - `resolve_display_title`:OSC 非空优先 / `None` 或空串回退派生标题。
>
> CI 无法证明**真实视觉**:窗口标题 / Tab 栏随 OSC / 22t / 23t 实时变化、SSH 远端 cwd 进标题、reset 回退派生、多 pane 取活跃 pane。
> 本页是那条人工置信层,也是 GAP-1.3 flip 🚧→✅ 的唯一门槛。**非 CI gate**。

## 前置

```bash
./build.sh app          # 重打包 dist/PureTerm.app(改了 pureterm 后必须;cargo build 不够)
open dist/PureTerm.app
```

> 用户跑的是 `dist/PureTerm.app`(见 memory `app-runs-from-dist-bundle`)。

## 场景 1 —— 本地 Tab push/pop 还原(核心 DoD)

开一个本地 Tab,在 shell 里跑:

```bash
printf '\e]0;Original\a'; printf '\e[22t'; printf '\e]0;Temp\a'; printf '\e[23t'
```

**预期**(观察窗口标题 + Tab 栏):
1. `Original` → 标题变 **Original**
2.(push 当前 Original 入栈)
3. `Temp` → 标题变 **Temp**
4.(pop)→ 标题**还原回 Original**

> 注:DoD 字面 `echo -e "\e[22;0t" && echo -e "\e[23;0t"` 是连续 push/pop、无中间 set,视觉上 no-op(不应报错)——**真实验证栈语义靠本场景**。

## 场景 2 —— reset 回退派生标题

```bash
printf '\e]0;Whatever\a'   # 标题变 Whatever
printf '\e]0;\a'           # 空标题 = reset
```

**预期**:第二步后标题**回退派生** `#1`(本地 Tab)/ `🔒 SSH: <name>`(SSH Tab)。

## 场景 3 —— SSH 远端 cwd 进标题(高价值)

1. 连一个 SSH profile(`Cmd+Shift+P` → ssh → 选 profile,或 `pureterm ssh connect --index N`)。
2. 远端 shell 通常每个 prompt 自动设标题(如 `user@host: ~/dir`)。
3. **预期**:窗口/Tab 标题显示远端 `user@host: ~/path`,随 `cd` 实时变化;断连/重连后回退 `🔒 SSH: <name>`。

## 场景 4 —— 多 pane(分屏)取活跃 pane 标题

1. `Cmd+D` split 出第二个 pane,两边各跑 `printf '\e]0;Left\a'` / `printf '\e]0;Right\a'`。
2. **预期**:Tab 标题随**焦点 pane**切换显示 Left / Right。

## 通过标准

- 场景 1 的 push/pop 还原可见 ✅(DoD 核心)
- 场景 2 的 reset 回退派生 ✅
- 场景 3 的 SSH 远端标题 ✅(可选但高价值)

全过 → flip GAP-1.3 🚧→✅ + FEATURES.md 去掉「待手动 GUI E2E」尾注 + 写 memory `gap13-title-stack.md`。
