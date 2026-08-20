---
gap: GAP-23
title: 标签栏
category: UI/外壳
plan_type: feature
priority: P4
effort: M
status: done
closed: 2026-06-29
---

# Tab Bar（标签栏）

## 概述

在窗口顶部显示可视化的 Tab 列表，支持 Tab 重命名、拖拽排序、右键菜单（关闭/复制/移动）、Tab 图标（进程图标）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | `TabBarControlView`（可自定义外观、拖拽排序、按钮关闭） | `sources/TerminalView/` |
| **参考实现 B** | 完整 Tab Bar（支持任意边停靠） | — |
| **参考实现 C** | 完整 Tab 支持（Cmd+T/W/1-9） | — |

## PureTerm 现状

**部分实现。** PureTerm 的 Iced UI 有 Tab 管理功能，但通过命令面板操作，没有可视化的 Tab Bar widget。

## 优先级

**P3 — 兼容性/国际化**

Tab Bar 是终端的基本 UI 组件，但 PureTerm 的命令面板方式也能满足需求。

## 工作量

**M（1 周）**

需要：Tab Bar widget、拖拽排序、右键菜单。

## 实现方向

1. 新增 `TabBar` iced widget（水平排列的 tab 按钮）
2. 每个 tab 显示：标题、关闭按钮、进程图标
3. 拖拽排序：`drag` + `drop` 事件
4. 右键菜单：关闭/关闭其他/复制/移动到窗口
5. 双击重命名
6. 配置：`tab_bar.position = "top" | "bottom" | "hidden"`
