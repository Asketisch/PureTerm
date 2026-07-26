//! A zero-cost wrapper widget that activates the OS input method (IME) for
//! the terminal pane. The built-in `Shader` widget never calls
//! `shell.request_input_method()`, so without this wrapper the OS IME is
//! never enabled and CJK input does not work.
//!
//! T-core-shaper v3: the [`cluster_to_cursor_position`] helper maps a
//! preedit byte offset to a pixel rectangle for the IME composition
//! window. It's exercised by the inline `cluster_demo` tests below —
//! no display, no iced runtime required, so it lives in the same
//! `cargo test -p pureterm --bin pureterm` invocation as the rest of
//! the cluster↔cell plumbing.

use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Clipboard, Layout, Shell};
use iced::advanced::input_method::{InputMethod, Purpose};
use iced::mouse;
use iced::{Element, Event, Rectangle, Size, Length, Vector};
use pureterm_alacritty_runtime::shaper::ShapedRun;
#[cfg(test)]
use pureterm_alacritty_runtime::shaper::Shaper as _;

/// T-core-shaper v3: compute the IME composition cursor's screen-space
/// pixel rectangle for a given preedit byte offset inside a previously
/// shaped run.
///
/// - `run`: the [`ShapedRun`] covering the current row (typically the
///   prompt row during composition).
/// - `byte_offset`: the byte position of the preedit cursor inside the
///   underlying input string. 0 = before the first cluster.
/// - `col_origin`: the screen column where the run starts (so the
///   rectangle is in pane coordinates).
/// - `cell_w` / `cell_h`: the cell dimensions in logical px.
///
/// Returns `None` when the run is empty (no glyphs shaped).
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

/// Wrapper that activates IME with `Purpose::Terminal` on every `update` call.
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
        // Delegate to inner widget first
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event, layout, cursor, renderer, clipboard, shell, viewport,
        );

        // Always request IME activation with Terminal purpose.
        // Without this, the OS never enables the input method for our window.
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

// --- T-core-shaper v3: cluster↔cell reverse-lookup demo ---
//
// These tests are the unit-test half of checklist B.5 ("ime_widget.rs
// 撤销/重做 cluster↔cell 反查 demo"). The visual half is exercised by
// the e2e flow described in `cluster_demo_e2e_documentation` below —
// rendering an actual iced widget requires a display, so it stays as a
// guide rather than a `#[test]`.
#[cfg(test)]
mod cluster_demo {
    use super::*;
    use pureterm_alacritty_runtime::shaper::MonospaceShaper;

    fn shape_ascii() -> ShapedRun {
        MonospaceShaper::new().shape("ls -la", 8.0)
    }

    fn shape_cjk() -> ShapedRun {
        // "你" is U+4F60 (3 UTF-8 bytes), wide → 2 cells.
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
        // "ls -la" → clusters: l(0), s(1), space(2), -(3), l(4), a(5).
        // Byte 2 (start of " ") → cell 2.
        let r = cluster_to_cursor_position(&run, 2, 0, 8.0, 16.0).unwrap();
        assert_eq!(r.x, 16.0); // col 2 * 8 px
    }

    #[test]
    fn ascii_byte_past_end_returns_last_cell() {
        let run = shape_ascii();
        // "ls -la" has 6 chars; byte 100 is past the end.
        // cluster_byte_to_cell returns Some(last_cell) (no None) since the
        // last glyph's cluster (5) <= 100.
        let r = cluster_to_cursor_position(&run, 100, 0, 8.0, 16.0).unwrap();
        // Last cell index in cluster_to_cell is 5 → x = 5 * 8 = 40.
        assert_eq!(r.x, 40.0);
    }

    #[test]
    fn cjk_wide_char_occupies_two_cells() {
        let run = shape_cjk();
        // "a你b" → clusters: a(0,1 cell), 你(1,2 cells), b(3,1 cell).
        // Byte 0 (before 'a') → cell 0.
        let r0 = cluster_to_cursor_position(&run, 0, 0, 8.0, 16.0).unwrap();
        assert_eq!(r0.x, 0.0);
        // Byte 1 (start of "你") → cell 1.
        let r1 = cluster_to_cursor_position(&run, 1, 0, 8.0, 16.0).unwrap();
        assert_eq!(r1.x, 8.0);
        // Byte 4 (start of "b") → cell 3 (a=1 + 你=2 = 3 cells consumed).
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
        // Origin at column 10 → x = (10 + cell) * cell_w.
        let r = cluster_to_cursor_position(&run, 2, 10, 8.0, 16.0).unwrap();
        assert_eq!(r.x, (10.0 + 2.0) * 8.0);
    }

    /// T-core-shaper v3: documentation-only description of the visual
    /// half of the demo. Not a `#[test]` because rendering an iced
    /// widget requires a display surface (X11/Wayland/macOS); running
    /// `iced::run` headless would need a full mock renderer. The unit
    /// tests above cover the core cluster-to-cell arithmetic; this
    /// doc-comment is the contract for the visual integration.
    #[allow(dead_code)]
    fn cluster_demo_e2e_documentation() {
        // 1. Boot `pureterm` on a host with a display.
        // 2. Run a unicode-test emitter in the terminal pane.
        // 3. The terminal pane's IME wrapper receives a composing event
        //    with a `preedit: Some("…")` byte stream.
        // 4. For each byte offset in the preedit stream, call
        //    `cluster_to_cursor_position(&run, offset, col, cell_w, cell_h)`
        //    and feed the resulting `Rectangle` to
        //    `shell.request_input_method(Enabled { cursor, .., preedit })`.
        // 5. Visually confirm the OS IME composition window tracks the
        //    cluster position — for CJK input the cursor sits over the
        //    middle of the wide character (cell * cell_w), not at a
        //    byte boundary.
    }
}
