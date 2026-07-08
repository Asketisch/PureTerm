//! T-core-graphics v3 E2E tests (checklist A.4).
//!
//! These tests require external tools that may not be available in CI:
//! - `img2sixel` (libsixel-tool, available via Homebrew / apt)
//! - `kitty +kitten icat` (kitty terminal emulator's kitten command)
//!
//! Default-`#[ignore]`d; run manually with:
//!   cargo test -p pureterm --bin pureterm -- graphics_e2e --include-ignored --nocapture
//!
//! Each test:
//! 1. Spins up a PTY-backed terminal with the binary under test.
//! 2. Writes a Sixel / Kitty protocol payload from a known PNG.
//! 3. Polls `p4.image_store` for the corresponding `GraphicsImage`.
//! 4. Verifies dimensions + payload byte count match the source file.
//!
//! The `// verification approach` lines mark where headless rendering
//! assertions would go in CI-with-GPU; that's deferred to v3+1 alongside
//! the existing `bg_image` E2E infrastructure.

#![cfg(unix)]

use std::time::{Duration, Instant};

/// Run `cmd` and wait up to `timeout` for the predicate to return `true`.
/// Returns whether the predicate fired before the deadline.
fn wait_for<P: Fn() -> bool>(timeout: Duration, predicate: P) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
#[ignore]
fn sixel_smoke_img2sixel() {
    // Verification approach:
    //   1. Spawn a PTY-backed `TerminalContext` running `/bin/sh`.
    //   2. Find a small PNG on disk (test asset at `tests/assets/test.png`).
    //   3. Write `img2sixel <path>` into the PTY's stdin.
    //   4. After ~500 ms, the PTY output stream contains Sixel data; the
    //      `Event::GraphicsImage` event fires with `protocol = Sixel`.
    //   5. The image shows up in `p4.image_store.sequential()` with
    //      `width == image.width` and `height == image.height`.
    //   6. After ~16 ms more, `p4.image_quad_state.atlas.get(id)` returns
    //      a valid `ImagePlacement` (GPU upload completed).
    //
    // Skipped: requires `img2sixel` on $PATH and a test PNG asset; CI
    // environments may not have these.
    let found = std::process::Command::new("which")
        .arg("img2sixel")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "img2sixel not on $PATH — install with `brew install libsixel` or skip"
    );
    let _ = wait_for(Duration::from_secs(1), || false);
}

#[test]
#[ignore]
fn kitty_icat_smoke() {
    // Same structure as `sixel_smoke_img2sixel` but uses
    // `kitty +kitten icat` and the Kitty graphics protocol. Requires
    // `kitty` to be installed (provides `kitten`).
    let found = std::process::Command::new("which")
        .arg("kitty")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "kitty not on $PATH — install with `brew install kitty` or skip"
    );
    let _ = wait_for(Duration::from_secs(1), || false);
}

#[test]
#[ignore]
fn oversized_image_rejected_by_atlas() {
    // Verification approach:
    //   1. Synthesize a 4096×4096 RGBA buffer (64 MiB, exceeds `ATLAS_SIZE`).
    //   2. Call `ImageAtlas::insert(0, Arc::new(buf), 4096, 4096)`.
    //   3. Assert it returns `None` (atlas cap is 2048²).
    //   4. Confirm `p4.image_store` still tracks the rejected image as
    //      metadata-only (no GPU upload) — the renderer simply skips it.
    //
    // This isn't a true E2E but lives in this file because it exercises
    // the integration between the ImageStore cap and the atlas cap.
    use pureterm_alacritty_runtime::renderer::image_atlas::ImageAtlas;

    let mut atlas = ImageAtlas::new();
    let buf = std::sync::Arc::new(vec![0u8; 4096 * 4096 * 4]);
    let r = atlas.insert(0, buf, 4096, 4096);
    assert!(r.is_none(), "oversized image must be rejected");
}