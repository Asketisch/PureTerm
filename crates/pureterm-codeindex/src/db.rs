//! SQLite schema + CRUD for indexed chunks.

use rusqlite::{params, Connection};

use crate::chunk::RawChunk;

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS files (
            path TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            mtime_secs INTEGER NOT NULL DEFAULT 0,
            language TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS chunks (
            id INTEGER PRIMARY KEY,
            file_path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
            symbol_name TEXT,
            symbol_kind TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL,
            content TEXT NOT NULL
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
            symbol_name,
            content,
            file_path UNINDEXED,
            symbol_kind UNINDEXED,
            start_line UNINDEXED,
            end_line UNINDEXED,
            content='chunks',
            content_rowid='id'
        );

        CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
            INSERT INTO chunks_fts(rowid, symbol_name, content, file_path, symbol_kind, start_line, end_line)
            VALUES (new.id, new.symbol_name, new.content, new.file_path, new.symbol_kind, new.start_line, new.end_line);
        END;

        CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
            INSERT INTO chunks_fts(chunks_fts, rowid, symbol_name, content, file_path, symbol_kind, start_line, end_line)
            VALUES ('delete', old.id, old.symbol_name, old.content, old.file_path, old.symbol_kind, old.start_line, old.end_line);
        END;

        CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
            INSERT INTO chunks_fts(chunks_fts, rowid, symbol_name, content, file_path, symbol_kind, start_line, end_line)
            VALUES ('delete', old.id, old.symbol_name, old.content, old.file_path, old.symbol_kind, old.start_line, old.end_line);
            INSERT INTO chunks_fts(rowid, symbol_name, content, file_path, symbol_kind, start_line, end_line)
            VALUES (new.id, new.symbol_name, new.content, new.file_path, new.symbol_kind, new.start_line, new.end_line);
        END;

        -- TODO(GAP-03): store embedding vectors (OpenAI text-embedding-3-small or local model).
        CREATE TABLE IF NOT EXISTS embeddings (
            chunk_id INTEGER PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
            embedding BLOB
        );

        CREATE INDEX IF NOT EXISTS idx_chunks_symbol ON chunks(symbol_name);
        CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_path);
        ",
    )
}

pub fn file_hash(conn: &Connection, path: &str) -> rusqlite::Result<Option<String>> {
    match conn.query_row(
        "SELECT content_hash FROM files WHERE path = ?1",
        params![path],
        |row| row.get(0),
    ) {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn replace_file_chunks(
    conn: &Connection,
    path: &str,
    content_hash: &str,
    mtime_secs: i64,
    language: &str,
    chunks: &[RawChunk],
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM chunks WHERE file_path = ?1", params![path])?;
    tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
    tx.execute(
        "INSERT INTO files (path, content_hash, mtime_secs, language) VALUES (?1, ?2, ?3, ?4)",
        params![path, content_hash, mtime_secs, language],
    )?;
    for c in chunks {
        tx.execute(
            "INSERT INTO chunks (file_path, symbol_name, symbol_kind, start_line, end_line, start_byte, end_byte, content)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                path,
                c.symbol_name,
                c.symbol_kind,
                c.start_line,
                c.end_line,
                c.start_byte,
                c.end_byte,
                c.content,
            ],
        )?;
    }
    tx.commit()
}
