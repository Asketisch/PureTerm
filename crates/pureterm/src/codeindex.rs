//! GAP-03:为 MCP 工具与状态栏进度接线提供的共享代码索引句柄。

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use pureterm_codeindex::{CodeIndex, IndexProgress};

static INDEX: OnceLock<Mutex<Option<CodeIndex>>> = OnceLock::new();
/// 共享进度句柄,在扫描开始前发布。无需持有索引锁
///(扫描过程会持锁)即可读取,使 GUI 状态栏能观察索引进度。
static PROGRESS: OnceLock<Mutex<Option<Arc<IndexProgress>>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<CodeIndex>> {
    INDEX.get_or_init(|| Mutex::new(None))
}

fn progress_slot() -> &'static Mutex<Option<Arc<IndexProgress>>> {
    PROGRESS.get_or_init(|| Mutex::new(None))
}

/// 工作区根:`PURETERM_CODEINDEX_ROOT` 或当前目录。
pub fn workspace_root() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("PURETERM_CODEINDEX_ROOT") {
        return Ok(PathBuf::from(p));
    }
    std::env::current_dir().map_err(|e| e.to_string())
}

/// 打开索引,首次使用时执行一次初始工作区扫描。
pub fn with_index<T>(f: impl FnOnce(&CodeIndex) -> Result<T, String>) -> Result<T, String> {
    let mut guard = slot().lock();
    if guard.is_none() {
        let root = workspace_root()?;
        let idx = CodeIndex::open(&root).map_err(|e| e.to_string())?;
        // 在(可能耗时的)扫描开始前发布共享进度句柄,使
        // `indexing_percent()` 无需获取索引锁即可观察——
        // 此前 `try_lock()` 在扫描中总是失败,导致进度
        // 结构性从未上报。(GAP-03 B3 修复。)
        *progress_slot().lock() = Some(idx.progress.clone());
        idx.index_workspace().map_err(|e| e.to_string())?;
        *guard = Some(idx);
    }
    f(guard.as_ref().expect("initialized above"))
}

/// 扫描进行中的索引进度(0–100),否则为 `None`。读取共享
/// 原子句柄,因此在 `with_index` 持锁扫描期间也能工作。
pub fn indexing_percent() -> Option<u8> {
    progress_slot().lock().as_ref().and_then(|p| p.percent())
}
