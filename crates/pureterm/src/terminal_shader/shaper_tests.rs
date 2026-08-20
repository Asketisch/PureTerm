//! T-core-shaper v2:渲染器侧字形整形接线的测试。
//!
//! 验证整形器返回的逐字形 `x_advance`
//! 确实驱动渲染器将施加的光标步进。

use pureterm_alacritty_runtime::shaper::{MonospaceShaper, Shaper};

#[test]
fn monospace_shaper_advances_one_cell_per_ascii() {
    let shaper = MonospaceShaper::new();
    let run = shaper.shape("abc", 7.0);
    assert_eq!(run.glyphs.len(), 3);
    for g in &run.glyphs {
        assert_eq!(g.x_advance, 7.0);
    }
}

#[test]
fn monospace_shaper_advances_two_cells_for_cjk() {
    let shaper = MonospaceShaper::new();
    let run = shaper.shape("你好", 7.0);
    assert_eq!(run.glyphs.len(), 2);
    for g in &run.glyphs {
        assert_eq!(g.x_advance, 14.0);
    }
}

#[test]
fn cluster_to_cell_offsets_account_for_wide_chars() {
    let shaper = MonospaceShaper::new();
    let run = shaper.shape("a你b", 7.0);
    // ASCII 占据格子 0,CJK 占据格子 1+2,ASCII 占据格子 3
    assert_eq!(run.cluster_to_cell, vec![0, 1, 3]);
}
