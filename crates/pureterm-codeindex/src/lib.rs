//! GAP-03 MVP:Tree-sitter 语义分块 + SQLite FTS 关键词搜索。
//!
//! 嵌入与 Merkle 树增量同步为占位(`embedding` 模块)。

mod chunk;
mod db;
mod index;
mod progress;

pub use index::{CodeIndex, FileContext, IndexStats, SearchHit, SymbolHit};
pub use progress::IndexProgress;
