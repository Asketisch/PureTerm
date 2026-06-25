# Event Stream（事件流 + 持久 JSONL 日志）

## 概述

`EventBus` 双写架构：events.jsonl（16MiB rotation）+ broadcast（4096 容量）。支持 12 类事件、实时 tail（`pureterm events list/tail`）、~20 个 emit 点。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **cmux** | EventStream（`Sources/EventStream.swift`），实时事件流 | `Sources/EventStream.swift` |

## PureTerm 现状

**🚧 进行中（代码闭环，待 E2E）。** 270 单测全绿 + release 编译通过 + CLI 冒烟通过。

**架构**：
- `EventBus` 双写：events.jsonl 16MiB rotation + broadcast 4096
- 12 类事件
- 实时 tail：`pureterm events list/tail`
- ~20 emit 点（feed/tab/pane/session/config/agent）

**待完成**：
- dist `.app` 重打包 + 手动 GUI E2E
- `events.subscribe` streaming
- `notification.*` producer
- workspace/sidebar/browser producer
- kqueue/inotify 优化

## 优先级

**P0 — AI 核心管道**

事件流是 AI agent 集成的基础设施，所有自动化工作流都依赖事件通知。

## 工作量

**M（1 周）— 收尾**

代码已闭环，仅需 E2E 验证和优化。

## 实现方向

1. E2E：`./build.sh release` → 启 GUI + Claude Code 触发审批 → `pureterm events tail --type feed` 验证
2. 后续：`events.subscribe` streaming API
3. 后续：notification producer（对接系统通知）
4. 后续：kqueue/inotify 替代轮询
