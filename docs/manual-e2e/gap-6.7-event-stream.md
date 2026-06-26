# GAP-6.7 Event Stream（事件流 + 持久 JSONL 日志）—— 手动 GUI E2E runbook

> 🚧 **待人工通过** — GAP-6.7 代码闭环 + harness 全绿，本页是 flip 🚧→✅ 的人工置信层。**非 CI gate。**

> CI 自动 harness（`crates/pureterm/src/{events.rs,events_bus.rs,events_log.rs}` 单测，**495 pureterm 绿**）已覆盖**逻辑不变量**:
> - `EventBus` 双写:每条事件落 `events.jsonl`(16MiB rotation + `.1` 归档)+ broadcast(4096);
> - `AppEvent` JSONL round-trip / null-data 省略 / `TypeFilter` category 前缀与 dotted-kind 精确匹配;
> - `read_recent` newest-first / `since_seq` 游标 / 跳过坏行与缺文件;
> - `drain_once`(读路径抽出的纯函数):完整行才 emit / 半行跨次缓冲 / rotation(inode 变化)从头重读;
> - `tail_follow` 端到端:append 后 ~poll 间隔内送达 + rotation 后新事件仍送达(集成测,分离线程)。
>
> CI 无法证明**真实视觉**:`pureterm events tail` 跟随真实 GUI 动作实时流出、`--type workspace` 只出 workspace 类(验证误分类修正)、`events list` 回读历史。本页覆盖这些。

## 前置

```bash
./build.sh app          # 重打包 dist/PureTerm.app(改了 pureterm 后必须;cargo build 不够)
open dist/PureTerm.app
```

> 用户跑的是 `dist/PureTerm.app`(见 memory `app-runs-from-dist-bundle`)。E2E 时另开一个终端跑 `pureterm events ...`(指向同一份 `~/Library/Application Support/pureterm/events.jsonl`)。

## 场景 1 —— `events tail` 实时跟随 GUI 动作(核心 DoD)

1. 启动 `dist/PureTerm.app`。
2. 另开终端:`pureterm events tail`(默认跟全部类别)。
3. 在 GUI 里依次:开新 Tab(`Cmd+T`)、分屏(`Cmd+D`)、切 Workspace。

**预期**:第二终端**实时**流出事件,每行形如 `<seq> <category> <kind> <data>`,例如:
```
12 tab.created {"tab_id":1,"pane_id":1}
13 pane.split {"..."}
14 workspace.switched {"workspace":"Workspace 2"}
```
核对 category/kind 与动作对应(tab/pane/workspace/session/feed/...)。

## 场景 2 —— `--type workspace` 只出 workspace 类(验证误分类修正)

> GAP-6.7 收尾把 `workspace.created`/`workspace.switched` 从误标的 `surface` 修正为 `workspace`。本场景验证它们不再漏到 `surface` 下。

1. 终端 A:`pureterm events tail --type workspace`。
2. GUI:新建 Workspace(`Cmd+Shift+P` → workspace.new,或快捷键)、再切回去。

**预期**:
- 终端 A 只出 `workspace.created` / `workspace.switched` 两类,category 列为 `workspace`;
- 对照 `pureterm events tail --type surface`(另一终端):workspace 动作**不**出现在 surface 流里(tab.* 仍在 surface)。

## 场景 3 —— `events list` 持久回读历史

1. GUI 跑一会儿产生若干事件后,`pureterm events quit`(或直接读盘)。
2. 终端:`pureterm events list --limit 20`。
3. 再试精确 kind:`pureterm events list --type feed.resolved`(若有 feed 事件)。

**预期**:
- `list` 从 `events.jsonl` 回读,**newest-first** 表格(`SEQ / CATEGORY / KIND / DATA`),`--limit` 截断;
- `--type feed.resolved` 只出该精确 kind(验证 dotted-kind 匹配)。

## 场景 4 —— tail 延迟与空闲唤醒

> GAP-6.7 收尾把 `tail_follow` 的轮询从 75ms 放宽到 150ms(≈ 空闲唤醒减半),并把读路径抽出可单测的 `drain_once`。

1. 终端 A:`pureterm events tail`。
2. GUI 触发一个动作(如开 Tab)。

**预期**:终端 A 在 ~150ms 内(人眼基本即时)出现对应事件;tail 常驻期间空闲 CPU 占用极低(150ms 轮询 + 每轮一次 0 字节 `read_to_end`)。

> 注:GAP-6.7 原计划用 `notify`(kqueue/inotify)替轮询,但实测 macOS `RecommendedWatcher` 走 FSEvents,事件 ~1s 才到(比 150ms 轮询还慢)且其 runloop 会拖住同进程其它线程的 sleep —— 在主平台无收益反有害。Linux inotify 倒是毫秒级,但 PureTerm 主平台是 macOS,故最终落地为「轮询放宽 + 读路径重构」,`notify` 暂不引入。详见 `events_log.rs::TAIL_FALLBACK_MS` 注释。

## 通过判据(flip 🚧→✅)

- [ ] 场景 1:`events tail` 实时跟随 GUI 动作 ✅(DoD 核心)
- [ ] 场景 2:`--type workspace` 出 `workspace.*` 且不漏到 surface ✅
- [ ] 场景 3:`events list` newest-first 回读 + dotted-kind 过滤 ✅
- [ ] 场景 4:tail ~150ms 内响应、空闲低 CPU ✅

全过 → flip GAP-6.7 🚧→✅ + FEATURES.md 加块 + `docs/featuresList/66-event-stream.md` 改状态。

## 已知边界(非 DoD)

- **`events.subscribe` 流式 MCP 工具**:未实现(需独立流式传输设计;当前 IPC 是阻塞式 `send_rpc`)。留作非阻塞后续。
- **`notification.*` producer**:未实现,被 GAP-09(Notification System)阻塞 —— OSC 9/99/777 解析器与 `Event::Bell` GUI 处理尚无。`Category::Notification` 已在 schema 预留。
- **sidebar producer**:未实现,被 GAP-6.8(可编程 sidebar)阻塞 —— sidebar 模块尚不存在。`Category::Sidebar` 已预留。
- **rotation(16MiB)**:难以手动触发,由 `events_log::tests::rotation_archives_active_file_at_threshold` + `drain_once_reopens_from_start_after_rotation` 单测覆盖,非手动 DoD。
