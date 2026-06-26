# Event Stream（事件流 + 持久 JSONL 日志）

## 概述

`EventBus` 双写架构：events.jsonl（16MiB rotation）+ broadcast（4096 容量）。支持 12 类事件、实时 tail（`pureterm events list/tail`）、~20 个 emit 点。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | EventStream（`Sources/EventStream.swift`），实时事件流 | `Sources/EventStream.swift` |

## PureTerm 现状

**✅ 已闭环（2026-06-26）。** 495 pureterm 单测全绿 + release 编译通过 + dist 重打包 + CLI 冒烟通过。GUI 手动 E2E 见 [`docs/manual-e2e/gap-6.7-event-stream.md`](../manual-e2e/gap-6.7-event-stream.md)（非 CI gate）。

**架构**：
- `EventBus` 双写：events.jsonl 16MiB rotation + broadcast 4096
- 12 类事件
- 实时 tail：`pureterm events list/tail`
- ~20 emit 点（feed/tab/pane/session/config/agent）

**已完成（本次收尾）**：
- workspace producer 误分类修正（`workspace.created`/`workspace.switched` 从 `Surface` 修正为 `Workspace`）
- `tail_follow` 读路径重构：抽出可单测的 `drain_once`，轮询 75ms→150ms（≈ 空闲唤醒减半）
- dist `.app` 重打包 + 手动 GUI E2E runbook（`docs/manual-e2e/gap-6.7-event-stream.md`）

**非阻塞后续**：
- `events.subscribe` streaming MCP 工具（需独立流式传输设计，当前 IPC 是阻塞式 `send_rpc`）
- `notification.*` producer（被 GAP-09 阻塞 —— OSC 9/99/777 解析器与 Bell GUI 处理尚无）
- sidebar producer（被 GAP-6.8 阻塞 —— sidebar 模块尚不存在）
- `notify`（kqueue/inotify）替轮询：经实测评估弃用 —— macOS `RecommendedWatcher` 走 FSEvents，事件 ~1s 才到（比 150ms 轮询还慢）且其 runloop 会拖住同进程其它线程的 sleep，主平台无收益反有害（详见 `events_log.rs::TAIL_FALLBACK_MS` 注释）

## 优先级

**P0 — AI 核心管道**

事件流是 AI agent 集成的基础设施，所有自动化工作流都依赖事件通知。

## 工作量

**M（1 周）— 收尾**

代码已闭环，仅需 E2E 验证和优化。

## 实现方向

1. ✅ E2E：`./build.sh app` → 启 GUI + 触发动作 → `pureterm events tail` 验证（runbook：`docs/manual-e2e/gap-6.7-event-stream.md`）
2. 后续：`events.subscribe` streaming API
3. 后续：notification producer（对接系统通知，依赖 GAP-09）
4. ~~后续：kqueue/inotify 替代轮询~~ —— 经实测评估弃用（macOS FSEvents 无收益反有害，见上）
