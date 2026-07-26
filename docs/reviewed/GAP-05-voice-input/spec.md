---
gap: GAP-05
title: Voice Input
category: AI Agent
plan_type: feature
priority: P4
effort: L
status: done
closed: 2026-06-29
---

# Voice Input（语音输入）

## 概述

通过麦克风采集语音，实时转录为文本并输入到终端。支持编程专用词汇识别，可与 AI Chat 联动实现语音对话。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | cpal 音频采集 + 转录 API，编辑器集成 | `crates/voice_input/src/`, `app/src/voice/` |
| **参考实现 B** | 无 | — |
| **参考实现 C** | 无 | — |

## PureTerm 现状

**未实现。**

## 优先级

**P4 — 视觉/分发**

语音输入是锦上添花功能，大部分用户习惯键盘输入。业界将其作为核心能力。

## 工作量

**L（1.5-2 周）**

需要：音频采集（`cpal` crate）、语音转录（OpenAI Whisper API 或本地模型）、编程词汇热词。

## 实现方向

1. 新增 `crates/pureterm-voice/` crate
2. 音频采集：`cpal` crate 跨平台录音
3. 转录：OpenAI Whisper API（流式）或本地 `whisper.cpp`
4. 热词：编程关键词（function names, commands）提升识别率
5. 快捷键：`Cmd+Shift+V` 按住说话，松开转录
6. 可选：与 AI Chat Panel 联动，实现语音对话
