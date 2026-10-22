//! GAP-7.9：把终端 pane 的网格捕获为文本（`surface.snapshot`）。
//!
//! 支撑 MCP 的 `capture_pane` 工具与 `pureterm surface snapshot` 命令。
//! 网格锁 + `bounds_to_string` 在 iced 主线程之外运行（请求像 `browser.*` 一样
//! 被延迟处理）：序列化完整回滚缓冲区可能要遍历数百万个单元格，
//! 在主线程上做会拖帧。

use std::sync::Arc;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;

use crate::ipc::JsonRpcResponse;
use crate::terminal_context::ChannelEventListener;

/// 只保留 `s` 的最后 `n` 行。返回 `(tail, truncated)`；当 `s` 超过 `n` 行并被
/// 裁剪时 `truncated` 为真。行切分与 `str::lines` 一致（末尾换行不产生空的
/// 最后一行）；输入末尾的换行会保留在 tail 中，使结果与输入的对应切片
/// 读起来完全一致。
pub fn tail_lines(s: &str, n: usize) -> (String, bool) {
    if n == 0 {
        return (String::new(), !s.is_empty());
    }
    let all: Vec<&str> = s.lines().collect();
    if all.len() <= n {
        return (s.to_string(), false);
    }
    let kept = &all[all.len() - n..];
    let mut out = kept.join("\n");
    if s.ends_with('\n') {
        out.push('\n');
    }
    (out, true)
}

/// T-core-graphics v2：捕获到表面快照中的内联图片放置。与文本网格一起持久化，
/// 使快照能按屏幕上的原样精确重渲染（用于即时回放、`pureterm surface snapshot
/// --include-images`，以及 MCP 的 `capture_pane` 工具）。
///
/// 放置的 RGBA 像素数据存放在 `pixels` 中。没有 `pixels` 时，快照只是一个
/// 占位符——渲染器可用它预留网格坐标，但不绘制。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImagePlacement {
    /// 稳定的图片 id（分块图形协议的 `i=…`、Sixel 序列 id 等）。
    /// 用于在快照/恢复周期之间去重。
    pub id: u64,
    /// 图片起始列（从 0 开始）。
    pub col: u16,
    /// 图片起始行（从 0 开始）。
    pub row: u16,
    /// 以单元格为单位的宽度（分块图形协议的 `c=` 或 Sixel 的 `;q`）。
    pub cols: u16,
    /// 以单元格为单位的高度。
    pub rows: u16,
    /// 解码后负载的像素尺寸。
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// RGBA8 像素数据。快照占位时为 `None`（序列化时图片已被回滚缓冲区修剪掉）。
    pub pixels: Option<Arc<Vec<u8>>>,
}

/// 主线程外的捕获完成后，iced 主循环结束一个延迟的 `surface.snapshot` 请求
/// 所需的一切。与 `BrowserResolved` 对应：`reply_tx` 是克隆的 IPC 句柄；循环
/// 通过它恰好发送一次响应，并取走 `oneshot::Sender`，使迟到的完成无法重复回复。
#[derive(Clone)]
pub struct SurfaceSnapshotResolved {
    pub ipc_id: u64,
    pub reply_tx: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<JsonRpcResponse>>>>,
    /// 完整构建好的响应（捕获结果，或加锁/任务失败时的错误）。
    /// 在主线程之外构建；主循环只负责转发。
    pub resp: JsonRpcResponse,
}

// 手工实现 `Debug`——`oneshot::Sender` 没有 `Debug`，而 `Message` 需要派生它。
impl std::fmt::Debug for SurfaceSnapshotResolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurfaceSnapshotResolved")
            .field("ipc_id", &self.ipc_id)
            .finish_non_exhaustive()
    }
}

/// 锁定终端网格并序列化为文本。`visible_only` 把捕获限制在当前视口
///（通过 `display_offset`）；`lines_cap` 只保留最后 N 行。直接构建
/// JSON-RPC 成功响应。
///
/// 设计在 iced 主线程之外运行（从 `spawn_blocking` 调用）：对完整历史执行
/// `bounds_to_string` 是 O(单元格数)，若内联在 `update()` 中会拖帧。
pub fn capture(
    term_arc: Arc<FairMutex<Term<ChannelEventListener>>>,
    visible_only: bool,
    lines_cap: Option<usize>,
    ipc_id: u64,
    cell_size: (f32, f32),
) -> JsonRpcResponse {
    let term = term_arc.lock();
    let start = if visible_only {
        // 首条可见行 = 最顶行 + display_offset。历史行是负数；`display_offset` 表示
        // 视口从底部向上滚动的距离（0 = 显示末尾）。
        Point::new(
            Line(term.topmost_line().0 + term.grid().display_offset() as i32),
            Column(0),
        )
    } else {
        Point::new(term.topmost_line(), Column(0))
    };
    let end = Point::new(term.bottommost_line(), term.last_column());
    let full = term.bounds_to_string(start, end);
    drop(term);

    let (text, truncated) = match lines_cap {
        Some(cap) => tail_lines(&full, cap),
        None => (full, false),
    };
    let lines = text.lines().count();
    JsonRpcResponse::ok(
        ipc_id,
        serde_json::json!({
            "text": text,
            "lines": lines,
            "truncated": truncated,
            // 单元格像素尺寸 —— 消费方(如 `capture export --format svg`)
            // 需要它才能把终端重建成尺寸正确的图像。
            "cell_width": cell_size.0,
            "cell_height": cell_size.1,
        }),
    )
}

/// GAP-02：把可见视口捕获为文本，最多保留最后 `max_lines` 行。
/// 在终端锁下运行——`max_lines` 应保持适中。
pub fn capture_visible_tail(term: &Term<ChannelEventListener>, max_lines: usize) -> String {
    let start = Point::new(
        Line(term.topmost_line().0 + term.grid().display_offset() as i32),
        Column(0),
    );
    let end = Point::new(term.bottommost_line(), term.last_column());
    let full = term.bounds_to_string(start, end);
    tail_lines(&full, max_lines).0
}

/// GAP-08：读取光标所在处的终端网格行（当前输入行）。
pub fn capture_cursor_line(term: &Term<ChannelEventListener>) -> String {
    let cursor = &term.grid().cursor;
    let line = cursor.point.line;
    let start = Point::new(line, Column(0));
    let end = Point::new(line, term.last_column());
    term.bounds_to_string(start, end).trim_end().to_string()
}

/// GAP-02：按 OSC 133 标记记录，捕获两条光标行之间（含边界）的网格文本。
/// 当区间为空或反转时回退到可见末尾。
pub fn capture_row_range(
    term: &Term<ChannelEventListener>,
    start_row: usize,
    end_row: usize,
    max_lines: usize,
) -> String {
    if start_row > end_row {
        return capture_visible_tail(term, max_lines);
    }
    let top = Line(start_row as i32);
    let bottom = Line(end_row as i32);
    let start = Point::new(top, Column(0));
    let end = Point::new(bottom, term.last_column());
    let full = term.bounds_to_string(start, end);
    if full.trim().is_empty() {
        return capture_visible_tail(term, max_lines);
    }
    tail_lines(&full, max_lines).0
}

/// GAP-51 v1：[`capture_row_range`] 的主线程外变体。iced 循环中的 OSC 133 D
/// 标记处理器委托给它，使长时间运行的网格遍历不会拖顿 GUI（与 
/// `surface.snapshot` IPC 工具采用同样的模式）。
///
/// `term_arc` 与 PTY 读取线程共享；我们只在 `bounds_to_string` 遍历期间持有锁，
/// 返回前释放。`Some`/`None` 的形状让调用方能区分“捕获成功”与
/// “任务 panic / 运行时已关闭”。
pub async fn capture_row_range_async(
    term_arc: Arc<FairMutex<Term<ChannelEventListener>>>,
    start_row: usize,
    end_row: usize,
    max_lines: usize,
) -> Option<String> {
    tokio::task::spawn_blocking(move || {
        let term = term_arc.lock();
        capture_row_range(&term, start_row, end_row, max_lines)
    })
    .await
    .ok()
}

#[cfg(test)]
mod tests {
    use super::{capture, tail_lines};
    use crate::terminal_context::{ChannelEventListener, TerminalEvent};
    use alacritty_terminal::sync::FairMutex;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::term::{Config, Term};
    use alacritty_terminal::vte::ansi;
    use crossbeam_channel::bounded;
    use std::sync::Arc;

    /// 捕获时使用的单元格尺寸（像素）。`capture()` 用它把字符数换算成
    /// 像素宽度，供 SVG 导出使用。
    const CELL: (f32, f32) = (8.0, 16.0);

    /// 构建一个写入 `text` 的 `Term<ChannelEventListener>` 网格
    ///（CR-LF 换行，与 PTY 投递字节的方式一致），供 `capture()` 使用。
    /// 通过一次性 channel 使用 `ChannelEventListener`——我们从不读取事件；
    /// 监听器只需满足 `Term` 的类型参数。
    fn term_with(text: &str) -> Arc<FairMutex<Term<ChannelEventListener>>> {
        let (tx, _rx) = bounded::<TerminalEvent>(16);
        let size = TermSize::new(80, 24);
        let mut term = Term::new(Config::default(), &size, ChannelEventListener::new(tx));
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut term, text.as_bytes());
        Arc::new(FairMutex::new(term))
    }

    #[test]
    fn below_count_returns_all_untruncated() {
        let (out, trunc) = tail_lines("a\nb\nc", 5);
        assert_eq!(out, "a\nb\nc");
        assert!(!trunc);
    }

    #[test]
    fn keeps_last_n_and_marks_truncated() {
        let (out, trunc) = tail_lines("a\nb\nc\nd", 2);
        assert_eq!(out, "c\nd");
        assert!(trunc);
    }

    #[test]
    fn preserves_trailing_newline() {
        // "a\nb\nc\n" 有 3 行 + 末尾换行；tail 2 保留 "b\nc\n"。
        let (out, trunc) = tail_lines("a\nb\nc\n", 2);
        assert_eq!(out, "b\nc\n");
        assert!(trunc);
    }

    #[test]
    fn zero_returns_empty_and_marks_truncated_if_nonempty() {
        let (out, trunc) = tail_lines("a\nb", 0);
        assert_eq!(out, "");
        assert!(trunc);
    }

    #[test]
    fn empty_input_is_not_truncated() {
        let (out, trunc) = tail_lines("", 3);
        assert_eq!(out, "");
        assert!(!trunc);
    }

    // ── capture()：真实网格读取（GAP-6.6 捕获 pane 验收）──────────────
    // 路由器的 `surface.snapshot` 处理器是桩；真实捕获路径是`iced_app.rs` →
    // `capture()`。这些测试直接驱动 `capture()`，使网格序列化逻辑无需真实
    // iced 窗口即可获得覆盖。

    #[test]
    fn capture_returns_text_written_into_grid() {
        let term = term_with("hello\r\nworld\r\n");
        let resp = capture(term, false, None, 1, CELL);
        let v = resp.result.expect("ok response");
        let text = v["text"].as_str().expect("text field");
        assert!(text.contains("hello"), "missing hello: {text:?}");
        assert!(text.contains("world"), "missing world: {text:?}");
        assert_eq!(v["truncated"], false, "full capture is not truncated");
    }

    #[test]
    fn capture_visible_only_returns_viewport_content() {
        // `visible_only` 从 `display_offset` 计算起始行；没有回滚缓冲区时，视口
        // 仍包含刚写入的末尾，因此捕获必须包含它（保护可见-only 分支，并确保
        // 不返回空/垃圾区间）。
        let term = term_with("fresh\r\nline\r\n");
        let resp = capture(term, true, None, 2, CELL);
        let v = resp.result.expect("ok response");
        let text = v["text"].as_str().expect("text field");
        assert!(text.contains("fresh") || text.contains("line"), "visible capture: {text:?}");
    }

    #[test]
    fn capture_lines_cap_truncates_full_grid_and_keeps_tail() {
        let term = term_with("a\r\nb\r\nc\r\nd\r\ne\r\n");
        // 完整捕获横跨整个视口（5 条写入行加末尾空行），因此会超过上限。
        // `tail_lines` 具体保留哪些行由 `tail_lines` 单元测试覆盖；
        // 这里只证明 `capture` 端到端接通了 `lines_cap`。
        let full = capture(Arc::clone(&term), false, None, 1, CELL);
        let full_text = full.result.unwrap()["text"].as_str().unwrap().to_string();
        let full_lines = full_text.lines().count();
        assert!(full_lines >= 5, "full capture must hold the ≥5 written lines: {full_lines}");

        // 上限 2 ⇒ 标记为截断且最多保留 2 行。
        let capped = capture(term, false, Some(2), 2, CELL);
        let v = capped.result.unwrap();
        assert_eq!(v["truncated"], true, "capping a >2-line grid must truncate");
        assert!(v["text"].as_str().unwrap().lines().count() <= 2, "capped should have ≤2 lines");
    }

    // ── GAP-51 v1:capture_row_range_async(异步变体) ──────────────────────────────
    //
    // `capture_row_range_async` 把加锁 + `bounds_to_string` 遍历推迟到
    // `spawn_blocking`。这里自行构建 runtime(而不是依赖
    // 我们不依赖的 `#[tokio::test]` 属性),让测试表面
    // 保持自包含。
    #[test]
    fn capture_row_range_async_returns_some() {
        let term = term_with("async-hello\r\nasync-world\r\n");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        let text = rt
            .block_on(super::capture_row_range_async(term, 0, 1, 32))
            .expect("async capture resolves to Some");
        assert!(text.contains("async-hello"), "missing async-hello: {text:?}");
        assert!(text.contains("async-world"), "missing async-world: {text:?}");
    }
}

#[cfg(test)]
mod image_placement_tests {
    //! T-core-graphics v2:针对快照侧 ImagePlacement 记账的测试。
    //! 该放置是纯数据;测试验证
    //! 经 JSON 快照响应的序列化往返。
    use super::*;
    use std::sync::Arc;

    #[test]
    fn image_placement_default_construction() {
        let p = ImagePlacement {
            id: 1,
            col: 0,
            row: 5,
            cols: 10,
            rows: 4,
            pixel_width: 100,
            pixel_height: 40,
            pixels: None,
        };
        assert_eq!(p.id, 1);
        assert_eq!(p.col, 0);
        assert_eq!(p.row, 5);
        assert_eq!(p.cols, 10);
        assert_eq!(p.rows, 4);
        assert!(p.pixels.is_none());
    }

    #[test]
    fn image_placement_with_pixels_equality() {
        let p1 = ImagePlacement {
            id: 7,
            col: 3,
            row: 0,
            cols: 5,
            rows: 5,
            pixel_width: 50,
            pixel_height: 50,
            pixels: Some(Arc::new(vec![0u8; 50 * 50 * 4])),
        };
        let p2 = p1.clone();
        assert_eq!(p1, p2);
        // 像素数据不同 → 不相等。
        let mut p3 = p1.clone();
        p3.pixels = Some(Arc::new(vec![1u8; 50 * 50 * 4]));
        assert_ne!(p1, p3);
    }

    /// 含图片的快照响应把放置序列化到
    /// `images` 列表下。这能防止误改动字段名。
    #[test]
    fn image_placement_serializes_into_json() {
        let p = ImagePlacement {
            id: 42,
            col: 2,
            row: 1,
            cols: 4,
            rows: 3,
            pixel_width: 40,
            pixel_height: 30,
            pixels: None,
        };
        let v = serde_json::json!({
            "images": vec![&p],
        });
        let arr = v["images"].as_array().expect("images is an array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], 42);
        assert_eq!(arr[0]["cols"], 4);
        assert_eq!(arr[0]["rows"], 3);
    }
}
