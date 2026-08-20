//! GAP-35:基于 JSON-RPC stdio 的极简 LSP 客户端(rust-analyzer MVP)。

mod client;
mod diagnostic;
mod transport;

pub use client::{LspClient, LspClientConfig};
pub use diagnostic::{Diagnostic, DiagnosticSeverity, DiagnosticStore, DiagnosticSummary, language_id};
