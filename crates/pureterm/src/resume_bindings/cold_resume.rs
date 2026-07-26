use super::tokens::{authorize, AuthDecision};
use super::types::{interpolate, InterpolateVars};
use crate::app_config::{AppConfig, ResumeBinding};


/// Interpolate a binding's shell template for spawning. `session_id` is "" for
/// a manual `resume run` (no agent session); cold-resume passes the dead id.
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

/// GAP-7.7: pure core of `cold_resume_argv`. Given already-loaded `bindings`,
/// resolve the `auto=true` binding for `kind` and, if `authorized` approves it,
/// return the interpolated argv. `None` when no auto binding matches or it
/// matches but isn't authorized — cold-resume never prompts (the `restore()`
/// path has no Feed panel access); the user must `pureterm surface resume run`
/// once to mint the token.
///
/// `authorized` is injected so this stays pure & testable without the on-disk
/// token store; the public `cold_resume_argv` passes the real `authorize`.
pub(crate) fn cold_resume_argv_from(
    bindings: &[ResumeBinding],
    kind: &str,
    session_id: &str,
    cwd: &str,
    authorized: impl Fn(&ResumeBinding) -> AuthDecision,
) -> Option<Vec<String>> {
    let mut matches: Vec<ResumeBinding> = bindings.iter().filter(|b| b.auto && b.kind == kind).cloned().collect();
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

/// GAP-7.7: resolve the `auto=true` resume binding for `kind` and, if a valid
/// HMAC token authorizes it, return the interpolated argv to spawn. Returns
/// `None` when no auto binding matches, or when it matches but has no token
/// yet — cold-resume never prompts (the `restore()` path has no Feed panel
/// access); the user must `pureterm surface resume run` once to mint the token.
pub(crate) fn cold_resume_argv(kind: &str, session_id: &str, cwd: &str) -> Option<Vec<String>> {
    let bindings = AppConfig::load_resume_bindings(&AppConfig::config_path());
    cold_resume_argv_from(&bindings, kind, session_id, cwd, |b| {
        authorize(&b.kind, &b.checkpoint, &b.shell)
    })
}
