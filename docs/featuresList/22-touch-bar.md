# Touch Bar（macOS 触控栏）

## 概述

在 MacBook Pro Touch Bar 上显示终端控件：Tab 列表、分屏按钮、常用命令快捷键、Vi 模式指示器。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | 完整 Touch Bar 支持（NSTouchBar，可自定义按钮） | `sources/TouchBar/` |
| **Tabby** | 无 | — |

## PureTerm 现状

**未实现。**

## 优先级

**P4 — 视觉/分发**

Apple 已弱化 Touch Bar（2021 年后 MacBook Pro 移除），优先级最低。

## 工作量

**M（1 周）**

需要：`objc2` 创建 `NSTouchBar`，注册 Touch Bar 物件。

## 实现方向

1. macOS：通过 `objc2` 创建 `NSTouchBar` + `NSTouchBarItem`
2. 固定按钮：Tab 列表、新建 Tab、分屏、Vi 模式切换
3. 动态按钮：根据当前 context 显示相关操作
4. 配置：`touchbar.enabled = true`, `touchbar.items = [...]`
