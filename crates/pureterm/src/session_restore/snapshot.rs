//! 快照写盘 —— `save_atomic_to`(原子写)+ `save_atomic`(默认路径封装) +
//! `migrate_v1_to_v2_in`(迁移核心,被根调用)。

use std::path::Path;

use super::{LayoutSnapshot, LegacyV1Snapshot, state_path, wrap_v1_into_v2};

#[cfg(unix)]
pub fn save_atomic_to(snapshot: &LayoutSnapshot, path: &std::path::Path) -> bool {
    use std::io::Write;

    let Some(dir) = path.parent() else {
        log::warn!("session_restore: snapshot path has no parent");
        return false;
    };
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!("session_restore: create_dir_all failed: {e}");
        return false;
    }
    let json = match serde_json::to_string_pretty(snapshot) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("session_restore: serialize failed: {e}");
            return false;
        }
    };
    let mut tmp = match tempfile::NamedTempFile::new_in(dir) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("session_restore: temp file failed: {e}");
            return false;
        }
    };
    if let Err(e) = tmp.write_all(json.as_bytes()) {
        log::warn!("session_restore: write failed: {e}");
        return false;
    }
    if let Err(e) = tmp.persist(path) {
        log::warn!("session_restore: persist failed: {e}");
        return false;
    }
    true
}

/// Atomically write the snapshot to the default state path.
/// Thin wrapper over [`save_atomic_to`] (best-effort: ignores the result).
#[cfg(unix)]
pub fn save_atomic(snapshot: &LayoutSnapshot) {
    let _ = save_atomic_to(snapshot, &state_path());
}

/// v1→v2 迁移核心(路径参数化,便于 tempdir 单测)。**先持久化 v2,写成功后才把
/// 旧 v1 改名 `.bak`**——确保写盘失败时不丢 v1 数据(下次启动 `load()` 重跑迁移
/// 仍能读到旧 v1)。旧文件改名 `.bak`(保留,永不 unlink,便于回退/排查)。
#[cfg(unix)]
pub(super) fn migrate_v1_to_v2_in(
    legacy_path: &Path,
    target: &Path,
) -> Option<LayoutSnapshot> {
    let data = std::fs::read(legacy_path).ok()?;
    let v1 = match serde_json::from_slice::<LegacyV1Snapshot>(&data) {
        Ok(s) if s.version == 1 => s,
        Ok(other) => {
            log::warn!(
                "session_restore: legacy version {} (expected 1), skipping",
                other.version
            );
            return None;
        }
        Err(e) => {
            log::warn!("session_restore: legacy v1 parse failed: {e}");
            return None;
        }
    };
    let v2 = wrap_v1_into_v2(v1.tabs, v1.active_tab, v1.viewport);
    // 先持久化 v2(原子写);只有写成功才把旧 v1 改名 `.bak`,避免写失败丢 v1。
    if save_atomic_to(&v2, target) {
        let bak = legacy_path.with_extension("json.bak");
        if let Err(e) = std::fs::rename(legacy_path, &bak) {
            log::warn!("session_restore: rename legacy → .bak failed: {e}");
        }
        log::info!("session_restore: migrated v1 gui-sessions.json → v2 workspaces.json");
    } else {
        log::warn!(
            "session_restore: v2 persist failed; keeping legacy gui-sessions.json for retry"
        );
    }
    Some(v2)
}
