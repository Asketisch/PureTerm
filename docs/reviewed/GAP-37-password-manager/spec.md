---
gap: GAP-37
title: Password Manager
category: 开发者工具
plan_type: integration
priority: P3
effort: M
status: done
closed: 2026-06-29
---

# Password Manager Integration（密码管理器集成）

## 概述

集成第三方密码管理器（1Password、LastPass、Keychain），在 SSH 认证时自动获取密码/密钥。支持 echo probe 检测可用密码源。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **参考实现 A** | 1Password, LastPass, Keychain, command-line adapters, echo probe | `sources/PasswordManager/` (OnePasswordDataSource, LastPassDataSource, KeychainPasswordDataSource, CommandLinePasswordDataSource, EchoProbe) |

## PureTerm 现状

**部分实现。** PureTerm 的 SSH 凭据加密（AES-256-GCM + Argon2id）和 `keyring` crate 集成提供了 OS 钥匙链支持，但没有 1Password/LastPass 等第三方集成。

## 优先级

**P3 — 兼容性/国际化**

密码管理器集成是便利性功能，OS 钥匙链已能满足大部分需求。

## 工作量

**M（1 周）**

需要：1Password CLI（`op`）、LastPass CLI（`lpass`）集成。

## 实现方向

1. 检测可用密码源：`op read` / `lpass show` / `security find-generic-password`
2. SSH 认证时：优先从密码管理器获取 → 回退到 keyring → 回退到交互输入
3. 配置：`ssh.password_manager = "1password" | "lastpass" | "keychain" | "auto"`
4. Echo probe：测试哪个密码源可用
