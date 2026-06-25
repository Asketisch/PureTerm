# Jump Host（跳板机管理）

## 概述

支持通过跳板机（bastion/jump host）连接目标服务器。自动处理多跳 SSH 连接，用户只需配置跳板机链。

## 竞品参考

| 竞品 | 功能 | 代码路径 |
|------|------|----------|
| **Tabby** | 自动跳板机配置 | `tabby-ssh/src/` |

## PureTerm 现状

**未实现。** PureTerm 的 SSH 模块支持直接连接，不支持 `ProxyJump` 链。

## 优先级

**P3 — 兼容性/国际化**

跳板机在企业环境中常用，但可通过 `~/.ssh/config` 的 `ProxyJump` 配置替代。

## 工作量

**M（1 周）**

核心是 SSH 配置解析 + 多跳连接管理。

## 实现方向

1. 解析 `~/.ssh/config` 中的 `ProxyJump` / `ProxyCommand` 配置
2. Profile 扩展：`jump_hosts = ["bastion.example.com"]`
3. 连接逻辑：先连跳板机 → 建立 tunnel → 连目标服务器
4. 多跳支持：A → B → C 链式连接
5. UI：Profile 编辑器中添加"跳板机"字段
6. 复用 ControlMaster：跳板机连接也可复用
