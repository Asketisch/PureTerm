//! GAP-7.7:自定义 resume 绑定。
//!
//! *resume 绑定* 是一条命名的、带模板的 shell 命令,以
//! `(kind, checkpoint)` 为键,在冷恢复时重连到外部持久会话
//! (tmux/screen/zellij),替代或补充默认的
//! `claude --resume <id>`。绑定在 `pureterm.toml` 中声明:
//!
//! ```toml
//! [[resume.bindings]]
//! kind = "tmux"
//! checkpoint = "work"
//! shell = ["tmux", "attach", "-t", "{checkpoint}"]
//! auto = true
//! ```
//!
//! 本模块负责:轻量占位符插值、配置的
//! 增删改查(`set_binding` / `remove_binding` / `list_bindings`),供
//! `pureterm surface resume set|list|remove` CLI 使用;以及(M2)HMAC 签名
//! 的持久审批 token、`run` 执行路径与冷恢复
//! 集成。仅 Unix——执行经 `puretermd` 完成。


use hmac::Hmac;
use sha2::Sha256;


/// 用于 token 计算的 HMAC-SHA256 类型别名。
type HmacSha256 = Hmac<Sha256>;

/// 提供给 [`interpolate`] 的占位符值。四个值全部用
/// 简单的 `String::replace` 替换(无模板引擎)——与
/// `session_restore.rs:378` 处的 `{session_id}` 插值保持一致。
#[derive(Debug, Clone, Copy)]
pub(crate) struct InterpolateVars<'a> {
    pub session_id: &'a str,
    pub checkpoint: &'a str,
    pub kind: &'a str,
    pub cwd: &'a str,
}

/// 在每个 argv 元素中替换 `{session_id}` / `{checkpoint}` / `{kind}` / `{cwd}`。
/// 不含占位符的元素原样返回。
pub(crate) fn interpolate(argv: &[String], vars: &InterpolateVars<'_>) -> Vec<String> {
    argv.iter()
        .map(|s| {
            s.replace("{session_id}", vars.session_id)
                .replace("{checkpoint}", vars.checkpoint)
                .replace("{kind}", vars.kind)
                .replace("{cwd}", vars.cwd)
        })
        .collect()
}
