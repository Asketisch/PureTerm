//! GAP-35: Minimal LSP client over JSON-RPC stdio (rust-analyzer MVP).

mod client;
mod diagnostic;
mod transport;

pub use client::{LspClient, LspClientConfig};
pub use diagnostic::{Diagnostic, DiagnosticSeverity, DiagnosticStore, DiagnosticSummary, language_id};
