# Print Support（打印支持）

## 概述

支持将终端内容打印到纸张或 PDF。包含打印预览、页面设置、页眉/页脚。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **iTerm2** | Terminal content printing with accessory view | `sources/Printing/` (iTermPrintAccessoryViewController) |

## PureTerm 现状

**未实现。**

## 优先级

**P4 — 视觉/分发**

打印功能在终端使用场景中极少用到。

## 工作量

**S（2-3 天）**

需要：打印 API 集成。

## 实现方向

1. macOS：`NSPrintOperation` + `NSPrintInfo`
2. Linux/Windows：导出为 PDF（`print` crate）
3. 快捷键：`Cmd+P` 打印当前 pane
4. 打印预览：显示将打印的内容
5. 页眉/页脚：可配置（文件名、日期、页码）
