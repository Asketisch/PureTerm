# tmux 兼容子集 —— 手动 display-smoke runbook

> CI 自动 harness(`crates/pureterm/src/tmux_cli.rs` `#[cfg(test)]` +
> `tests/round_trip`、`split_tree` / `surface_snapshot` 单测、`join_pane_e2e`、
> `pureterm-daemon` `pipe_pane`)已覆盖 GAP-6.6 的**行为逻辑**:target 解析、
> CLI↔router JSON-RPC 往返 + `IpcSideEffect` 发射、split 树操作、capture() 真实
> 读栅格、join-pane Arc-follows 不变量、pipe-pane pump→子进程字节落地。CI 无法
> 证明**像素渲染**与**真实 GUI/daemon 联动**(分屏几何、join-pane 跨 tab 渲染、
> focus 高亮、capture-pane 输出、pipe-pane/wait-for 的真实 PTY)。本页是那条人工
> 置信层 —— 每次相关代码改动后建议跑一次,勾选签字。
>
> **非 CI gate**。GAP-6.6 翻 ✅ 时注明「CI: 自动 harness;手动: 本文件」(同
> [`agent-features-smoke.md`](agent-features-smoke.md) 的约定)。

## 前置

```bash
./build.sh app          # 重打包 dist/PureTerm.app(改了 pureterm/puretermd 后必须)
open dist/PureTerm.app
```

在 PureTerm 里开 ≥2 个 tab,其中一个分出 ≥2 个 pane(`Cmd+D` / `Cmd+Shift+D`),
这样 `%N` / tab index / swap / join 都有靶子。`pureterm tmux` 走 Socket API v2,
复用 GUI 的 IPC socket(`PURETERM_SOCKET` 或目录扫描)。**在外部终端**跑下列命令
(不是在 PureTerm pane 里 —— 那样会被自己的 PTY 干扰)。

> pane id 从 1 起自增;`%0` 是「第一个 pane」的回退别名(DoD 用它)。先
> `pureterm tmux list-panes` 看真实 id。

---

## GAP-6.6 命令逐条

### capture-pane(DoD:`-p` 输出最近 50 行)

```bash
pureterm tmux capture-pane -t %0 -p
```

- [ ] 输出该 pane 当前可见 + 滚动的文本(非空,与屏幕一致)
- [ ] 不带 `--lines` 时默认截到最后 50 行(`-p` 语义);加 `--lines 10` 截到 10 行
- [ ] `-S 0` 只取当前视口(可见部分);`-S -` 取全滚动历史
- [ ] 输出到 stdout(无 `[pureterm tmux]` 噪声;不带 `-p` 时有提示)

### list-panes

```bash
pureterm tmux list-panes
pureterm tmux list-panes -F "#{pane_id} tab=#{tab_index} active=#{pane_active} cwd=#{pane_current_path}"
```

- [ ] 列出所有 tab 的所有 pane(跨 tab 扁平,DFS 序),每行一个
- [ ] 默认格式含 `pane_id` / `pane_index` / `tab_index` / `tab_title` / `active`
- [ ] `-F` 自定义格式串变量正确替换;`#{pane_active}` 对聚焦 pane 为 `1`

### send-keys

```bash
pureterm tmux send-keys -t %0 "echo hello-tmux" Enter
pureterm tmux send-keys -t %0 Up            # 方向键
```

- [ ] 文本 + `Enter`(`\r`)写入目标 pane 的 PTY,shell 执行 `echo` 并回显
- [ ] 已知名(Enter/Tab/Escape/Space/BSpace/Up/Down/…)翻译正确;字面量(如 `ls`)原样拼接
- [ ] `-t` 省略 → 写活跃 pane;`%N` / `:+` / `:-` 都能命中

### split-window

```bash
pureterm tmux split-window        # 默认上下堆叠(vertical)
pureterm tmux split-window -h     # 左右并排(horizontal)
```

- [ ] 无 `-h`:活跃 pane 上下分成两块(水平分割线)
- [ ] `-h`:左右并排(竖直分割线);新 pane 各自起 shell

### kill-pane

```bash
pureterm tmux kill-pane           # 关闭活跃 pane
```

- [ ] 活跃 pane 关闭,PTY/daemon session 结束;剩余 pane 重排
- [ ] (已知限制)`-t %N` 定向关闭**未实现** —— 现仍关活跃 pane 并 warn,见下方「已知限制」

### select-pane

```bash
pureterm tmux select-pane :+      # 下一个
pureterm tmux select-pane :-      # 上一个
```

- [ ] `:+` / `:-` 在 DFS 序里移动焦点(高亮边框跳到目标 pane)
- [ ] (已知限制)`-L/-R/-U/-D` 几何方向、`-l` last **未实现**,报 phase-2 错误

### resize-pane

```bash
pureterm tmux resize-pane -R 5    # 右扩
pureterm tmux resize-pane -L 5
pureterm tmux resize-pane -x 100 -y 30    # 绝对列/行
```

- [ ] 方向 + 数量改变分割比例(每个 amount ≈ 3% ratio),分割线实时移动
- [ ] `-x` / `-y` 绝对尺寸生效;比例被 clamp 到 [MIN_RATIO, MAX_RATIO],不爆框

### swap-pane

```bash
pureterm tmux swap-pane -s %1 -t %2   # 按 id 交换两 pane 内容位置
pureterm tmux swap-pane -D            # 活跃 pane 与下一个邻居交换
```

- [ ] `-s A -t B`:两 pane 的位置(内容)互换,树结构与比例保持
- [ ] `-U`/`-D`:活跃 pane 与 DFS 上一个/下一个邻居交换

### break-pane(pane → 新 tab)

```bash
pureterm tmux break-pane -s %1
```

- [ ] 源 pane 从当前 tab 移出,**升为一个新的独立 tab**(标题、PTY、滚动历史跟随)
- [ ] 原 tab 剩余 pane 重排;若源是原 tab 唯一 pane,则不变(不能搬空)

### join-pane(跨 tab 移动 — 重点)

```bash
# 把 tab0 的某 pane 搬进 tab1(作为 tab1 活跃 pane 的 sibling)
pureterm tmux join-pane -s %1 -t 1
pureterm tmux join-pane -s %1 -t 1 -h    # 水平 sibling
```

- [ ] 源 pane 从源 tab 消失、出现在目标 tab,几何并入(split-tree 合法)
- [ ] **PTY/session 存活**(Arc-follows 不变量的真实落地):搬完后对它
      `pureterm tmux send-keys -t <new_id> "echo still-alive" Enter`,**原进程、
      滚动历史、前台 job 都在**(没有重连/重新认证);SSH pane 同理不断开
- [ ] 源 tab 剩 ≥1 pane(不搬空);`-h` 决定 sibling 朝向
- [ ] `-t` 接 tab index 或 `%N`(取该 pane 所在 tab)

### pipe-pane(PTY 输出 → shell 命令)

```bash
pureterm tmux pipe-pane -t %0 'cat > /tmp/pane.log'   # 开启
# ... 在该 pane 里产生输出(ls / 编译 / 任何回显)...
cat /tmp/pane.log                                       # 应见 PTY 输出
pureterm tmux pipe-pane -t %0                            # 无命令 → 停止
```

- [ ] 开启后,该 pane 的 PTY 输出实时落进 `/tmp/pane.log`(pump→drain→`sh -c`)
- [ ] 换命令(`pipe-pane ... 'tee /tmp/x | wc -l > /tmp/n'`)替换旧 pipe,旧子进程被回收
- [ ] 无命令的 `pipe-pane -t %0` 停止管道;停后新输出不再落文件

### wait-for(跨进程 channel/lock 同步)

在**两个外部终端**里:

```bash
# 终端 A:阻塞等待
pureterm tmux wait-for deploy-done
# 终端 B:唤醒
pureterm tmux wait-for -S deploy-done        # signal 唤醒所有 waiter
```

- [ ] 终端 A 阻塞,直到终端 B `-S` 才返回;signal 报告唤醒的 waiter 数
- [ ] `-L name` 取锁 / `-U name` 释放一个(FIFO);多 locker 阻塞,`-U` 只放一个
- [ ] 直连 daemon socket(绕过 `daemon_client` 5s 超时),park 无上限,匹配 tmux

---

## 已知限制(非 bug,phase 记录)

- **`kill-pane -t`**:定向关闭未实现,现关活跃 pane 并 warn(`tmux_cli.rs` phase-2 注释)。
- **`select-pane -L/-R/-U/-D` 与 `-l`**:几何方向 / last-pane 未实现,报 phase-2 错误。
- **`capture-pane` 无粘贴缓冲**:不带 `-p` 时仍输出到 stdout并提示(无 tmux paste buffer)。
- **`session:win.pane` 完整 target 语法**:不解析(仅 `%N` / `:+` / `:-` / 省略)。
- **`select-layout`**:所有预设布局统一映射到 equalize(不区分 even-/tiled/main-)。

## 已知环境注意

- **clippy**:`cargo clippy --workspace -- -D warnings` 当前被 vendored `alacritty_terminal`
  的 `#![cfg_attr(clippy, deny(warnings))]` + 新 clippy lint 阻塞(预先存在,非本工作引入;
  CLAUDE.md 禁止无关格式化 vendored 源码)。本工作新增代码无 rustc 警告。
- **fmt**:repo 的 rustfmt 配置用 nightly-only 选项(`imports_granularity`/`group_imports`),
  stable rustfmt 会报整库 diff(预先存在)。
- **daemon `instance_lock_is_exclusive` 测试**:本机若有 `puretermd` 在跑会失败(环境性,
  非代码问题)。GAP-6.6 的 daemon 单测(`pipe_pane` 等)不受影响。
