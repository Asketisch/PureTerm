//! GAP-04 MVP:桌面截图(macOS `screencapture`)与输入注入桩。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 全屏捕获的原始 PNG 字节。
pub fn capture_screen_png() -> Result<Vec<u8>, String> {
    #[cfg(target_os = "macos")]
    {
        let tmp = temp_png_path();
        // `screencapture` 把目标路径作为 CLI 参数接收,因此路径必须
        // 是 UTF-8。非 UTF-8 的 $TMPDIR 不受支持——直接失败,而不是
        // 回退到共享的硬编码路径(旧的 `unwrap_or` 连 PID 都丢掉了,
        // 会跨实例冲突,还提供一个静态符号链接目标)。
        let path_str = tmp
            .to_str()
            .ok_or_else(|| "screenshot: temp dir path is not UTF-8 (GAP-04)".to_string())?;
        let status = Command::new("screencapture")
            .args(["-x", "-t", "png", path_str])
            .status()
            .map_err(|e| format!("screencapture spawn failed: {e}"))?;
        if !status.success() {
            // screencapture 可能写出了不完整的文件——尽力
            // 清理,然后在不读取坏字节的前提下暴露失败。
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("screencapture exited with {status}"));
        }
        read_and_cleanup(&tmp)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("desktop screenshot: macOS-only in MVP (GAP-04)".into())
    }
}

/// 读取捕获的 PNG,然后**总是**删除临时文件——成功时、
/// 读取失败时、以及文件缺失时皆然。捕获契约是:
/// `screencapture` 创建了该文件,每条路径上的清理都归我们;
/// 旧版本在 `read` 失败时会泄漏它(`?` 跳过了清理)。
/// 空捕获(screencapture 没写出可用内容)返回 `Err`。
fn read_and_cleanup(path: &Path) -> Result<Vec<u8>, String> {
    let read = std::fs::read(path);
    // 每条路径上无条件的尽力清理——绝不泄漏临时 PNG。
    let _ = std::fs::remove_file(path);
    let bytes = read.map_err(|e| format!("read screenshot: {e}"))?;
    if bytes.is_empty() {
        return Err("screenshot empty".into());
    }
    Ok(bytes)
}

/// 鼠标点击——MVP 桩(返回结构化的未实现错误)。
pub fn click(x: i32, y: i32) -> Result<(), String> {
    let _ = (x, y);
    Err(format!(
        "desktop_click({x},{y}): input injection stub — not yet implemented (GAP-04 MVP)"
    ))
}

/// 向聚焦的应用输入文本——MVP 桩。
pub fn type_text(text: &str) -> Result<(), String> {
    let _ = text;
    Err("desktop_type: input injection stub — not yet implemented (GAP-04 MVP)".into())
}

/// 按名称按键——MVP 桩。
pub fn key_press(key: &str) -> Result<(), String> {
    let _ = key;
    Err(format!(
        "desktop_key_press({key:?}): input injection stub — not yet implemented (GAP-04 MVP)"
    ))
}

/// 每次捕获一个唯一且难预测的临时路径,以缓解经典的
/// `/tmp` 符号链接竞态(仅 PID 的名字可被猜到;旧的静态
/// 回退更糟)。PID + 高分辨率时间戳 + 进程内计数器
/// 使其保持 std-only(无需 `tempfile` 依赖)。在真正的多用户
/// 加固主机上的残余风险已记录为可接受的 MVP 限制。
fn temp_png_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("pureterm-screenshot-{pid}-{nanos}-{n}.png"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_stubs_return_errors() {
        assert!(click(10, 20).is_err());
        assert!(type_text("hello").is_err());
        assert!(key_press("Return").is_err());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn screenshot_non_macos_errors() {
        assert!(capture_screen_png().is_err());
    }

    #[test]
    fn read_and_cleanup_removes_file_on_success() {
        let p = std::env::temp_dir().join("pureterm-test-cleanup-ok.png");
        std::fs::write(&p, b"fake-png-bytes").unwrap();
        let bytes = read_and_cleanup(&p).unwrap();
        assert_eq!(bytes, b"fake-png-bytes");
        assert!(!p.exists(), "temp file must be removed on success");
    }

    #[test]
    fn read_and_cleanup_removes_file_and_errors_when_empty() {
        let p = std::env::temp_dir().join("pureterm-test-cleanup-empty.png");
        std::fs::write(&p, b"").unwrap();
        let err = read_and_cleanup(&p).unwrap_err();
        assert!(err.contains("empty"), "unexpected err: {err}");
        assert!(!p.exists(), "temp file must be removed even when empty");
    }

    #[test]
    fn read_and_cleanup_errors_without_panic_when_missing() {
        let p = std::env::temp_dir().join("pureterm-test-cleanup-missing.png");
        let _ = std::fs::remove_file(&p); // ensure absent
        assert!(read_and_cleanup(&p).is_err());
    }

    #[test]
    fn temp_png_paths_are_distinct() {
        // 连续两次的路径必须不同——PID+纳秒+计数器命名正是
        // 符号链接竞态的缓解手段;名字一旦碰撞即失去防护。
        let a = temp_png_path();
        let b = temp_png_path();
        assert_ne!(a, b);
    }
}
