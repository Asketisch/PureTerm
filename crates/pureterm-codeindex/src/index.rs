//! Workspace indexer + query API.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use walkdir::WalkDir;

use crate::chunk::{chunk_source, SourceLanguage};
use crate::db::{file_hash, init_schema, replace_file_chunks};
use crate::progress::IndexProgress;

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("path not in workspace: {0}")]
    PathOutsideRoot(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchHit {
    pub file_path: String,
    pub symbol_name: Option<String>,
    pub symbol_kind: String,
    pub start_line: u32,
    pub end_line: u32,
    pub snippet: String,
    pub score: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SymbolHit {
    pub file_path: String,
    pub symbol_name: String,
    pub symbol_kind: String,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileContext {
    pub file_path: String,
    pub language: String,
    pub chunks: Vec<FileChunk>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileChunk {
    pub symbol_name: Option<String>,
    pub symbol_kind: String,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IndexStats {
    pub files_scanned: u32,
    pub files_indexed: u32,
    pub files_skipped: u32,
    pub chunks_written: u32,
}

pub struct CodeIndex {
    root: PathBuf,
    db: Connection,
    pub progress: IndexProgress,
}

impl CodeIndex {
    /// Open (or create) an index database for `root`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, IndexError> {
        let root = root.as_ref().canonicalize()?;
        let db_path = db_path_for_root(&root)?;
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let db = Connection::open(&db_path)?;
        init_schema(&db)?;
        Ok(Self {
            root,
            db,
            progress: IndexProgress::default(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Full workspace scan. Skips unchanged files by content hash.
    /// TODO(GAP-03): Merkle tree incremental sync.
    pub fn index_workspace(&self) -> Result<IndexStats, IndexError> {
        let files: Vec<PathBuf> = WalkDir::new(&self.root)
            .into_iter()
            .filter_entry(|e| !e.file_type().is_dir() || e.depth() == 0 || !should_skip_dir(e.path()))
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| SourceLanguage::from_path(e.path()).is_some())
            .map(|e| e.into_path())
            .collect();

        self.progress.reset(files.len() as u32);
        let mut stats = IndexStats::default();
        stats.files_scanned = files.len() as u32;

        for path in files {
            match self.index_file(&path) {
                Ok(IndexFileResult::Indexed { chunks }) => {
                    stats.files_indexed += 1;
                    stats.chunks_written += chunks;
                }
                Ok(IndexFileResult::Skipped) => stats.files_skipped += 1,
                Err(_e) => {
                    stats.files_skipped += 1;
                }
            }
            self.progress.inc();
        }
        self.progress.finish();
        Ok(stats)
    }

    fn index_file(&self, abs_path: &Path) -> Result<IndexFileResult, IndexError> {
        let rel = rel_path(&self.root, abs_path)?;
        let lang = SourceLanguage::from_path(abs_path).expect("filtered");
        let source = fs::read_to_string(abs_path)?;
        let hash = content_hash(&source);
        if let Some(existing) = file_hash(&self.db, &rel)? {
            if existing == hash {
                return Ok(IndexFileResult::Skipped);
            }
        }
        let mtime = abs_path
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let chunks = chunk_source(lang, &source);
        let n = chunks.len() as u32;
        replace_file_chunks(
            &self.db,
            &rel,
            &hash,
            mtime,
            language_label(lang),
            &chunks,
        )?;
        Ok(IndexFileResult::Indexed { chunks: n })
    }

    /// Keyword search via SQLite FTS5. Embedding/semantic search is TODO.
    pub fn search_code(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, IndexError> {
        let q = fts_query(query);
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.db.prepare(
            "SELECT file_path, symbol_name, symbol_kind, start_line, end_line,
                    snippet(chunks_fts, 1, '…', '…', '…', 20) AS snip,
                    bm25(chunks_fts) AS score
             FROM chunks_fts
             WHERE chunks_fts MATCH ?1
             ORDER BY score
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![q, limit as i64], |row| {
            Ok(SearchHit {
                file_path: row.get(0)?,
                symbol_name: row.get(1)?,
                symbol_kind: row.get(2)?,
                start_line: row.get::<_, i64>(3)? as u32,
                end_line: row.get::<_, i64>(4)? as u32,
                snippet: row.get(5)?,
                score: row.get(6)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn find_symbol(&self, name: &str, limit: usize) -> Result<Vec<SymbolHit>, IndexError> {
        let pattern = format!("%{name}%");
        let mut stmt = self.db.prepare(
            "SELECT file_path, symbol_name, symbol_kind, start_line, end_line
             FROM chunks
             WHERE symbol_name LIKE ?1 ESCAPE '\\'
             ORDER BY file_path, start_line
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit as i64], |row| {
            let sym: Option<String> = row.get(1)?;
            Ok(SymbolHit {
                file_path: row.get(0)?,
                symbol_name: sym.unwrap_or_default(),
                symbol_kind: row.get(2)?,
                start_line: row.get::<_, i64>(3)? as u32,
                end_line: row.get::<_, i64>(4)? as u32,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_file_context(&self, path: &str) -> Result<FileContext, IndexError> {
        let rel = normalize_requested_path(&self.root, path)?;
        let language: String = match self.db.query_row(
            "SELECT language FROM files WHERE path = ?1",
            params![rel],
            |row| row.get(0),
        ) {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => "unknown".into(),
            Err(e) => return Err(e.into()),
        };

        let mut stmt = self.db.prepare(
            "SELECT symbol_name, symbol_kind, start_line, end_line, content
             FROM chunks WHERE file_path = ?1 ORDER BY start_line",
        )?;
        let chunks: Vec<FileChunk> = stmt
            .query_map(params![rel], |row| {
                Ok(FileChunk {
                    symbol_name: row.get(0)?,
                    symbol_kind: row.get(1)?,
                    start_line: row.get::<_, i64>(2)? as u32,
                    end_line: row.get::<_, i64>(3)? as u32,
                    content: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(FileContext {
            file_path: rel,
            language,
            chunks,
        })
    }
}

enum IndexFileResult {
    Indexed { chunks: u32 },
    Skipped,
}

fn db_path_for_root(root: &Path) -> Result<PathBuf, IndexError> {
    let hash = content_hash(root.to_string_lossy().as_ref());
    let base = dirs_fallback_cache()?.join("pureterm").join("codeindex");
    Ok(base.join(format!("{hash}.sqlite")))
}

fn dirs_fallback_cache() -> Result<PathBuf, IndexError> {
    if let Some(p) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(p));
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return Ok(PathBuf::from(home).join("Library/Caches"));
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".cache"));
    }
    Ok(PathBuf::from("/tmp"))
}

fn content_hash(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

fn language_label(lang: SourceLanguage) -> &'static str {
    match lang {
        SourceLanguage::Rust => "rust",
        SourceLanguage::Python => "python",
        SourceLanguage::JavaScript => "javascript",
    }
}

fn rel_path(root: &Path, abs: &Path) -> Result<String, IndexError> {
    abs.strip_prefix(root)
        .map_err(|_| IndexError::PathOutsideRoot(abs.display().to_string()))
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

fn normalize_requested_path(root: &Path, path: &str) -> Result<String, IndexError> {
    let p = Path::new(path);
    if p.is_absolute() {
        rel_path(root, &p.canonicalize().map_err(IndexError::from)?)
    } else {
        Ok(path.trim_start_matches("./").replace('\\', "/"))
    }
}

fn should_skip_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                ".git" | "target" | "node_modules" | ".cargo" | "dist" | "build" | ".pureterm"
            ) || name.starts_with('.')
        })
}

/// Escape user tokens for FTS5 prefix query: `foo bar` → `"foo"* OR "bar"*`.
fn fts_query(raw: &str) -> String {
    raw.split_whitespace()
        .filter_map(|t| {
            let clean: String = t.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
            if clean.is_empty() {
                None
            } else {
                Some(format!("\"{clean}\"*"))
            }
        })
        .collect::<Vec<_>>()
        .join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_sample(root: &Path) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn alpha() {}\npub fn beta() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("helper.py"),
            "def greet():\n    return 'hi'\n",
        )
        .unwrap();
        fs::write(
            root.join("app.js"),
            "function run() { console.log('ok'); }\n",
        )
        .unwrap();
    }

    #[test]
    fn index_and_search_keywords() {
        let tmp = tempfile::tempdir().unwrap();
        write_sample(tmp.path());
        let idx = CodeIndex::open(tmp.path()).unwrap();
        let stats = idx.index_workspace().unwrap();
        assert!(stats.files_indexed >= 3, "{stats:?}");
        assert!(stats.chunks_written >= 3);

        let hits = idx.search_code("alpha", 10).unwrap();
        assert!(hits.iter().any(|h| h.file_path.contains("lib.rs")));

        let syms = idx.find_symbol("greet", 10).unwrap();
        assert_eq!(syms[0].symbol_name, "greet");

        let ctx = idx.get_file_context("src/lib.rs").unwrap();
        assert_eq!(ctx.language, "rust");
        assert!(ctx.chunks.len() >= 2);

        // Second index should skip unchanged files.
        let stats2 = idx.index_workspace().unwrap();
        assert_eq!(stats2.files_skipped, stats2.files_scanned);
    }

    #[test]
    fn fts_query_builder() {
        assert_eq!(fts_query("hello world"), "\"hello\"* OR \"world\"*");
        assert_eq!(fts_query("   "), "");
    }
}
