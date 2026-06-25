# Smart Selection Hover Preview（智能选择悬停预览）

## 概述

鼠标悬停在智能选择识别的 URL/路径/SSH URL 上时，显示 tooltip 预览：URL 显示页面标题 + favicon，路径显示文件信息。右键弹出动作菜单（打开/复制/编辑器打开）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | Semantic History：Cmd-click 打开文件在编辑器中（带行号），URL 点击打开浏览器 | `sources/SemanticHistory/` (iTermSemanticHistoryController, iTermURLActionFactory, iTermPathFinder) |
| **cmux** | 全局搜索 + 文件预览 | — |
| **Tabby** | URL 点击通过 @xterm/addon-web-links | — |

## PureTerm 现状

**部分实现。** PureTerm 的智能选择（`crates/pureterm/src/main.rs`）已实现 9 种规则检测（URL/SSH/邮箱/路径等），但**没有 hover preview**。

## 优先级

**P2 — 终端增强**

Hover Preview 是智能选择的自然延伸，提升用户交互体验。

## 工作量

**M（1 周）**

需要：hover 检测延迟、tooltip widget、异步数据获取（URL title）。

## 实现方向

1. 鼠标 hover 检测：200ms 延迟（避免闪烁）
2. URL preview：`reqwest` 抓取 HTML `<title>` + favicon
3. 路径 preview：`std::fs::metadata` + `ignore` crate（文件大小/类型/修改时间）
4. iced `Tooltip` widget 显示预览
5. 右键菜单：打开/复制/在编辑器中打开
6. 快捷键：`Cmd+Shift+O` 打开选中 URL/路径
