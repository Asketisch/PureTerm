//! GAP-03: shared code-index handle for MCP tools (GUI status-bar wiring deferred).

use std::path::PathBuf;
use std::sync::OnceLock;

use parking_lot::Mutex;
use pureterm_codeindex::CodeIndex;

static INDEX: OnceLock<Mutex<Option<CodeIndex>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<CodeIndex>> {
    INDEX.get_or_init(|| Mutex::new(None))
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
        idx.index_workspace().map_err(|e| e.to_string())?;
        *guard = Some(idx);
    }
    f(guard.as_ref().expect("initialized above"))
}

/// Status-bar stub: indexing progress when MCP/index runs in-process (usually None in GUI).
pub fn indexing_percent() -> Option<u8> {
    slot()
        .try_lock()
        .and_then(|g| g.as_ref().and_then(|idx| idx.progress.percent()))
}
