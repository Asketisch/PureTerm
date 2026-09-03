//! GAP-7.7: 自定义恢复绑定。

mod types;
mod config;
mod cli;
mod tokens;
mod cold_resume;

pub(crate) use cli::dispatch;
pub(crate) use cold_resume::{cold_resume_argv, interpolate_shell};
pub(crate) use tokens::{
    approve_token, authorize, AuthDecision,
};

#[cfg(test)]
#[path = "resume_bindings/tests.rs"]
mod tests;
