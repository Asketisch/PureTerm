//! 一个零开销的包装组件,用于为终端面板激活操作系统输入法(IME)。
//! 内置的 `Shader` 组件从不调用 `shell.request_input_method()`,因此没有
//! 这个包装,OS IME 永远不会被启用,CJK 输入无法工作。
//!
//! T-core-shaper v3:[`cluster_to_cursor_position`] 辅助函数把 preedit
//! 字节偏移映射为 IME 组合窗口的像素矩形。它由下方内联的 `cluster_demo`
//! 测试验证 —— 无需显示器、无需 iced 运行时,因此与其余
//! cluster↔cell 管道同属 `cargo test -p pureterm --bin pureterm` 调用。

use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Clipboard, Layout, Shell};
use iced::advanced::input_method::{InputMethod, Purpose};
use iced::mouse;
use iced::{Element, Event, Rectangle, Size, Length, Vector};
use pureterm_alacritty_runtime::shaper::ShapedRun;
#[cfg(test)]
use pureterm_alacritty_runtime::shaper::Shaper as _;

/// T-core-shaper v3:计算给定 preedit 字节偏移(位于先前已 shape 的 run 内)
/// 在 IME 组合光标处的屏幕空间像素矩形。
///
/// - `run`:覆盖当前行的 [`ShapedRun`](通常为组合期间的提示符行)。
/// - `byte_offset`:preedit 光标在底层输入字符串中的字节位置。0 = 在第一个
///   cluster 之前。
/// - `col_origin`:run 开始的屏幕列(使矩形位于面板坐标系中)。
/// - `cell_w` / `cell_h`:逻辑像素下的单元格尺寸。
///
/// 当 run 为空(未 shape 出字形)时返回 `None`。
pub fn cluster_to_cursor_position(
    run: &ShapedRun,
    byte_offset: usize,
    col_origin: u16,
    cell_w: f32,
    cell_h: f32,
) -> Option<Rectangle> {
    let cell = run.cluster_byte_to_cell(byte_offset)?;
    let x = (col_origin as f32 + cell as f32) * cell_w;
    Some(Rectangle {
        x,
        y: 0.0,
        width: cell_w,
        height: cell_h,
    })
}

/// 每次 `update` 调用时以 `Purpose::Terminal` 激活 IME 的包装组件。
pub struct ImeWrapper<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
}

impl<'a, Message, Theme, Renderer> ImeWrapper<'a, Message, Theme, Renderer> {
    pub fn new(content: Element<'a, Message, Theme, Renderer>) -> Self {
        Self { content }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for ImeWrapper<'_, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            limits,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget()
            .draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // 先委托给内部组件
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event, layout, cursor, renderer, clipboard, shell, viewport,
        );

        // 总是以 Terminal 用途请求启用 IME。
        // 没有这一步,OS 永远不会为我们的窗口启用输入法。
        shell.request_input_method::<String>(&InputMethod::Enabled {
            cursor: Rectangle::default(),
            purpose: Purpose::Terminal,
            preedit: None,
        });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'a, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

impl<'a, Message, Theme, Renderer> From<ImeWrapper<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(wrapper: ImeWrapper<'a, Message, Theme, Renderer>) -> Self {
        Element::new(wrapper)
    }
}

// --- T-core-shaper v3:cluster↔cell 反查 demo ---
//
// 这些测试是核对表 B.5("ime_widget.rs 撤销/重做 cluster↔cell 反查 demo")
// 的单测部分。可视部分由下面 `cluster_demo_e2e_documentation` 描述的
// e2e 流程验证 —— 渲染真正的 iced 组件需要显示器,所以它保留为
// 指南而非 `#[test]`。
#[cfg(test)]
mod cluster_demo {
    use super::*;
    use pureterm_alacritty_runtime::shaper::MonospaceShaper;

    fn shape_ascii() -> ShapedRun {
        MonospaceShaper::new().shape("ls -la", 8.0)
    }

    fn shape_cjk() -> ShapedRun {
        // "你" 是 U+4F60(3 个 UTF-8 字节),宽字符 → 2 个单元格。
        MonospaceShaper::new().shape("a你b", 8.0)
    }

    #[test]
    fn ascii_byte_zero_is_cell_zero() {
        let run = shape_ascii();
        let r = cluster_to_cursor_position(&run, 0, 0, 8.0, 16.0).unwrap();
        assert_eq!(r.x, 0.0);
        assert_eq!(r.width, 8.0);
        assert_eq!(r.height, 16.0);
    }

    #[test]
    fn ascii_middle_byte_lands_on_correct_cell() {
        let run = shape_ascii();
        // "ls -la" → 集群:l(0), s(1), 空格(2), -(3), l(4), a(5)。
        // 字节 2(" " 开头)→ cell 2。
        let r = cluster_to_cursor_position(&run, 2, 0, 8.0, 16.0).unwrap();
        assert_eq!(r.x, 16.0); // 第 2 列 * 8 px
    }

    #[test]
    fn ascii_byte_past_end_returns_last_cell() {
        let run = shape_ascii();
        // "ls -la" 有 6 个字符;字节 100 已越界。
        // cluster_byte_to_cell 返回 Some(last_cell)(不会返回 None),因为
        // 最后一个字形的 cluster(5)<= 100。
        let r = cluster_to_cursor_position(&run, 100, 0, 8.0, 16.0).unwrap();
        // cluster_to_cell 中最末单元格索引为 5 → x = 5 * 8 = 40。
        assert_eq!(r.x, 40.0);
    }

    #[test]
    fn cjk_wide_char_occupies_two_cells() {
        let run = shape_cjk();
        // "a你b" → 集群:a(0,1 格), 你(1,2 格), b(3,1 格)。
        // 字节 0('a' 之前)→ cell 0。
        let r0 = cluster_to_cursor_position(&run, 0, 0, 8.0, 16.0).unwrap();
        assert_eq!(r0.x, 0.0);
        // 字节 1("你" 开头)→ cell 1。
        let r1 = cluster_to_cursor_position(&run, 1, 0, 8.0, 16.0).unwrap();
        assert_eq!(r1.x, 8.0);
        // 字节 4("b" 开头)→ cell 3(a=1 + 你=2 = 已占 3 格)。
        let r3 = cluster_to_cursor_position(&run, 4, 0, 8.0, 16.0).unwrap();
        assert_eq!(r3.x, 24.0);
    }

    #[test]
    fn empty_run_returns_none() {
        let run = ShapedRun {
            glyphs: vec![],
            cluster_to_cell: vec![],
        };
        assert!(cluster_to_cursor_position(&run, 0, 0, 8.0, 16.0).is_none());
    }

    #[test]
    fn col_origin_shifts_rectangle() {
        let run = shape_ascii();
        // 原点在第 10 列 → x = (10 + cell) * cell_w。
        let r = cluster_to_cursor_position(&run, 2, 10, 8.0, 16.0).unwrap();
        assert_eq!(r.x, (10.0 + 2.0) * 8.0);
    }

    /// T-core-shaper v3:demo 可视半部分的纯文档说明。不是 `#[test]`,因为
    /// 渲染 iced 组件需要显示表面(X11/Wayland/macOS);无头运行 `iced::run`
    /// 需要完整的 mock 渲染器。上面的单元测试覆盖 cluster→cell 算术核心;
    /// 本文档注释是可视集成的契约。
    #[allow(dead_code)]
    fn cluster_demo_e2e_documentation() {
        // 1. 在带显示的宿主机上启动 `pureterm`。
        // 2. 在终端面板里运行一个 unicode 测试发射器。
        // 3. 终端面板的 IME 包装器收到一个带 `preedit: Some("…")` 字节流的
        //    组合事件。
        // 4. 对 preedit 流中的每个字节偏移,调用
        //    `cluster_to_cursor_position(&run, offset, col, cell_w, cell_h)`,
        //    并把结果 `Rectangle` 喂给
        //    `shell.request_input_method(Enabled { cursor, .., preedit })`。
        // 5. 目视确认 OS IME 组合窗口跟随 cluster 位置 —— 对 CJK 输入,光标
        //    停在宽字符的中间(cell * cell_w),而不是字节边界。
    }
}
