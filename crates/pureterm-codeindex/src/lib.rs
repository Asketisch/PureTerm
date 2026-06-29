//! GAP-03 MVP: Tree-sitter semantic chunking + SQLite FTS keyword search.
//!
//! Embeddings and Merkle-tree incremental sync are stubbed (`embedding` module).

mod chunk;
mod db;
mod index;
mod progress;

pub use index::{CodeIndex, FileContext, IndexStats, SearchHit, SymbolHit};
pub use progress::IndexProgress;
