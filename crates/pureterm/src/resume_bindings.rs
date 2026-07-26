//! GAP-7.7: Custom resume bindings.

mod types;
mod config;
mod cli;
mod tokens;
mod cold_resume;

pub(crate) use cli::dispatch;
pub(crate) use cold_resume::{cold_resume_argv, cold_resume_argv_from, interpolate_shell};
pub(crate) use config::{list_bindings, remove_binding, set_binding};
pub(crate) use tokens::{
    approve_token, authorize, authorize_with, compute_token, ensure_secret, ensure_secret_at,
    load_tokens_at, save_tokens_at, secret_path, tokens_path, AuthDecision,
};
pub(crate) use types::{interpolate, InterpolateVars};

#[cfg(test)]
#[path = "resume_bindings/tests.rs"]
mod tests;
