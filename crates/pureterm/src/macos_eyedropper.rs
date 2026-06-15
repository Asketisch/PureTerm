//! macOS 屏幕吸管（Digital Color Meter 风格）：用 AppKit `NSColorSampler`
//! 弹出系统 loupe，用户从桌面任意位置取一个像素颜色，经 oneshot 回传 `#RRGGBB`。
//!
//! 设计：调用方（`iced_app` 的 `Message::Settings` 分支）在主线程调 `start_sample`，
//! 传入 `tokio::sync::oneshot::Sender<Option<String>>`；本函数同步建 sampler、装
//! ObjC block（捕获 sender），随即返回。`NSColorSampler` 在展示期间自我 retain
//! （见 `showSamplerWithSelectionHandler:` 文档：«The calling NSColorSampler
//! instance is retained until the sampling session is completed»），故这里丢弃
//! sampler 局部变量是安全的。用户点选/取消时，AppKit 在主线程回调 block，block
//! 把 hex（或取消时的 `None`）经 sender 发出。future 端只 `await` oneshot，不碰
//! AppKit，因此无论被哪条线程 poll 都安全。

use block2::RcBlock;
use objc2_app_kit::{NSColor, NSColorSampler, NSColorSpace};

/// 主线程调用：弹出系统吸管 loupe。完成后经 `sender` 回传 `Some("#RRGGBB")`；
/// 用户取消（Esc）回传 `None`。非主线程直接回传 `None`（不应发生）。
pub fn start_sample(sender: tokio::sync::oneshot::Sender<Option<String>>) {
    // NSColorSampler 的 handler 固定在主线程回调；调用本身也应来自主线程
    // （iced update 即主线程）。这里仅作防御性兜底。
    let sampler = NSColorSampler::new();

    // tokio oneshot 的 `send` 取得 `self`（FnOnce），但 block2 的 block 要求 `Fn`。
    // 把 sender 装进 `Mutex<Option<_>>`，闭包经 `&self` 锁 + `Option::take` 发送，
    // 于是闭包只需共享引用其捕获值 → 满足 `Fn`（handler 实际只回调一次）。
    let sender_cell = std::sync::Mutex::new(Some(sender));

    let block = RcBlock::new(move |color_ptr: *mut NSColor| {
        let hex = if color_ptr.is_null() {
            None
        } else {
            // SAFETY: AppKit 在 block 回调期间保证 color_ptr 指向有效的 NSColor。
            let color: &NSColor = unsafe { &*color_ptr };
            ns_color_to_hex(color)
        };
        if let Some(tx) = sender_cell.lock().ok().and_then(|mut g| g.take()) {
            let _ = tx.send(hex);
        }
    });

    // SAFETY: sampler 是新建的有效实例；block 是合法的 `Fn(*mut NSColor)` block，
    // 生命周期由 RcBlock（堆 block）保证，AppKit 会 retain 它直到会话结束。
    unsafe { sampler.showSamplerWithSelectionHandler(&block) };
}

/// 把一个 `NSColor`（可能位于 device/extended 色彩空间）归一到 sRGB 并输出
/// 大写 `#RRGGBB`。转换或读取失败返回 `None`。
fn ns_color_to_hex(color: &NSColor) -> Option<String> {
    let srgb = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    // redComponent 等返回 CGFloat（64-bit 上即 f64）；统一 `as f64` 兼容 32-bit。
    let to8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(format!(
        "#{:02X}{:02X}{:02X}",
        to8(srgb.redComponent() as f64),
        to8(srgb.greenComponent() as f64),
        to8(srgb.blueComponent() as f64),
    ))
}
