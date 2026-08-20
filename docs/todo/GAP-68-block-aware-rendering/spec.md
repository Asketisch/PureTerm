# GAP-68 — Block-aware 主画面渲染

## 背景 / 动机

GAP-51 已建立 block-based output 数据模型（`output_blocks.rs`，OSC 133 C mark 触发），但**渲染层不消费**——主画面仍走 grid snapshot（cell 数组），block 模型仅服务右侧 Blocks 侧栏（`view_blocks.rs:82`）。这是 Warp block 模型 vs 传统流式 buffer 的核心差异点：让每个命令块在主画面有视觉边界（头部命令行 + exit code badge + 可折叠）。

## 目标

1. 主画面渲染时叠加 block 装饰层：块头部命令badge、exit code 着色（绿/红）、块边界分隔。
2. block 可折叠（点击 chevron 收起输出）。

## 现状证据

- 数据模型：`crates/pureterm/src/output_blocks.rs:59-291`
- 写入：`iced_app.rs:5053,5118,5123`
- 仅 sidebar 消费：`sidebar/right/view_blocks.rs:12,82`
- shader 不读 block：`grep output_blocks` 在 `terminal_shader/` 0 命中

## 变更点

1. shader / primitive_prepare 读 active pane 的 `PaneBlockStore`，在 block 头部行叠加 badge 图元（复用圆角矩形 primitive）。
2. exit code 染色：exit=0 绿、非 0 红。
3. 折叠：block store 加 `collapsed: bool`；折叠时渲染跳过输出行（高度计算随之调整）。

## 优先级 / 范围

- **优先级**：P2（本轮产出 spec，实现视进度）
- **范围**：badge + exit 着色；折叠交互可后续
- **不做**：block 级独立滚动、block 级主题

## 验证 / Test Plan

手测：跑命令后主画面每块有 badge + exit 着色。
