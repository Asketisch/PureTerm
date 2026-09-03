use super::tokens::{authorize, AuthDecision};
use super::types::{interpolate, InterpolateVars};
use crate::app_config::{AppConfig, ResumeBinding};


/// 插值绑定的 shell 模板以便启动。手动 `resume run` 时 `session_id` 为 ""
/// (没有 agent 会话);冷恢复则传入已死亡的会话 id。
pub(crate) fn interpolate_shell(
    shell: &[String],
    kind: &str,
    checkpoint: &str,
    session_id: &str,
    cwd: &str,
) -> Vec<String> {
    interpolate(
        shell,
        &InterpolateVars {
            session_id,
            checkpoint,
            kind,
            cwd,
        },
    )
}

/// GAP-7.7:`cold_resume_argv` 的纯核心。给定已加载的 `bindings`,
/// 解析 `kind` 的 `auto=true` 绑定;若 `authorized` 批准,
/// 返回插值后的 argv。当没有匹配的自动绑定、或
/// 匹配但未获授权时返回 `None`——冷恢复从不弹提示(`restore()`
/// 路径无法访问 Feed 面板);用户必须执行一次 `pureterm surface resume run`
/// 来铸造 token。
///
/// `authorized` 以注入方式传入,使本函数无需磁盘 token 存储
/// 即可保持纯函数、可测试;公开的 `cold_resume_argv` 传入真实的 `authorize`。
pub(crate) fn cold_resume_argv_from(
    bindings: &[ResumeBinding],
    kind: &str,
    session_id: &str,
    cwd: &str,
    authorized: impl Fn(&ResumeBinding) -> AuthDecision,
) -> Option<Vec<String>> {
    let matches: Vec<ResumeBinding> = bindings.iter().filter(|b| b.auto && b.kind == kind).cloned().collect();
    if matches.len() > 1 {
        log::warn!(
            "cold_resume_argv: multiple auto bindings for kind `{kind}`; using the first"
        );
    }
    let b = matches.first()?;
    if !matches!(authorized(b), AuthDecision::Approved) {
        log::info!(
            "cold_resume_argv: kind `{kind}` auto binding has no token; skipping \
             (run `pureterm surface resume run --kind {kind} --checkpoint {}` to authorize)",
            b.checkpoint
        );
        return None;
    }
    Some(interpolate_shell(&b.shell, &b.kind, &b.checkpoint, session_id, cwd))
}

/// GAP-7.7:解析 `kind` 的 `auto=true` resume 绑定;若有效的
/// HMAC token 授权,则返回要启动的插值 argv。
/// 当没有匹配的自动绑定、或匹配但尚无 token 时返回 `None`;
/// 冷恢复从不弹提示(`restore()` 路径无法访问 Feed 面板),
/// 用户必须执行一次 `pureterm surface resume run` 来铸造 token。
pub(crate) fn cold_resume_argv(kind: &str, session_id: &str, cwd: &str) -> Option<Vec<String>> {
    let bindings = AppConfig::load_resume_bindings(&AppConfig::config_path());
    cold_resume_argv_from(&bindings, kind, session_id, cwd, |b| {
        authorize(&b.kind, &b.checkpoint, &b.shell)
    })
}
