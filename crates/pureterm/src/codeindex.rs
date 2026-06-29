//! GAP-03: shared code-index handle for MCP tools + status-bar progress wiring.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use pureterm_codeindex::{CodeIndex, IndexProgress};

static INDEX: OnceLock<Mutex<Option<CodeIndex>>> = OnceLock::new();
/// Shared progress handle, published before a scan starts. Readable without the index
/// lock (which the scan holds), so the GUI status bar can observe indexing progress.
static PROGRESS: OnceLock<Mutex<Option<Arc<IndexProgress>>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<CodeIndex>> {
    INDEX.get_or_init(|| Mutex::new(None))
}

fn progress_slot() -> &'static Mutex<Option<Arc<IndexProgress>>> {
    PROGRESS.get_or_init(|| Mutex::new(None))
}

/// Workspace root: `PURETERM_CODEINDEX_ROOT` or current directory.
pub fn workspace_root() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("PURETERM_CODEINDEX_ROOT") {
        return Ok(PathBuf::from(p));
    }
    std::env::current_dir().map_err(|e| e.to_string())
}

/// Open the index, run an initial workspace scan on first use.
pub fn with_index<T>(f: impl FnOnce(&CodeIndex) -> Result<T, String>) -> Result<T, String> {
    let mut guard = slot().lock();
    if guard.is_none() {
        let root = workspace_root()?;
        let idx = CodeIndex::open(&root).map_err(|e| e.to_string())?;
        // Publish the shared progress handle BEFORE the (potentially long) scan so
        // `indexing_percent()` can observe it without acquiring the index lock —
        // previously `try_lock()` on the index always failed mid-scan, so progress was
        // structurally never reported. (GAP-03 B3 fix.)
        *progress_slot().lock() = Some(idx.progress.clone());
        idx.index_workspace().map_err(|e| e.to_string())?;
        *guard = Some(idx);
    }
    f(guard.as_ref().expect("initialized above"))
}

/// Indexing progress (0–100) when a scan is in flight, else `None`. Reads the shared
/// atomic handle, so it works even while `with_index` holds the index lock mid-scan.
pub fn indexing_percent() -> Option<u8> {
    progress_slot().lock().as_ref().and_then(|p| p.percent())
}
