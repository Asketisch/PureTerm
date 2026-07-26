---
gap: GAP-03
title: Codebase Indexing
category: AI Agent
plan_type: feature
priority: P1
effort: L
status: done
closed: 2026-06-28
---

# Codebase Indexing（代码库索引）

## 概述

对项目代码进行语义索引（Tree-sitter AST chunking + 嵌入向量），使 AI agent 能高效理解代码结构、搜索符号、回答代码相关问题。支持增量同步（Merkle tree）。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | Tree-sitter 语义 chunking + Merkle tree 增量同步 + 嵌入向量，跨 session/config 搜索 | `crates/ai/src/index/full_source_code_embedding/` |
| **cmux** | 全局搜索 ripgrep + 搜索索引（FileSearchRipgrepParser），非语义级别 | `Sources/` (搜索索引相关) |
| **参考实现 B** | Open Quickly 自建索引（FileSearchRipgrepParser），关键词级别 | `sources/` (搜索索引相关) |

## PureTerm 现状

**MVP 已交付（2026-06-28）。** 新增 `crates/pureterm-codeindex/`：Tree-sitter chunking（Rust/Python/JS）、SQLite + FTS5 关键词搜索、MCP 工具 `search_code` / `get_file_context` / `find_symbol`。嵌入向量与 Merkle 增量同步为 TODO。

## 优先级

**P1 — AI 差异化能力**

Codebase Indexing 是 AI agent 深度理解代码的基础，业界将其作为核心能力。对 PureTerm 而言，这是从"终端工具"升级为"AI 开发环境"的关键一步。

## 工作量

**L（1.5-2 周）**

需要：Tree-sitter 集成、chunking 策略、嵌入向量生成（需 LLM API）、Merkle tree 增量同步、存储层。

## 实现方向

1. 新增 `crates/pureterm-codeindex/` crate
2. Tree-sitter 解析源码为 AST → chunk（函数/类/模块级别）
3. 嵌入向量：调用 OpenAI `text-embedding-3-small` 或本地模型
4. 存储：SQLite + 向量索引（`sqlite-vec` 或 `surrealdb`）
5. Merkle tree 跟踪文件变更，仅重新索引变更文件
6. 暴露为 MCP tool：`search_code(query)`, `get_file_context(path)`, `find_symbol(name)`
7. 后台索引，UI 显示进度（状态栏）
