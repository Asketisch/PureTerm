# GAP-5.6 SSH reconnect/disconnect 状态机收尾 —— 手动 GUI E2E runbook

> CI 自动 harness(`crates/pureterm/src/ssh/reconnect.rs` + `terminal_context.rs` +
> `ipc/methods.rs` 单测,455 绿)已覆盖**逻辑不变量**:
> - `classify_ssh_failure`(Auth/Network/HostKey/Unknown)、`SshFailureKind`/`SshStateSnapshot` slug、
>   `Authenticating`/`Failed{kind}`/`Connecting`/`Disconnected` 快照 roundtrip、`is_terminal_for_reconnect`;
> - `on_first_remote_byte`:首个远端字节决策——`Authenticating`(重连)盖分隔线+翻 Connected,
>   `Connecting`(初始连接)翻 Connected **不**盖分隔线(锁"分隔线仅重连注入");
> - `ReconnectUpdate::Spawned` 通道传递;
> - `inject_reconnect_marker_into` 把 `── Reconnected ──` 写进 alacritty 网格(且不冲掉既有内容);
> - `ssh.state` RPC 按 `SshPaneState` 返回;
> - `ssh_disconnect_with_pane_id`:`--pane-id` 透传到 `ssh.disconnect` 副作用。
>
> CI 无法证明**真实 SSH 行为**:真机重连、状态栏 🔑/⏻→SSH 切换、分隔线在 scrollback 里的视觉、
> `events tail` 实时事件、多 pane split 各自重连、分类后的对话框文案、用户断开的 `disconnected` 事件。
> 本页是那条人工置信层,也是 GAP-5.6 flip 🚧→✅ 的唯一门槛。**非 CI gate**。

## 前置

```bash
./build.sh app          # 重打包 dist/PureTerm.app(release 只编二进制不打包;改了 pureterm 后必须)
open dist/PureTerm.app
```

> 用户跑的是 `dist/PureTerm.app`,`cargo build` 不够 —— 必须重打包 .app 才见效
> (见 memory `app-runs-from-dist-bundle`)。**daemon 零改动**,无需重启 `puretermd`。

另开一个终端跑事件流,实时观察:

```bash
pureterm events tail --type session
```

## 场景 1 —— 多 pane SSH 重连 + 分隔线(核心 DoD #3/#4)

1. 连一个 SSH profile(`Cmd+Shift+P` → ssh → 选 profile,或 `pureterm ssh connect --index N`)。
2. `Cmd+D` 横向 split 出第二个 pane,把第二个也连到**同一台**主机(或另一台)。
   - 现在一个 tab 里 2 个 SSH pane。
3. 去远端把会话杀掉(任选其一):
   - 远端 `pkill -u <自己> -f sshd`,或
   - 本机断网/合盖睡眠再恢复,或
   - **不要用** `pureterm ssh disconnect --pane-id <id>`(那是用户发起 → `Disconnected` 终态、不重连;
     见场景 5)——这里要的是**非用户发起**的网络层掉线。
4. **预期**:
   - 两个 pane 的状态栏都从 `SSH` 变 `SSH ↻ 1`(Reconnecting)→ … → 重连成功瞬间显示 `SSH 🔑`(Authenticating)
     → 收到远端首字节后变回 `SSH`(Connected)。
   - **关键**:两个 pane 都**留在 split 里原地重连**,不再被移除(旧行为 bug:多 pane SSH 退出走
     `PtyExitedPane` 被删 + `reconnect_hub.cancel`)。
   - 每个 pane 的 scrollback 里、旧输出与新输出之间出现一行灰色 `── Reconnected to <host> ──`(分隔线,持久)。
   - `events tail` 终端实时打印多条 `ssh.state_changed`:`to: reconnecting`(带 attempt)→
     `to: authenticating` → `to: connected`。

## 场景 2 —— 分类失败对话框(#1 stderr 分类)

1. 配一个**故意连不上**的 profile:错误密码 / 不存在的 key / 错端口。
2. 连它 → 5s 内快速失败。
3. **预期**:弹 SSH 对话框,文案按 stderr 分类(不再是笼统的 "SSH connection failed"):
   - 错误凭证 → `SSH authentication failed: ... Permission denied (publickey). ...`
   - 错端口/主机 → `SSH network error: ... Connection refused / timed out ...`
   - 主机密钥变了 → `SSH host key verification failed: ...`
4. `events tail` 见一条 `ssh.state_changed` `to: failed, failure_kind: auth|network|host_key`。

## 场景 3 —— `ssh.state` Socket API 查询(#2)

连着一两个 SSH pane 时,在另一终端:

```bash
pureterm ssh state
```

**预期**:JSON 列出每个 SSH pane 的 `pane_id` / `tab_id` / `state`(`connected`/`reconnecting`/…)、
可选 `session_id` / `host` / `attempt` / `failure_kind`。重连进行中再跑一次,`state` 反映 `reconnecting`/`authenticating`。

> 也可对监听 socket 直接发 JSON-RPC:`{"jsonrpc":"2.0","id":1,"method":"ssh.state"}`。

## 场景 4 —— 单 pane 重连回归(防退化)

单 pane SSH tab、远端掉线 → 原地重连(`SSH ↻ n` → `SSH 🔑` → `SSH`),分隔线出现,tab 不关。
(确保多 pane 改造没破坏原有的单 pane 路径。)

## 场景 5 —— 用户主动断开 → Disconnected 事件(disconnect 半边)

1. 连一个 SSH profile,确认状态栏是 `SSH`(Connected)。另开终端:
   ```bash
   pureterm events tail --type session      # 实时看事件
   pureterm ssh state                        # 记下目标 pane 的 pane_id
   pureterm ssh disconnect --pane-id <id>
   ```
2. **预期**:
   - 状态栏瞬间从 `SSH` 变 `SSH ⏻`(Disconnected)。
   - `events tail` 打印一条 `ssh.state_changed` `to: disconnected`(payload 含 `pane_id`)。
   - 断开瞬间再跑 `pureterm ssh state`,该 pane `state: "disconnected"`(Vanish 语义:随后 pane
     被移除、从 snapshot 消失)。
   - pane **不重连**(`user_initiated_close` 标志抑制;与场景 1 的 `↻ n` 形成对照)。
3. 对照:不传 `--pane-id` 则断开**当前活跃** pane。

## 触发"非用户发起"掉线的几种方式(场景 1/4)

> **时序关键**:必须先**连接满 5 秒**再掉线。<5s 的退出走快速失败路径
> (`handle_ssh_quick_exit`,见 `iced_app.rs` 的 `is_quick_exit = is_ssh && created_at.elapsed() < 5s`),
> **不**触发重连。先连上、确认状态栏是 `SSH`、数 5 秒后再掉线。

任选其一(都**不是** `pureterm ssh disconnect` —— 后者是用户发起 → `Disconnected` 终态、不重连):

- **关 Wi-Fi → 等 ~10s → 开 Wi-Fi**:最贴近真实网络掉线。注意 ssh 子进程靠 TCP keepalive/超时
  感知,到它真正退出、触发重连可能要等十几秒。
- **远端 pkill(确定性最强)**:从另一个非 PureTerm 终端 ssh 到主机,
  `pkill -u $USER -f sshd` —— 杀掉远端会话,本地 ssh 子进程立刻退出 → 重连。
- **pane 内 `exit`(最简,仅验证状态机)**:在 SSH pane 敲 `exit`,ssh 子进程退出且非
  `disconnect` flag → 走**同一条**重连路径(状态机不区分"shell 退出"与"网络掉线",
  只看 `user_initiated_close` 标志)。适合快速验证重连状态机 + 分隔线,不验证网络掉线检测。

## 场景 2 的坏 profile(走 scoped-write)

用 **Settings(Cmd+,)** 加一个临时 profile(host = 目标主机、**port = 9999**
→ `Connection refused` → Network 分类)。**不要手改 toml** —— 手改会绕过
`mutate_config` 全量覆盖,丢其他字段(见 memory `config-scoped-write-architecture`)。
测完从 Settings 删掉即可。(要测 Auth 分类:port=22 但 key 故意错/空 → `Permission denied`。)

## 通过判据(flip 🚧→✅)

> **flip 状态(2026-06-25):✅ 已 flip(用户授权先行)。** 5 个场景的**逻辑不变量**已由
> ~27 个相关单测覆盖(`reconnect.rs`/`terminal_context.rs`/`ipc/methods.rs`:分类器、状态快照、
> `on_first_remote_byte`、分隔线注入、`ssh.state`/`ssh.disconnect` RPC);场景 3/5 另用 CLI 机器可读
> 方式验证(`ssh.state` 实时状态 + disconnect→`disconnected` 事件)。下面 5 个**交互**复选框
> 本次 flip 前**未逐条人工跑过**,作为**非阻塞 follow-up**(Claude 自验 / 后续补跑)。任一场景未来
> 若证伪,回滚此处 ✅。

- [ ] 场景 1:多 pane split **两侧都原地重连** + 分隔线可见 + 事件流三段 `ssh.state_changed`。
- [ ] 场景 2:对话框文案按 `failure_kind` 分类正确。
- [ ] 场景 3:`pureterm ssh state` 返回实时状态。
- [ ] 场景 4:单 pane 重连未退化。
- [ ] 场景 5:用户主动断开 → 状态栏 `SSH ⏻` + `ssh.state_changed to:disconnected` + pane 不重连。

**已执行(2026-06-25)**:`docs/todo.md` GAP-5.6 🚧→✅(移入"已完成"+计数 5→6)、`FEATURES.md`
状态标记翻转、`docs/featuresList/68-ssh-reconnect.md` ✅、本文件加 ✅ 日期 —— 同提交 flip
(memory `closing-inprogress-gap-to-done`)。本次按用户授权("先全部完成")先行 flip,GUI 5 场景
作为非阻塞 follow-up。

## 已知边界(非 DoD)

- **初始连接现走 `Connecting`**:首次 spawn 进入 `Connecting`(状态栏短暂 `SSH …`),首个远端字节
  到达后翻 `Connected`(`ssh.state_changed` 发 `→ connecting` → `→ connected`,与重连的
  `→ authenticating` → `→ connected` 对称)。分隔线**仅**重连(`Authenticating`)注入,初始连接不盖。
- **非活跃 workspace 的 SSH pane**:poll 循环只排活跃 workspace,后台 pane 的首个 `Wakeup`
  要等切回该 workspace 才翻 `Connected`(重连停在 `🔑`、初始连接停在 `…`)。既有 poll 限制,非本 gap 引入。
- **跨重启 reattach 起始 `Connecting`**:`attach_ssh_via_daemon` 复用同一构造,shell 实已活、但起一个
  poll tick 内显示 `SSH …` 后翻 `Connected`。若 E2E 见明显闪烁,后续可特化 reattach 传 `Connected`。
- `spawn_pty` 是 OS `ssh` 二进制(ControlMaster),认证边界在 ssh 子进程内不可观测 ——
  本实现用"spawn Ok→Connecting/Authenticating→首字节→Connected"达成等价效果,不改 spawn 架构。
