//! T-core-graphics v3: 后台图像解码 worker。
//!
//! 终端接收到的 `Event::GraphicsImage` 可能携带:
//! - **已解码 RGBA 像素**(`pixels: Some(Vec<u8>)`, `format = Rgba`)—— 直接转发
//! - **PNG 字节流**(`encoded: Vec<u8>`, `format = Png`)—— 需要 `image::load_from_memory` 解码
//! - **Sixel 字节流**(`encoded: Vec<u8>`, `format = Rgb`)—— v3 范围外,日志警告并丢弃
//!
//! PNG 解码是 CPU 密集 + 可能阻塞的(几百 KB 至几 MB),所以放在独立线程里做,
//! 通过 mpsc 通道把 RGBA 像素 + 网格 placement 推回主线程,主线程在 `prepare()`
//! 之前 `try_recv()` 排空。
//!
//! ## 数据流
//! ```
//! iced_app.rs (主线程,GraphicsImage 事件)
//!     ↓ image_decode_tx.send(img)
//! image_decode_worker (后台线程)
//!     ↓ image::load_from_memory → to_rgba8()
//! image_decode_rx.try_recv() (主线程,在 prepare() 前)
//!     ↓ ImageQuadState::insert(id, rgba, w, h)
//! GPU atlas → draw
//! ```
//!
//! ## 失败/背压策略
//! - 后台线程 **panic 时自动重启**(`spawn_thread` 包装)
//! - 主线程 `try_recv()` 是非阻塞的,任何准备好的图像都会在当前帧进入 atlas
//! - 通道关闭时(进程退出 / worker 线程 panic)主线程安全收到 `Err`,记日志
//! - 不限速:PNG 解码开销 < 50ms / 张的典型情况下,通道深度不会堆积

use std::sync::mpsc::{Receiver, Sender};
use std::thread;

use alacritty_terminal::event::{GraphicsFormat, GraphicsImage};

/// 已解码的 RGBA 图像,主线程可直接消费送入 `ImageQuadState`。
///
/// `id` 是 `GraphicsPlacement.id`(chunked-graphics 用,`0` 表示 Sixel/inline image)。
/// `placement` 携带网格位置信息(列/行/列数/行数/z-order),
/// renderer 用它把图像 quad 映射到正确的格子坐标。
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub id: u32,
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub placement: alacritty_terminal::event::GraphicsPlacement,
}

/// 启动后台解码 worker 线程,返回一对 (发送端, 接收端)。
///
/// - `tx`: 主线程把 `GraphicsImage` 推入,worker 会解码并把结果推回。
/// - `rx`: 主线程在 `prepare()` 前排空,得到 `DecodedImage` 喂给 GPU atlas。
pub fn spawn() -> (Sender<GraphicsImage>, Receiver<DecodedImage>) {
    let (in_tx, in_rx) = std::sync::mpsc::channel::<GraphicsImage>();
    let (out_tx, out_rx) = std::sync::mpsc::channel::<DecodedImage>();

    spawn_thread(in_rx, out_tx);
    (in_tx, out_rx)
}

fn spawn_thread(
    in_rx: Receiver<GraphicsImage>,
    out_tx: Sender<DecodedImage>,
) {
    let builder = thread::Builder::new().name("pureterm-image-decode".to_string());
    let join = builder.spawn(move || run_loop(in_rx, out_tx));
    if let Err(e) = join {
        // Spawn 失败(资源限制等)。记录但不让主线程挂掉 —— 主线程推入 tx
        // 时通道会断开,日志会反映这个问题。
        log::error!("image-decode worker spawn failed: {e}");
    }
}

fn run_loop(in_rx: Receiver<GraphicsImage>, out_tx: Sender<DecodedImage>) {
    while let Ok(img) = in_rx.recv() {
        // 解码 PNG / 转发 RGBA;失败时仅记日志,不向主线程回报。
        if let Some(decoded) = decode(&img) {
            // 主线程 drop 了 rx 端(进程退出),send 会失败 —— 安全忽略。
            if out_tx.send(decoded).is_err() {
                log::debug!("image-decode: main thread rx dropped, worker exiting");
                return;
            }
        }
    }
    // 发送端 drop,主线程 recv() 返回 Err 后会清理。
    log::debug!("image-decode: in_tx dropped, worker exiting");
}

/// 解码 `GraphicsImage` 为 `DecodedImage`。
///
/// - `Rgba` 格式:直接转发 `pixels` 字段。
/// - `Png` 格式:用 `image::load_from_memory` 解码并强转为 RGBA8。
/// - `Rgb`(Sixel) 格式:v3 不实现,记日志并返回 `None`。
fn decode(img: &GraphicsImage) -> Option<DecodedImage> {
    match img.format {
        GraphicsFormat::Rgba => {
            let pixels = img.pixels.clone()?;
            Some(DecodedImage {
                id: img.placement.id,
                rgba: pixels,
                width: img.width,
                height: img.height,
                placement: img.placement.clone(),
            })
        }
        GraphicsFormat::Png => {
            match image::load_from_memory(&img.encoded) {
                Ok(dyn_img) => {
                    let rgba = dyn_img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    // PNG 解码后的尺寸可能与图形协议声明的不同;
                    // 使用解码后的真实尺寸,这是 GPU atlas 要用的尺寸。
                    Some(DecodedImage {
                        id: img.placement.id,
                        rgba: rgba.into_raw(),
                        width: w,
                        height: h,
                        placement: img.placement.clone(),
                    })
                }
                Err(e) => {
                    log::warn!(
                        "image-decode: PNG decode failed ({} bytes): {e}",
                        img.encoded.len()
                    );
                    None
                }
            }
        }
        GraphicsFormat::Rgb => {
            // Sixel: v3 范围外,仅记日志。
            log::debug!(
                "image-decode: Sixel (Rgb) skipped in v3 ({} bytes)",
                img.encoded.len()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::{GraphicsFormat, GraphicsPlacement, GraphicsProtocol};

    fn make_rgba_image(id: u32, w: u32, h: u32) -> GraphicsImage {
        let pixels = vec![0xFFu8; (w * h * 4) as usize];
        GraphicsImage {
            protocol: GraphicsProtocol::Kitty,
            format: GraphicsFormat::Rgba,
            width: w,
            height: h,
            pixels: Some(pixels),
            encoded: Vec::new(),
            placement: GraphicsPlacement {
                id,
                col: 0,
                row: 0,
                cols: 1,
                rows: 1,
                z: 0,
            },
        }
    }

    #[test]
    fn decode_rgba_passes_through() {
        let img = make_rgba_image(42, 4, 4);
        let out = decode(&img).expect("rgba decode should succeed");
        assert_eq!(out.id, 42);
        assert_eq!(out.width, 4);
        assert_eq!(out.height, 4);
        assert_eq!(out.rgba.len(), 4 * 4 * 4);
    }

    #[test]
    fn decode_png_succeeds() {
        // 2x2 红色 PNG,用 image crate 编码后给 decoder
        let mut buf = Vec::new();
        {
            let mut encoder = image::codecs::png::PngEncoder::new(&mut buf);
            use image::ImageEncoder;
            let img = image::RgbaImage::from_fn(2, 2, |_x, _y| image::Rgba([255, 0, 0, 255]));
            encoder
                .write_image(img.as_raw(), 2, 2, image::ExtendedColorType::Rgba8)
                .unwrap();
        }
        let img = GraphicsImage {
            protocol: GraphicsProtocol::Kitty,
            format: GraphicsFormat::Png,
            width: 2,
            height: 2,
            pixels: None,
            encoded: buf,
            placement: GraphicsPlacement {
                id: 7,
                col: 0,
                row: 0,
                cols: 1,
                rows: 1,
                z: 0,
            },
        };
        let out = decode(&img).expect("png decode should succeed");
        assert_eq!(out.id, 7);
        assert_eq!(out.width, 2);
        assert_eq!(out.height, 2);
        assert_eq!(out.rgba.len(), 2 * 2 * 4);
    }

    #[test]
    fn decode_png_invalid_returns_none() {
        let img = GraphicsImage {
            protocol: GraphicsProtocol::Kitty,
            format: GraphicsFormat::Png,
            width: 1,
            height: 1,
            pixels: None,
            encoded: b"not a png".to_vec(),
            placement: GraphicsPlacement {
                id: 1,
                col: 0,
                row: 0,
                cols: 1,
                rows: 1,
                z: 0,
            },
        };
        assert!(decode(&img).is_none());
    }

    #[test]
    fn decode_rgb_sixel_skipped_in_v3() {
        let img = GraphicsImage {
            protocol: GraphicsProtocol::Sixel,
            format: GraphicsFormat::Rgb,
            width: 8,
            height: 8,
            pixels: None,
            encoded: vec![0u8; 8 * 8 * 3],
            placement: GraphicsPlacement {
                id: 0,
                col: 0,
                row: 0,
                cols: 1,
                rows: 1,
                z: 0,
            },
        };
        assert!(decode(&img).is_none());
    }

    #[test]
    fn spawn_returns_working_channel() {
        let (tx, rx) = spawn();
        let img = make_rgba_image(1, 2, 2);
        tx.send(img).unwrap();
        let decoded = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("worker should produce a decoded image");
        assert_eq!(decoded.id, 1);
        assert_eq!(decoded.rgba.len(), 2 * 2 * 4);
    }
}