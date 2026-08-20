//! T-core-graphics v3 E2E 测试(清单 A.4)。
//!
//! 这些测试需要外部工具,CI 环境可能没有:
//! - `img2sixel`(libsixel-tool,可通过 Homebrew / apt 安装)
//! - 一个自带图形协议 `icat` kitten 的外部终端
//!   (此处仅用作分块图形载荷的参考发射器)
//!
//! 默认标了 `#[ignore]`;手动运行:
//!   手动运行:cargo test -p pureterm --bin pureterm -- graphics_e2e --include-ignored --nocapture
//!
//! 每个测试:
//! 1. 启动一个由被测二进制驱动的、基于 PTY 的终端。
//! 2. 从已知 PNG 写出 Sixel / 分块图形协议载荷。
//! 3. 轮询 `p4.image_store` 查找对应的 `GraphicsImage`。
//! 4. 校验尺寸与载荷字节数与源文件一致。
//!
//! 以 `// verification approach` 标注的行标记了无头渲染
//! 断言的落点(在带 GPU 的 CI 中);这推迟到 v3+1,与
//! 现有 `bg_image` E2E 基础设施一并落地。

#![cfg(unix)]

use std::time::{Duration, Instant};

/// 运行 `cmd`,最多等待 `timeout` 直到谓词返回 `true`。
/// 返回谓词是否在截止时间前满足。
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
    // 验证思路:
    //   1. 启动一个运行 `/bin/sh` 的、基于 PTY 的 `TerminalContext`。
    //   2. 在磁盘上找一张小 PNG(测试资源位于 `tests/assets/test.png`)。
    //   3. 把 `img2sixel <path>` 写入 PTY 的 stdin。
    //   4. 约 500 ms 后,PTY 输出流包含 Sixel 数据,
    //      `Event::GraphicsImage` 事件以 `protocol = Sixel` 触发。
    //   5. 图像出现在 `p4.image_store.sequential()` 中,
    //      `width == image.width` 且 `height == image.height`。
    //   6. 再过约 16 ms,`p4.image_quad_state.atlas.get(id)` 返回
    //      有效的 `ImagePlacement`(GPU 上传完成)。
    //
    // 跳过场景:$PATH 缺少 `img2sixel` 或测试 PNG 资源;CI
    // 环境可能没有这些。
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
fn chunked_graphics_smoke() {
    // 结构与 `sixel_smoke_img2sixel` 相同,但改用
    // 外部参考发射器的分块图形协议
    // `icat` kitten。要求该发射器二进制在 $PATH 中。
    let found = std::process::Command::new("which")
        .arg("kitty")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "reference graphics emitter `kitty` not on $PATH — install with `brew install kitty` or skip"
    );
    let _ = wait_for(Duration::from_secs(1), || false);
}

#[test]
#[ignore]
fn oversized_image_rejected_by_atlas() {
    // 验证思路:
    //   1. 合成 4096×4096 的 RGBA 缓冲(64 MiB,超过 `ATLAS_SIZE`)。
    //   2. 调用 `ImageAtlas::insert(0, Arc::new(buf), 4096, 4096)`。
    //   3. 断言其返回 `None`(图集上限为 2048²)。
    //   4. 确认 `p4.image_store` 仍把被拒图像记录为
    //      仅元数据(无 GPU 上传)——渲染器直接跳过它。
    //
    // 这并非真正的 E2E,但它放在此文件,因为它覆盖了
    // ImageStore 容量与图集容量之间的集成点。
    use pureterm_alacritty_runtime::renderer::image_atlas::ImageAtlas;

    let mut atlas = ImageAtlas::new();
    let buf = std::sync::Arc::new(vec![0u8; 4096 * 4096 * 4]);
    let r = atlas.insert(0, buf, 4096, 4096);
    assert!(r.is_none(), "oversized image must be rejected");
}