//! 导出 `Term` 类型,它是 Grid 的高层 API。

use std::fmt::Write as _;
use std::ops::{Index, IndexMut, Range};
use std::sync::Arc;
use std::{cmp, mem, ptr, slice, str};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as Base64;
use bitflags::bitflags;
use log::{debug, trace};
use unicode_width::UnicodeWidthChar;

use crate::event::{Event, EventListener};
use crate::grid::{Dimensions, Grid, GridIterator, Scroll};
use crate::index::{self, Boundary, Column, Direction, Line, Point, Side};
use crate::selection::{Selection, SelectionRange, SelectionType};
use crate::smart_selection::SmartSelectionService;
use crate::term::cell::{Cell, Flags, LineLength};
use crate::term::color::Colors;
use crate::decscl::ConformanceLevel;
use crate::vi_mode::{ViModeCursor, ViMotion};
use crate::vte::ansi::{
    self, Attr, CharsetIndex, Color, CursorShape, CursorStyle, Handler, Hyperlink, KeyboardModes,
    KeyboardModesApplyBehavior, NamedColor, NamedMode, NamedPrivateMode, PrivateMode, Rgb,
    StandardCharset,
};

pub mod cell;
pub mod color;
pub mod search;

/// 最小列数。
///
/// 至少需要 2 列才能容纳全角 Unicode 字符。
pub const MIN_COLUMNS: usize = 2;

/// 最小可见行数。
pub const MIN_SCREEN_LINES: usize = 1;

/// 窗口标题栈的最大深度。
const TITLE_STACK_MAX_DEPTH: usize = 4096;

/// 默认语义选择分隔字符。
pub const SEMANTIC_ESCAPE_CHARS: &str = ",│`|:\"' ()[]{}<>\t";

/// 键盘模式栈的最大深度。
const KEYBOARD_MODE_STACK_MAX_DEPTH: usize = TITLE_STACK_MAX_DEPTH;

/// 默认制表位间隔,对应 terminfo 的 `it` 值。
const INITIAL_TABSTOPS: usize = 8;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TermMode: u32 {
        const NONE                    = 0;
        const SHOW_CURSOR             = 1;
        const APP_CURSOR              = 1 << 1;
        const APP_KEYPAD              = 1 << 2;
        const MOUSE_REPORT_CLICK      = 1 << 3;
        const BRACKETED_PASTE         = 1 << 4;
        const SGR_MOUSE               = 1 << 5;
        const MOUSE_MOTION            = 1 << 6;
        const LINE_WRAP               = 1 << 7;
        const LINE_FEED_NEW_LINE      = 1 << 8;
        const ORIGIN                  = 1 << 9;
        const INSERT                  = 1 << 10;
        const FOCUS_IN_OUT            = 1 << 11;
        const ALT_SCREEN              = 1 << 12;
        const MOUSE_DRAG              = 1 << 13;
        const UTF8_MOUSE              = 1 << 14;
        const ALTERNATE_SCROLL        = 1 << 15;
        const VI                      = 1 << 16;
        const URGENCY_HINTS           = 1 << 17;
        const DISAMBIGUATE_ESC_CODES  = 1 << 18;
        const REPORT_EVENT_TYPES      = 1 << 19;
        const REPORT_ALTERNATE_KEYS   = 1 << 20;
        const REPORT_ALL_KEYS_AS_ESC  = 1 << 21;
        const REPORT_ASSOCIATED_TEXT  = 1 << 22;
        const MOUSE_MODE              = Self::MOUSE_REPORT_CLICK.bits() | Self::MOUSE_MOTION.bits() | Self::MOUSE_DRAG.bits();
        const KITTY_KEYBOARD_PROTOCOL = Self::DISAMBIGUATE_ESC_CODES.bits()
                                      | Self::REPORT_EVENT_TYPES.bits()
                                      | Self::REPORT_ALTERNATE_KEYS.bits()
                                      | Self::REPORT_ALL_KEYS_AS_ESC.bits()
                                      | Self::REPORT_ASSOCIATED_TEXT.bits();
         const ANY                    = u32::MAX;
    }
}

impl From<KeyboardModes> for TermMode {
    fn from(value: KeyboardModes) -> Self {
        let mut mode = Self::empty();

        let disambiguate_esc_codes = value.contains(KeyboardModes::DISAMBIGUATE_ESC_CODES);
        mode.set(TermMode::DISAMBIGUATE_ESC_CODES, disambiguate_esc_codes);

        let report_event_types = value.contains(KeyboardModes::REPORT_EVENT_TYPES);
        mode.set(TermMode::REPORT_EVENT_TYPES, report_event_types);

        let report_alternate_keys = value.contains(KeyboardModes::REPORT_ALTERNATE_KEYS);
        mode.set(TermMode::REPORT_ALTERNATE_KEYS, report_alternate_keys);

        let report_all_keys_as_esc = value.contains(KeyboardModes::REPORT_ALL_KEYS_AS_ESC);
        mode.set(TermMode::REPORT_ALL_KEYS_AS_ESC, report_all_keys_as_esc);

        let report_associated_text = value.contains(KeyboardModes::REPORT_ASSOCIATED_TEXT);
        mode.set(TermMode::REPORT_ASSOCIATED_TEXT, report_associated_text);

        mode
    }
}

impl Default for TermMode {
    fn default() -> TermMode {
        TermMode::SHOW_CURSOR
            | TermMode::LINE_WRAP
            | TermMode::ALTERNATE_SCROLL
            | TermMode::URGENCY_HINTS
    }
}

/// 把终端坐标转换为视口相对坐标。
#[inline]
pub fn point_to_viewport(display_offset: usize, point: Point) -> Option<Point<usize>> {
    let viewport_line = point.line.0 + display_offset as i32;
    usize::try_from(viewport_line).ok().map(|line| Point::new(line, point.column))
}

/// 把视口相对坐标转换为终端坐标。
#[inline]
pub fn viewport_to_point(display_offset: usize, point: Point<usize>) -> Point {
    let line = Line(point.line as i32) - display_offset;
    Point::new(line, point.column)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineDamageBounds {
    /// 受损行号。
    pub line: usize,

    /// 最左受损列。
    pub left: usize,

    /// 最右受损列。
    pub right: usize,
}

impl LineDamageBounds {
    #[inline]
    pub fn new(line: usize, left: usize, right: usize) -> Self {
        Self { line, left, right }
    }

    #[inline]
    pub fn undamaged(line: usize, num_cols: usize) -> Self {
        Self { line, left: num_cols, right: 0 }
    }

    #[inline]
    pub fn reset(&mut self, num_cols: usize) {
        *self = Self::undamaged(self.line, num_cols);
    }

    #[inline]
    pub fn expand(&mut self, left: usize, right: usize) {
        self.left = cmp::min(self.left, left);
        self.right = cmp::max(self.right, right);
    }

    #[inline]
    pub fn is_damaged(&self) -> bool {
        self.left <= self.right
    }
}

/// 自上次 [`Term::reset_damage`] 调用以来收集的终端受损信息。
#[derive(Debug)]
pub enum TermDamage<'a> {
    /// 整个终端均受损。
    Full,

    /// 遍历终端中受损行的迭代器。
    Partial(TermDamageIterator<'a>),
}

/// 遍历终端视口内受损行的迭代器。
#[derive(Clone, Debug)]
pub struct TermDamageIterator<'a> {
    line_damage: slice::Iter<'a, LineDamageBounds>,
    display_offset: usize,
}

impl<'a> TermDamageIterator<'a> {
    pub fn new(line_damage: &'a [LineDamageBounds], display_offset: usize) -> Self {
        let num_lines = line_damage.len();
        // 过滤掉视口外不可见的受损区域。
        let line_damage = &line_damage[..num_lines.saturating_sub(display_offset)];
        Self { display_offset, line_damage: line_damage.iter() }
    }
}

impl Iterator for TermDamageIterator<'_> {
    type Item = LineDamageBounds;

    fn next(&mut self) -> Option<Self::Item> {
        self.line_damage.find_map(|line| {
            line.is_damaged().then_some(LineDamageBounds::new(
                line.line + self.display_offset,
                line.left,
                line.right,
            ))
        })
    }
}

/// 终端受损状态。
struct TermDamageState {
    /// 提示是否应无视实际受损变化而整体标记终端受损。
    full: bool,

    /// 各行受损信息。
    lines: Vec<LineDamageBounds>,

    /// 旧终端光标位置。
    last_cursor: Point,
}

impl TermDamageState {
    fn new(num_cols: usize, num_lines: usize) -> Self {
        let lines =
            (0..num_lines).map(|line| LineDamageBounds::undamaged(line, num_cols)).collect();

        Self { full: true, lines, last_cursor: Default::default() }
    }

    #[inline]
    fn resize(&mut self, num_cols: usize, num_lines: usize) {
        // 重置位置,避免旧光标落在视口之外。
        self.last_cursor = Default::default();
        self.full = true;

        self.lines.clear();
        self.lines.reserve(num_lines);
        for line in 0..num_lines {
            self.lines.push(LineDamageBounds::undamaged(line, num_cols));
        }
    }

    /// 标记视口内某个点受损。
    #[inline]
    fn damage_point(&mut self, point: Point<usize>) {
        self.damage_line(point.line, point.column.0, point.column.0);
    }

    /// 扩展 `line` 的受损范围,至少覆盖 `left` 到 `right` 列。
    #[inline]
    fn damage_line(&mut self, line: usize, left: usize, right: usize) {
        self.lines[line].expand(left, right);
    }

    /// 重置终端受损信息。
    fn reset(&mut self, num_cols: usize) {
        self.full = false;
        self.lines.iter_mut().for_each(|line| line.reset(num_cols));
    }
}

pub struct Term<T> {
    /// 终端焦点状态,控制光标形状。
    pub is_focused: bool,

    /// 键盘选区(Vi 模式)光标。
    pub vi_mode_cursor: ViModeCursor,

    pub selection: Option<Selection>,

    /// 当前活动的网格。
    ///
    /// 跟踪当前使用的屏幕缓冲区。备用屏幕缓冲区活动时
    /// 它就是备用网格;否则是主屏幕缓冲区。
    grid: Grid<Cell>,

    /// 当前未活动的网格。
    ///
    /// 与活动网格相反。备用屏幕缓冲区活动时,
    /// 它是主网格;否则是备用屏幕缓冲区。
    inactive_grid: Grid<Cell>,

    /// `charsets` 的下标,指示当前 ASCII 被映射到哪个字符集。
    active_charset: CharsetIndex,

    /// 制表位。
    tabs: TabStops,

    /// 模式标志。
    mode: TermMode,

    /// 滚动区域。
    ///
    /// 从视口顶部索引的、终端顶部到底部的行范围。
    scroll_region: Range<Line>,

    /// 被修改过的终端颜色。
    colors: Colors,

    /// 光标当前样式。
    cursor_style: Option<CursorStyle>,

    /// 向事件循环发送事件的代理。
    event_proxy: T,

    /// 窗口当前标题。
    title: Option<String>,

    /// 已保存窗口标题的栈。从栈中弹出一个标题后,
    /// 就把它设为终端的 `title`。
    title_stack: Vec<Option<String>>,

    /// 键盘模式栈。
    keyboard_mode_stack: Vec<KeyboardModes>,

    /// 当前未活动的键盘模式栈。
    inactive_keyboard_mode_stack: Vec<KeyboardModes>,

    /// 受损单元信息。
    damage: TermDamageState,

    /// 基于精确正则匹配的智能选区服务。
    pub smart_selection: SmartSelectionService,

    /// 终端直接使用的配置。
    config: Config,

    /// GAP-1.2: DECSCL 一致性级别(CSI Ps " p)。默认 VT100。
    conformance_level: ConformanceLevel,
}

/// [`Term`] 的配置项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// 最大滚动历史行数。
    pub scrolling_history: usize,

    /// 重置光标时使用的默认样式。
    pub default_cursor_style: CursorStyle,

    /// Vi 模式光标样式。
    pub vi_mode_cursor_style: Option<CursorStyle>,

    /// 终止语义选区的字符集合。
    ///
    /// 默认值为 [`SEMANTIC_ESCAPE_CHARS`]。
    pub semantic_escape_chars: String,

    /// 是否启用 kitty 键盘协议。
    pub kitty_keyboard: bool,

    /// OSC52 支持模式。
    pub osc52: Osc52,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scrolling_history: 10000,
            semantic_escape_chars: SEMANTIC_ESCAPE_CHARS.to_owned(),
            default_cursor_style: Default::default(),
            vi_mode_cursor_style: Default::default(),
            // PureTerm(GAP-63):kitty 键盘协议默认开启。Agent CLI
            // (Claude Code 的 Shift+Enter 换行、Codex 的 Alt 组合键)
            // 依赖 disambiguate 模式区分换行与提交。协议由程序按需
            // push/pop,不支持它的程序不受影响;仍可用
            // `[terminal] kitty_keyboard = false` 关闭。
            kitty_keyboard: true,
            osc52: Default::default(),
        }
    }
}

/// OSC 52 行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(rename_all = "lowercase"))]
pub enum Osc52 {
    /// 禁用该转义序列的处理。
    Disabled,
    /// 只接受复制(写入剪贴板)序列。
    ///
    /// 此选项为默认值,是「完全禁用」(最安全)
    /// 与「允许粘贴」(安全性较低)之间的折中。
    #[default]
    OnlyCopy,
    /// 只接受粘贴(读取剪贴板)序列。
    OnlyPaste,
    /// 两者都接受。
    CopyPaste,
}

impl<T> Term<T> {
    #[inline]
    pub fn scroll_display(&mut self, scroll: Scroll)
    where
        T: EventListener,
    {
        let old_display_offset = self.grid.display_offset();
        self.grid.scroll_display(scroll);
        self.event_proxy.send_event(Event::MouseCursorDirty);

        // 把 Vi 模式光标约束在视口内。
        let viewport_start = -(self.grid.display_offset() as i32);
        let viewport_end = viewport_start + self.bottommost_line().0;
        let vi_cursor_line = &mut self.vi_mode_cursor.point.line.0;
        *vi_cursor_line = cmp::min(viewport_end, cmp::max(viewport_start, *vi_cursor_line));
        self.vi_mode_recompute_selection();

        // 显示偏移变化时整体标记受损。
        if old_display_offset != self.grid().display_offset() {
            self.mark_fully_damaged();
        }
    }

    pub fn new<D: Dimensions>(config: Config, dimensions: &D, event_proxy: T) -> Term<T> {
        let num_cols = dimensions.columns();
        let num_lines = dimensions.screen_lines();

        let history_size = config.scrolling_history;
        let grid = Grid::new(num_lines, num_cols, history_size);
        let inactive_grid = Grid::new(num_lines, num_cols, 0);

        let tabs = TabStops::new(grid.columns());

        let scroll_region = Line(0)..Line(grid.screen_lines() as i32);

        // 初始化终端受损状态,启动时覆盖整个终端。
        let damage = TermDamageState::new(num_cols, num_lines);

        Term {
            inactive_grid,
            scroll_region,
            event_proxy,
            damage,
            config,
            grid,
            tabs,
            inactive_keyboard_mode_stack: Default::default(),
            keyboard_mode_stack: Default::default(),
            active_charset: Default::default(),
            vi_mode_cursor: Default::default(),
            cursor_style: Default::default(),
            colors: color::Colors::default(),
            title_stack: Default::default(),
            is_focused: Default::default(),
            selection: Default::default(),
            smart_selection: Default::default(),
            title: Default::default(),
            mode: Default::default(),
            conformance_level: ConformanceLevel::default(),
        }
    }

    /// GAP-1.2: 当前 DECSCL 一致性级别。
    #[inline]
    pub fn conformance_level(&self) -> ConformanceLevel {
        self.conformance_level
    }

    /// GAP-1.2: 从 CSI Ps " p 应用 DECSCL 级别。
    #[inline]
    pub fn set_conformance_level(&mut self, level: ConformanceLevel) {
        self.conformance_level = level;
    }

    /// 收集各行变化的信息,
    /// 用于尽量减少重绘操作量。
    ///
    /// 用户可控元素(如 `Vi` 模式光标与 `Selection`)
    /// **不**属于收集的受损状态;相邻帧之间比较
    /// 新旧值即可轻松跟踪它们。
    ///
    /// 读取受损信息后应调用 [`reset_damage`]。
    ///
    /// [`reset_damage`]: 见 [`Self::reset_damage`]。
    #[must_use]
    pub fn damage(&mut self) -> TermDamage<'_> {
        // 进入插入模式后确保整个终端处于受损状态。
        // 退出插入模式在 ansi handler 中处理。
        if self.mode.contains(TermMode::INSERT) {
            self.mark_fully_damaged();
        }

        let previous_cursor = mem::replace(&mut self.damage.last_cursor, self.grid.cursor.point);

        if self.damage.full {
            return TermDamage::Full;
        }

        // 若新旧光标位置不同,补充两者的受损信息,
        // 以覆盖 `Term::input` 产生的所有变化。
        if self.damage.last_cursor != previous_cursor {
            // 即使有 `display_offset`,光标坐标也总在视口内。
            let point = Point::new(previous_cursor.line.0 as usize, previous_cursor.column);
            self.damage.damage_point(point);
        }

        // 始终标记当前光标受损。
        self.damage_cursor();

        // 注意:显示偏移非零时改变全部内容
        // 的受损(如滚动)通过整体受损处理。
        let display_offset = self.grid().display_offset();
        TermDamage::Partial(TermDamageIterator::new(&self.damage.lines, display_offset))
    }

    /// 重置终端受损信息。
    pub fn reset_damage(&mut self) {
        self.damage.reset(self.columns());
    }

    #[inline]
    fn mark_fully_damaged(&mut self) {
        self.damage.full = true;
    }

    /// 为 [`Term`] 设置新的配置项。
    pub fn set_options(&mut self, options: Config)
    where
        T: EventListener,
    {
        let old_config = mem::replace(&mut self.config, options);

        let title_event = match &self.title {
            Some(title) => Event::Title(title.clone()),
            None => Event::ResetTitle,
        };

        self.event_proxy.send_event(title_event);

        if self.mode.contains(TermMode::ALT_SCREEN) {
            self.inactive_grid.update_history(self.config.scrolling_history);
        } else {
            self.grid.update_history(self.config.scrolling_history);
        }

        if self.config.kitty_keyboard != old_config.kitty_keyboard {
            self.keyboard_mode_stack = Vec::new();
            self.inactive_keyboard_mode_stack = Vec::new();
            self.mode.remove(TermMode::KITTY_KEYBOARD_PROTOCOL);
        }

        // 配置更新时整体标记受损。
        self.mark_fully_damaged();
    }

    /// 把活动选区转换为字符串。
    pub fn selection_to_string(&self) -> Option<String> {
        let selection_range = self.selection.as_ref().and_then(|s| s.to_range(self))?;
        let SelectionRange { start, end, .. } = selection_range;

        let mut res = String::new();

        match self.selection.as_ref() {
            Some(Selection { ty: SelectionType::Block, .. }) => {
                for line in (start.line.0..end.line.0).map(Line::from) {
                    res += self
                        .line_to_string(line, start.column..end.column, start.column.0 != 0)
                        .trim_end();
                    res += "\n";
                }

                res += self.line_to_string(end.line, start.column..end.column, true).trim_end();
            },
            Some(Selection { ty: SelectionType::Lines, .. }) => {
                res = self.bounds_to_string(start, end) + "\n";
            },
            _ => {
                res = self.bounds_to_string(start, end);
            },
        }

        Some(res)
    }

    /// 把两点之间的范围转换为字符串。
    pub fn bounds_to_string(&self, start: Point, end: Point) -> String {
        let mut res = String::new();

        for line in (start.line.0..=end.line.0).map(Line::from) {
            let start_col = if line == start.line { start.column } else { Column(0) };
            let end_col = if line == end.line { end.column } else { self.last_column() };

            res += &self.line_to_string(line, start_col..end_col, line == end.line);
        }

        res.strip_suffix('\n').map(str::to_owned).unwrap_or(res)
    }

    /// 把网格中的单行转换为字符串。
    fn line_to_string(
        &self,
        line: Line,
        mut cols: Range<Column>,
        include_wrapped_wide: bool,
    ) -> String {
        let mut text = String::new();

        let grid_line = &self.grid[line];
        let line_length = cmp::min(grid_line.line_length(), cols.end + 1);

        // 当尾部占位被选中时,把宽字符也包含进来。
        if grid_line[cols.start].flags.contains(Flags::WIDE_CHAR_SPACER) {
            cols.start -= 1;
        }

        let mut tab_mode = false;
        for column in (cols.start.0..line_length.0).map(Column::from) {
            let cell = &grid_line[column];

            // 遇到制表符后,跳过单元直到下一个制表位。
            if tab_mode {
                if self.tabs[column] || cell.c != ' ' {
                    tab_mode = false;
                } else {
                    continue;
                }
            }

            if cell.c == '\t' {
                tab_mode = true;
            }

            if !cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                // 压入单元的主字符。
                text.push(cell.c);

                // 压入零宽字符。
                for c in cell.zerowidth().into_iter().flatten() {
                    text.push(*c);
                }
            }
        }

        if cols.end >= self.columns() - 1
            && (line_length.0 == 0
                || !self.grid[line][line_length - 1].flags.contains(Flags::WRAPLINE))
        {
            text.push('\n');
        }

        // 若宽字符不在选区内但其前置占位在选区内,也包含它。
        if line_length == self.columns()
            && line_length.0 >= 2
            && grid_line[line_length - 1].flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            && include_wrapped_wide
        {
            text.push(self.grid[line - 1i32][Column(0)].c);
        }

        text
    }

    /// 渲染所需的终端内容。
    #[inline]
    pub fn renderable_content(&self) -> RenderableContent<'_>
    where
        T: EventListener,
    {
        RenderableContent::new(self)
    }

    /// 访问原始网格数据结构。
    pub fn grid(&self) -> &Grid<Cell> {
        &self.grid
    }

    /// 可变访问原始网格数据结构。
    pub fn grid_mut(&mut self) -> &mut Grid<Cell> {
        &mut self.grid
    }

    /// 把终端调整到新的尺寸。
    pub fn resize<S: Dimensions>(&mut self, size: S) {
        let old_cols = self.columns();
        let old_lines = self.screen_lines();

        let num_cols = size.columns();
        let num_lines = size.screen_lines();

        if old_cols == num_cols && old_lines == num_lines {
            debug!("Term::resize dimensions unchanged");
            return;
        }

        debug!("New num_cols is {num_cols} and num_lines is {num_lines}");

        // 让 Vi 模式光标随内容一起移动。
        let history_size = self.history_size();
        let mut delta = num_lines as i32 - old_lines as i32;
        let min_delta = cmp::min(0, num_lines as i32 - self.grid.cursor.point.line.0 - 1);
        delta = cmp::min(cmp::max(delta, min_delta), history_size as i32);
        self.vi_mode_cursor.point.line += delta;

        let is_alt = self.mode.contains(TermMode::ALT_SCREEN);
        self.grid.resize(!is_alt, num_lines, num_cols);
        self.inactive_grid.resize(is_alt, num_lines, num_cols);

        // 仅在必要时使选区与制表位失效。
        if old_cols != num_cols {
            self.selection = None;

            // 重建制表位列表。
            self.tabs.resize(num_cols);
        } else if let Some(selection) = self.selection.take() {
            let max_lines = cmp::max(num_lines, old_lines) as i32;
            let range = Line(0)..Line(max_lines);
            self.selection = selection.rotate(self, &range, -delta);
        }

        // 把 Vi 光标约束到视口内。
        let vi_point = self.vi_mode_cursor.point;
        let viewport_top = Line(-(self.grid.display_offset() as i32));
        let viewport_bottom = viewport_top + self.bottommost_line();
        self.vi_mode_cursor.point.line =
            cmp::max(cmp::min(vi_point.line, viewport_bottom), viewport_top);
        self.vi_mode_cursor.point.column = cmp::min(vi_point.column, self.last_column());

        // 重置滚动区域。
        self.scroll_region = Line(0)..Line(self.screen_lines() as i32);

        // 调整受损信息尺寸。
        self.damage.resize(num_cols, num_lines);
    }

    /// 活动的终端模式。
    #[inline]
    pub fn mode(&self) -> &TermMode {
        &self.mode
    }

    /// 交换主屏幕与备用屏幕缓冲区。
    pub fn swap_alt(&mut self) {
        if !self.mode.contains(TermMode::ALT_SCREEN) {
            // 把备用屏幕光标设为当前主屏幕光标。
            self.inactive_grid.cursor = self.grid.cursor.clone();

            // 丢弃主屏幕保存的光标信息。
            self.grid.saved_cursor = self.grid.cursor.clone();

            // 清空备用屏幕内容。
            self.inactive_grid.reset_region(..);
        }

        mem::swap(&mut self.keyboard_mode_stack, &mut self.inactive_keyboard_mode_stack);
        let keyboard_mode =
            self.keyboard_mode_stack.last().copied().unwrap_or(KeyboardModes::NO_MODE).into();
        self.set_keyboard_mode(keyboard_mode, KeyboardModesApplyBehavior::Replace);

        mem::swap(&mut self.grid, &mut self.inactive_grid);
        self.mode ^= TermMode::ALT_SCREEN;
        self.selection = None;
        self.mark_fully_damaged();
    }

    /// 屏幕向下滚动。
    ///
    /// 文本下移;底部清空。
    /// 要求起点位于滚动范围内。
    #[inline]
    fn scroll_down_relative(&mut self, origin: Line, mut lines: usize) {
        trace!("Scrolling down relative: origin={origin}, lines={lines}");

        lines = cmp::min(lines, (self.scroll_region.end - self.scroll_region.start).0 as usize);
        lines = cmp::min(lines, (self.scroll_region.end - origin).0 as usize);

        let region = origin..self.scroll_region.end;

        // 滚动选区。
        self.selection =
            self.selection.take().and_then(|s| s.rotate(self, &region, -(lines as i32)));

        // 滚动 Vi 模式光标。
        let line = &mut self.vi_mode_cursor.point.line;
        if region.start <= *line && region.end > *line {
            *line = cmp::min(*line + lines, region.end - 1);
        }

        // 在起点与底部之间滚动
        self.grid.scroll_down(&region, lines);
        self.mark_fully_damaged();
    }

    /// 屏幕向上滚动
    ///
    /// 文本上移;顶部清空。
    /// 要求起点位于滚动范围内。
    #[inline]
    fn scroll_up_relative(&mut self, origin: Line, mut lines: usize) {
        trace!("Scrolling up relative: origin={origin}, lines={lines}");

        lines = cmp::min(lines, (self.scroll_region.end - self.scroll_region.start).0 as usize);

        let region = origin..self.scroll_region.end;

        // 滚动选区。
        self.selection = self.selection.take().and_then(|s| s.rotate(self, &region, lines as i32));

        self.grid.scroll_up(&region, lines);

        // 滚动 Vi 模式光标。
        let viewport_top = Line(-(self.grid.display_offset() as i32));
        let top = if region.start == 0 { viewport_top } else { region.start };
        let line = &mut self.vi_mode_cursor.point.line;
        if (top <= *line) && region.end > *line {
            *line = cmp::max(*line - lines, top);
        }
        self.mark_fully_damaged();
    }

    fn deccolm(&mut self)
    where
        T: EventListener,
    {
        // 设置 132 列字体没有意义,但仍执行其余副作用。
        // 清空滚动区域。
        self.set_scrolling_region(1, None);

        // 清空网格。
        self.grid.reset_region(..);
        self.mark_fully_damaged();
    }

    #[inline]
    pub fn exit(&mut self)
    where
        T: EventListener,
    {
        self.event_proxy.send_event(Event::Exit);
    }

    /// 切换 Vi 模式。
    #[inline]
    pub fn toggle_vi_mode(&mut self)
    where
        T: EventListener,
    {
        self.mode ^= TermMode::VI;

        if self.mode.contains(TermMode::VI) {
            let display_offset = self.grid.display_offset() as i32;
            if self.grid.cursor.point.line > self.bottommost_line() - display_offset {
                // 若终端光标不可见,把光标移到左上角。
                let point = Point::new(Line(-display_offset), Column(0));
                self.vi_mode_cursor = ViModeCursor::new(point);
            } else {
                // 重置 Vi 模式光标位置以与主光标一致。
                self.vi_mode_cursor = ViModeCursor::new(self.grid.cursor.point);
            }
        }

        // 通知 UI 光标闪烁状态变化。
        self.event_proxy.send_event(Event::CursorBlinkingChange);
    }

    /// 移动 Vi 模式光标。
    #[inline]
    pub fn vi_motion(&mut self, motion: ViMotion)
    where
        T: EventListener,
    {
        // 要求 Vi 模式处于活动状态。
        if !self.mode.contains(TermMode::VI) {
            return;
        }

        // 移动光标。
        self.vi_mode_cursor = self.vi_mode_cursor.motion(self, motion);
        self.vi_mode_recompute_selection();
    }

    /// 把 Vi 光标移到网格中的某一点。
    #[inline]
    pub fn vi_goto_point(&mut self, point: Point)
    where
        T: EventListener,
    {
        // 移动视口使该点可见。
        self.scroll_to_point(point);

        // 把 Vi 光标移到该点。
        self.vi_mode_cursor.point = point;

        self.vi_mode_recompute_selection();
    }

    /// 更新活动选区以匹配 Vi 模式光标位置。
    #[inline]
    fn vi_mode_recompute_selection(&mut self) {
        // 要求 Vi 模式处于活动状态。
        if !self.mode.contains(TermMode::VI) {
            return;
        }

        // 仅当存在非空选区时更新。
        if let Some(selection) = self.selection.as_mut().filter(|s| !s.is_empty()) {
            selection.update(self.vi_mode_cursor.point, Side::Left);
            selection.include_all();
        }
    }

    /// 若点位于视口外,滚动显示区到该点。
    pub fn scroll_to_point(&mut self, point: Point)
    where
        T: EventListener,
    {
        let display_offset = self.grid.display_offset() as i32;
        let screen_lines = self.grid.screen_lines() as i32;

        if point.line < -display_offset {
            let lines = point.line + display_offset;
            self.scroll_display(Scroll::Delta(-lines.0));
        } else if point.line >= (screen_lines - display_offset) {
            let lines = point.line + display_offset - screen_lines + 1i32;
            self.scroll_display(Scroll::Delta(-lines.0));
        }
    }

    /// 跳到宽单元末尾。
    pub fn expand_wide(&self, mut point: Point, direction: Direction) -> Point {
        let flags = self.grid[point.line][point.column].flags;

        match direction {
            Direction::Right if flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) => {
                point.column = Column(1);
                point.line += 1;
            },
            Direction::Right if flags.contains(Flags::WIDE_CHAR) => {
                point.column = cmp::min(point.column + 1, self.last_column());
            },
            Direction::Left if flags.intersects(Flags::WIDE_CHAR | Flags::WIDE_CHAR_SPACER) => {
                if flags.contains(Flags::WIDE_CHAR_SPACER) {
                    point.column -= 1;
                }

                let prev = point.sub(self, Boundary::Grid, 1);
                if self.grid[prev].flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                    point = prev;
                }
            },
            _ => (),
        }

        point
    }

    #[inline]
    pub fn semantic_escape_chars(&self) -> &str {
        &self.config.semantic_escape_chars
    }

    #[cfg(test)]
    pub(crate) fn set_semantic_escape_chars(&mut self, semantic_escape_chars: &str) {
        self.config.semantic_escape_chars = semantic_escape_chars.into();
    }

    /// 当前活动的终端光标样式。
    ///
    /// Vi 模式活动期间,自动返回 Vi 模式光标样式。
    #[inline]
    pub fn cursor_style(&self) -> CursorStyle {
        let cursor_style = self.cursor_style.unwrap_or(self.config.default_cursor_style);

        if self.mode.contains(TermMode::VI) {
            self.config.vi_mode_cursor_style.unwrap_or(cursor_style)
        } else {
            cursor_style
        }
    }

    pub fn colors(&self) -> &Colors {
        &self.colors
    }

    /// 在当前光标位置插入换行。
    #[inline]
    fn wrapline(&mut self)
    where
        T: EventListener,
    {
        if !self.mode.contains(TermMode::LINE_WRAP) {
            return;
        }

        trace!("Wrapping input");

        self.grid.cursor_cell().flags.insert(Flags::WRAPLINE);

        if self.grid.cursor.point.line + 1 >= self.scroll_region.end {
            self.linefeed();
        } else {
            self.damage_cursor();
            self.grid.cursor.point.line += 1;
        }

        self.grid.cursor.point.column = Column(0);
        self.grid.cursor.input_needs_wrap = false;
        self.damage_cursor();
    }

    /// 把 `c` 写入光标位置的单元。
    #[inline(always)]
    fn write_at_cursor(&mut self, c: char) {
        let c = self.grid.cursor.charsets[self.active_charset].map(c);
        let fg = self.grid.cursor.template.fg;
        let bg = self.grid.cursor.template.bg;
        let flags = self.grid.cursor.template.flags;
        let extra = self.grid.cursor.template.extra.clone();

        let mut cursor_cell = self.grid.cursor_cell();

        // 覆写全角单元时清空所有相关单元。
        if cursor_cell.flags.intersects(Flags::WIDE_CHAR | Flags::WIDE_CHAR_SPACER) {
            // 移除宽字符及其占位。
            let wide = cursor_cell.flags.contains(Flags::WIDE_CHAR);
            let point = self.grid.cursor.point;
            if wide && point.column < self.last_column() {
                self.grid[point.line][point.column + 1].flags.remove(Flags::WIDE_CHAR_SPACER);
            } else if point.column > 0 {
                self.grid[point.line][point.column - 1].clear_wide();
            }

            // 移除前置占位。
            if point.column <= 1 && point.line != self.topmost_line() {
                let column = self.last_column();
                self.grid[point.line - 1i32][column].flags.remove(Flags::LEADING_WIDE_CHAR_SPACER);
            }

            cursor_cell = self.grid.cursor_cell();
        }

        cursor_cell.c = c;
        cursor_cell.fg = fg;
        cursor_cell.bg = bg;
        cursor_cell.flags = flags;
        cursor_cell.extra = extra;
    }

    #[inline]
    fn damage_cursor(&mut self) {
        // 普通光标坐标总在视口内。
        let point =
            Point::new(self.grid.cursor.point.line.0 as usize, self.grid.cursor.point.column);
        self.damage.damage_point(point);
    }

    #[inline]
    fn set_keyboard_mode(&mut self, mode: TermMode, apply: KeyboardModesApplyBehavior) {
        let active_mode = self.mode & TermMode::KITTY_KEYBOARD_PROTOCOL;
        self.mode &= !TermMode::KITTY_KEYBOARD_PROTOCOL;
        let new_mode = match apply {
            KeyboardModesApplyBehavior::Replace => mode,
            KeyboardModesApplyBehavior::Union => active_mode.union(mode),
            KeyboardModesApplyBehavior::Difference => active_mode.difference(mode),
        };
        trace!("Setting keyboard mode to {new_mode:?}");
        self.mode |= new_mode;
    }
}

impl<T> Dimensions for Term<T> {
    #[inline]
    fn columns(&self) -> usize {
        self.grid.columns()
    }

    #[inline]
    fn screen_lines(&self) -> usize {
        self.grid.screen_lines()
    }

    #[inline]
    fn total_lines(&self) -> usize {
        self.grid.total_lines()
    }
}

impl<T: EventListener> Handler for Term<T> {
    /// 一个待显示的字符。
    #[inline(never)]
    fn input(&mut self, c: char) {
        // 该字符将占用的单元数。
        let width = match c.width() {
            Some(width) => width,
            None => return,
        };

        // 处理零宽字符。
        if width == 0 {
            // 取前一列。
            let mut column = self.grid.cursor.point.column;
            if !self.grid.cursor.input_needs_wrap {
                column.0 = column.saturating_sub(1);
            }

            // 把零宽字符叠加在第一个全角字符单元上。
            let line = self.grid.cursor.point.line;
            if self.grid[line][column].flags.contains(Flags::WIDE_CHAR_SPACER) {
                column.0 = column.saturating_sub(1);
            }

            self.grid[line][column].push_zerowidth(c);
            return;
        }

        // 把光标移到下一行。
        if self.grid.cursor.input_needs_wrap {
            self.wrapline();
        }

        // 若处于插入模式,先把单元右移。
        let columns = self.columns();
        if self.mode.contains(TermMode::INSERT) && self.grid.cursor.point.column + width < columns {
            let line = self.grid.cursor.point.line;
            let col = self.grid.cursor.point.column;
            let row = &mut self.grid[line][..];

            for col in (col.0..(columns - width)).rev() {
                row.swap(col + width, col);
            }
        }

        if width == 1 {
            self.write_at_cursor(c);
        } else {
            if self.grid.cursor.point.column + 1 >= columns {
                if self.mode.contains(TermMode::LINE_WRAP) {
                    // 若字形放不进本行,在宽字符前插入占位。
                    self.grid.cursor.template.flags.insert(Flags::LEADING_WIDE_CHAR_SPACER);
                    self.write_at_cursor(' ');
                    self.grid.cursor.template.flags.remove(Flags::LEADING_WIDE_CHAR_SPACER);
                    self.wrapline();
                } else {
                    // 关闭自动换行时防止越界崩溃。
                    self.grid.cursor.input_needs_wrap = true;
                    return;
                }
            }

            // 把全宽字形写入当前光标单元。
            self.grid.cursor.template.flags.insert(Flags::WIDE_CHAR);
            self.write_at_cursor(c);
            self.grid.cursor.template.flags.remove(Flags::WIDE_CHAR);

            // 在宽字形后面的单元写入占位。
            self.grid.cursor.point.column += 1;
            self.grid.cursor.template.flags.insert(Flags::WIDE_CHAR_SPACER);
            self.write_at_cursor(' ');
            self.grid.cursor.template.flags.remove(Flags::WIDE_CHAR_SPACER);
        }

        if self.grid.cursor.point.column + 1 < columns {
            self.grid.cursor.point.column += 1;
        } else {
            self.grid.cursor.input_needs_wrap = true;
        }
    }

    #[inline]
    fn decaln(&mut self) {
        trace!("Decalnning");

        for line in (0..self.screen_lines()).map(Line::from) {
            for column in 0..self.columns() {
                let cell = &mut self.grid[line][Column(column)];
                *cell = Cell::default();
                cell.c = 'E';
            }
        }

        self.mark_fully_damaged();
    }

    #[inline]
    fn goto(&mut self, line: i32, col: usize) {
        let line = Line(line);
        let col = Column(col);

        trace!("Going to: line={line}, col={col}");
        let (y_offset, max_y) = if self.mode.contains(TermMode::ORIGIN) {
            (self.scroll_region.start, self.scroll_region.end - 1)
        } else {
            (Line(0), self.bottommost_line())
        };

        self.damage_cursor();
        self.grid.cursor.point.line = cmp::max(cmp::min(line + y_offset, max_y), Line(0));
        self.grid.cursor.point.column = cmp::min(col, self.last_column());
        self.damage_cursor();
        self.grid.cursor.input_needs_wrap = false;
    }

    #[inline]
    fn goto_line(&mut self, line: i32) {
        trace!("Going to line: {line}");
        self.goto(line, self.grid.cursor.point.column.0)
    }

    #[inline]
    fn goto_col(&mut self, col: usize) {
        trace!("Going to column: {col}");
        self.goto(self.grid.cursor.point.line.0, col)
    }

    #[inline]
    fn insert_blank(&mut self, count: usize) {
        let cursor = &self.grid.cursor;
        let bg = cursor.template.bg;

        // 确保插入操作在终端边界内
        let count = cmp::min(count, self.columns() - cursor.point.column.0);

        let source = cursor.point.column;
        let destination = cursor.point.column.0 + count;
        let num_cells = self.columns() - destination;

        let line = cursor.point.line;
        self.damage.damage_line(line.0 as usize, 0, self.columns() - 1);

        let row = &mut self.grid[line][..];

        for offset in (0..num_cells).rev() {
            row.swap(destination + offset, source.0 + offset);
        }

        // 单元刚被移向行尾;
        // 用空白填充源与目标之间的区域。
        for cell in &mut row[source.0..destination] {
            *cell = bg.into();
        }
    }

    #[inline]
    fn move_up(&mut self, lines: usize) {
        trace!("Moving up: {lines}");

        let line = self.grid.cursor.point.line - lines;
        let column = self.grid.cursor.point.column;
        self.goto(line.0, column.0)
    }

    #[inline]
    fn move_down(&mut self, lines: usize) {
        trace!("Moving down: {lines}");

        let line = self.grid.cursor.point.line + lines;
        let column = self.grid.cursor.point.column;
        self.goto(line.0, column.0)
    }

    #[inline]
    fn move_forward(&mut self, cols: usize) {
        trace!("Moving forward: {cols}");
        let last_column = cmp::min(self.grid.cursor.point.column + cols, self.last_column());

        let cursor_line = self.grid.cursor.point.line.0 as usize;
        self.damage.damage_line(cursor_line, self.grid.cursor.point.column.0, last_column.0);

        self.grid.cursor.point.column = last_column;
        self.grid.cursor.input_needs_wrap = false;
    }

    #[inline]
    fn move_backward(&mut self, cols: usize) {
        trace!("Moving backward: {cols}");
        let column = self.grid.cursor.point.column.saturating_sub(cols);

        let cursor_line = self.grid.cursor.point.line.0 as usize;
        self.damage.damage_line(cursor_line, column, self.grid.cursor.point.column.0);

        self.grid.cursor.point.column = Column(column);
        self.grid.cursor.input_needs_wrap = false;
    }

    #[inline]
    fn identify_terminal(&mut self, intermediate: Option<char>) {
        match intermediate {
            None => {
                trace!("Reporting primary device attributes");
                let text = String::from("\x1b[?6c");
                self.event_proxy.send_event(Event::PtyWrite(Arc::new(text)));
            },
            Some('>') => {
                trace!("Reporting secondary device attributes");
                // PureTerm(GAP-65):DA2 版本号改报 PureTerm 版本
                // 编码(version_number("0.1.0") = 100)。原样报告
                // alacritty 内核版本会被 Agent 探测脚本误判为
                // alacritty 宿主而启用其特有行为;主 DA1 `\x1b[?6c`
                // 保持 xterm 兼容不变。
                let version = version_number(crate::PURETERM_VERSION);
                // PureTerm: 避免 format 机制的额外分配;响应
                // 永远不超过 32 字节。
                let mut text = String::with_capacity(32);
                let _ = write!(text, "\x1b[>0;{version};1c");
                self.event_proxy.send_event(Event::PtyWrite(Arc::new(text)));
            },
            _ => debug!("Unsupported device attributes intermediate"),
        }
    }

    #[inline]
    fn report_keyboard_mode(&mut self) {
        if !self.config.kitty_keyboard {
            return;
        }

        trace!("Reporting active keyboard mode");
        let current_mode =
            self.keyboard_mode_stack.last().unwrap_or(&KeyboardModes::NO_MODE).bits();
        let mut text = String::with_capacity(16);
        let _ = write!(text, "\x1b[?{current_mode}u");
        self.event_proxy.send_event(Event::PtyWrite(Arc::new(text)));
    }

    #[inline]
    fn push_keyboard_mode(&mut self, mode: KeyboardModes) {
        if !self.config.kitty_keyboard {
            return;
        }

        trace!("Pushing `{mode:?}` keyboard mode into the stack");

        if self.keyboard_mode_stack.len() >= KEYBOARD_MODE_STACK_MAX_DEPTH {
            let removed = self.title_stack.remove(0);
            trace!(
                "Removing '{removed:?}' from bottom of keyboard mode stack that exceeds its \
                 maximum depth"
            );
        }

        self.keyboard_mode_stack.push(mode);
        self.set_keyboard_mode(mode.into(), KeyboardModesApplyBehavior::Replace);
    }

    #[inline]
    fn pop_keyboard_modes(&mut self, to_pop: u16) {
        if !self.config.kitty_keyboard {
            return;
        }

        trace!("Attempting to pop {to_pop} keyboard modes from the stack");
        let new_len = self.keyboard_mode_stack.len().saturating_sub(to_pop as usize);
        self.keyboard_mode_stack.truncate(new_len);

        // 重新加载活动模式。
        let mode = self.keyboard_mode_stack.last().copied().unwrap_or(KeyboardModes::NO_MODE);
        self.set_keyboard_mode(mode.into(), KeyboardModesApplyBehavior::Replace);
    }

    #[inline]
    fn set_keyboard_mode(&mut self, mode: KeyboardModes, apply: KeyboardModesApplyBehavior) {
        if !self.config.kitty_keyboard {
            return;
        }

        self.set_keyboard_mode(mode.into(), apply);
    }

    #[inline]
    fn device_status(&mut self, arg: usize) {
        trace!("Reporting device status: {arg}");
        match arg {
            5 => {
                let text = String::from("\x1b[0n");
                self.event_proxy.send_event(Event::PtyWrite(Arc::new(text)));
            },
            6 => {
                let pos = self.grid.cursor.point;
                let mut text = String::with_capacity(32);
                let _ = write!(text, "\x1b[{};{}R", pos.line + 1, pos.column + 1);
                self.event_proxy.send_event(Event::PtyWrite(Arc::new(text)));
            },
            _ => debug!("unknown device status query: {arg}"),
        };
    }

    #[inline]
    fn move_down_and_cr(&mut self, lines: usize) {
        trace!("Moving down and cr: {lines}");

        let line = self.grid.cursor.point.line + lines;
        self.goto(line.0, 0)
    }

    #[inline]
    fn move_up_and_cr(&mut self, lines: usize) {
        trace!("Moving up and cr: {lines}");

        let line = self.grid.cursor.point.line - lines;
        self.goto(line.0, 0)
    }

    /// 在光标位置插入制表符。
    #[inline]
    fn put_tab(&mut self, mut count: u16) {
        // 最后一列之后的制表符等同换行。
        if self.grid.cursor.input_needs_wrap {
            self.wrapline();
            return;
        }

        while self.grid.cursor.point.column < self.columns() && count != 0 {
            count -= 1;

            let c = self.grid.cursor.charsets[self.active_charset].map('\t');
            let cell = self.grid.cursor_cell();
            if cell.c == ' ' {
                cell.c = c;
            }

            loop {
                if (self.grid.cursor.point.column + 1) == self.columns() {
                    break;
                }

                self.grid.cursor.point.column += 1;

                if self.tabs[self.grid.cursor.point.column] {
                    break;
                }
            }
        }
    }

    /// 退格。
    #[inline]
    fn backspace(&mut self) {
        trace!("Backspace");

        if self.grid.cursor.point.column > Column(0) {
            let line = self.grid.cursor.point.line.0 as usize;
            let column = self.grid.cursor.point.column.0;
            self.grid.cursor.point.column -= 1;
            self.grid.cursor.input_needs_wrap = false;
            self.damage.damage_line(line, column - 1, column);
        }
    }

    /// 回车。
    #[inline]
    fn carriage_return(&mut self) {
        trace!("Carriage return");
        let new_col = 0;
        let line = self.grid.cursor.point.line.0 as usize;
        self.damage.damage_line(line, new_col, self.grid.cursor.point.column.0);
        self.grid.cursor.point.column = Column(new_col);
        self.grid.cursor.input_needs_wrap = false;
    }

    /// 换行。
    #[inline]
    fn linefeed(&mut self) {
        trace!("Linefeed");
        let next = self.grid.cursor.point.line + 1;
        if next == self.scroll_region.end {
            self.scroll_up(1);
        } else if next < self.screen_lines() {
            self.damage_cursor();
            self.grid.cursor.point.line += 1;
            self.damage_cursor();
        }
    }

    /// 把当前位置设为制表位。
    #[inline]
    fn bell(&mut self) {
        trace!("Bell");
        self.event_proxy.send_event(Event::Bell);
    }

    #[inline]
    fn substitute(&mut self) {
        trace!("[unimplemented] Substitute");
    }

    /// 执行 LF/NL(换行/新行)。
    ///
    /// LF/NL 模式有一段有趣的历史。根据 ECMA-48 第 4
    /// 版,在换行(LINE FEED)模式下:
    ///
    /// > 执行格式控制功能换行(LF)、换页(FF)、
    /// > 换行制表(VT)时,活动位置仅沿行推进方向
    /// > 移动。
    ///
    /// 在新行(NEW LINE)模式下:
    ///
    /// > 执行格式控制功能换行(LF)、换页(FF)、
    /// > 换行制表(VT)时,活动位置移动到下一行、
    /// > 下一页等处的行首位置;LF 的情况即所称的
    /// > 新行(NL)选项。
    ///
    /// 此外,ECMA-48 第 4 版说明该选项已被弃用;
    /// 第 5 版仅提及该选项(未作解释),
    /// 表示它已被移除。
    ///
    /// 作为仿真器,由于应用程序可能仍依赖它,
    /// 我们必须支持它。
    #[inline]
    fn newline(&mut self) {
        self.linefeed();

        if self.mode.contains(TermMode::LINE_FEED_NEW_LINE) {
            self.carriage_return();
        }
    }

    #[inline]
    fn set_horizontal_tabstop(&mut self) {
        trace!("Setting horizontal tabstop");
        self.tabs[self.grid.cursor.point.column] = true;
    }

    #[inline]
    fn scroll_up(&mut self, lines: usize) {
        let origin = self.scroll_region.start;
        self.scroll_up_relative(origin, lines);
    }

    #[inline]
    fn scroll_down(&mut self, lines: usize) {
        let origin = self.scroll_region.start;
        self.scroll_down_relative(origin, lines);
    }

    #[inline]
    fn insert_blank_lines(&mut self, lines: usize) {
        trace!("Inserting blank {lines} lines");

        let origin = self.grid.cursor.point.line;
        if self.scroll_region.contains(&origin) {
            self.scroll_down_relative(origin, lines);
        }
    }

    #[inline]
    fn delete_lines(&mut self, lines: usize) {
        let origin = self.grid.cursor.point.line;
        let lines = cmp::min(self.screen_lines() - origin.0 as usize, lines);

        trace!("Deleting {lines} lines");

        if lines > 0 && self.scroll_region.contains(&origin) {
            self.scroll_up_relative(origin, lines);
        }
    }

    #[inline]
    fn erase_chars(&mut self, count: usize) {
        let cursor = &self.grid.cursor;

        trace!("Erasing chars: count={}, col={}", count, cursor.point.column);

        let start = cursor.point.column;
        let end = cmp::min(start + count, Column(self.columns()));

        // 被清空的单元设置当前背景色。
        let bg = self.grid.cursor.template.bg;
        let line = cursor.point.line;
        self.damage.damage_line(line.0 as usize, start.0, end.0);
        let row = &mut self.grid[line];
        for cell in &mut row[start..end] {
            *cell = bg.into();
        }
    }

    #[inline]
    fn delete_chars(&mut self, count: usize) {
        let columns = self.columns();
        let cursor = &self.grid.cursor;
        let bg = cursor.template.bg;

        // 确保删除操作在终端边界内。
        let count = cmp::min(count, columns);

        let start = cursor.point.column.0;
        let end = cmp::min(start + count, columns - 1);
        let num_cells = columns - end;

        let line = cursor.point.line;
        self.damage.damage_line(line.0 as usize, 0, self.columns() - 1);
        let row = &mut self.grid[line][..];

        for offset in 0..num_cells {
            row.swap(start + offset, end + offset);
        }

        // 清空行尾的 `count` 个单元(删 1 个字符时
        // 即删 1 个单元)。
        let end = columns - count;
        for cell in &mut row[end..] {
            *cell = bg.into();
        }
    }

    #[inline]
    fn move_backward_tabs(&mut self, count: u16) {
        trace!("Moving backward {count} tabs");

        let old_col = self.grid.cursor.point.column.0;
        for _ in 0..count {
            let mut col = self.grid.cursor.point.column;

            if col == 0 {
                break;
            }

            for i in (0..(col.0)).rev() {
                if self.tabs[index::Column(i)] {
                    col = index::Column(i);
                    break;
                }
            }
            self.grid.cursor.point.column = col;
        }

        let line = self.grid.cursor.point.line.0 as usize;
        self.damage.damage_line(line, self.grid.cursor.point.column.0, old_col);
    }

    #[inline]
    fn move_forward_tabs(&mut self, count: u16) {
        trace!("Moving forward {count} tabs");

        let num_cols = self.columns();
        let old_col = self.grid.cursor.point.column.0;
        for _ in 0..count {
            let mut col = self.grid.cursor.point.column;

            if col == num_cols - 1 {
                break;
            }

            for i in col.0 + 1..num_cols {
                col = index::Column(i);
                if self.tabs[col] {
                    break;
                }
            }

            self.grid.cursor.point.column = col;
        }

        let line = self.grid.cursor.point.line.0 as usize;
        self.damage.damage_line(line, old_col, self.grid.cursor.point.column.0);
    }

    #[inline]
    fn save_cursor_position(&mut self) {
        trace!("Saving cursor position");

        self.grid.saved_cursor = self.grid.cursor.clone();
    }

    #[inline]
    fn restore_cursor_position(&mut self) {
        trace!("Restoring cursor position");

        self.damage_cursor();
        self.grid.cursor = self.grid.saved_cursor.clone();
        self.damage_cursor();
    }

    #[inline]
    fn clear_line(&mut self, mode: ansi::LineClearMode) {
        trace!("Clearing line: {mode:?}");

        let cursor = &self.grid.cursor;
        let bg = cursor.template.bg;
        let point = cursor.point;

        let (left, right) = match mode {
            ansi::LineClearMode::Right if cursor.input_needs_wrap => return,
            ansi::LineClearMode::Right => (point.column, Column(self.columns())),
            ansi::LineClearMode::Left => (Column(0), point.column + 1),
            ansi::LineClearMode::All => (Column(0), Column(self.columns())),
        };

        self.damage.damage_line(point.line.0 as usize, left.0, right.0 - 1);

        let row = &mut self.grid[point.line];
        for cell in &mut row[left..right] {
            *cell = bg.into();
        }

        let range = self.grid.cursor.point.line..=self.grid.cursor.point.line;
        self.selection = self.selection.take().filter(|s| !s.intersects_range(range));
    }

    /// 设置索引色的值。
    #[inline]
    fn set_color(&mut self, index: usize, color: Rgb) {
        trace!("Setting color[{index}] = {color:?}");

        // 颜色变化且不是光标时标记终端受损。
        if index != NamedColor::Cursor as usize && self.colors[index] != Some(color) {
            self.mark_fully_damaged();
        }

        self.colors[index] = Some(color);
    }

    /// 响应颜色查询转义序列。
    #[inline]
    fn dynamic_color_sequence(&mut self, prefix: String, index: usize, terminator: &str) {
        trace!("Requested write of escape sequence for color code {prefix}: color[{index}]");

        let terminator = terminator.to_owned();
        self.event_proxy.send_event(Event::ColorRequest(
            index,
            Arc::new(move |color| {
                let mut s = String::with_capacity(40);
                let _ = write!(
                    s,
                    "\x1b]{};rgb:{1:02x}{1:02x}/{2:02x}{2:02x}/{3:02x}{3:02x}{4}",
                    prefix, color.r, color.g, color.b, terminator
                );
                s
            }),
        ));
    }

    /// 把索引色重置为原始值。
    #[inline]
    fn reset_color(&mut self, index: usize) {
        trace!("Resetting color[{index}]");

        // 颜色变化且不是光标时标记终端受损。
        if index != NamedColor::Cursor as usize && self.colors[index].is_some() {
            self.mark_fully_damaged();
        }

        self.colors[index] = None;
    }

    /// 把数据存入剪贴板。
    #[inline]
    fn clipboard_store(&mut self, clipboard: u8, base64: &[u8]) {
        if !matches!(self.config.osc52, Osc52::OnlyCopy | Osc52::CopyPaste) {
            debug!("Denied osc52 store");
            return;
        }

        let clipboard_type = match clipboard {
            b'c' => ClipboardType::Clipboard,
            b'p' | b's' => ClipboardType::Selection,
            _ => return,
        };

        if let Ok(bytes) = Base64.decode(base64) {
            if let Ok(text) = String::from_utf8(bytes) {
                self.event_proxy.send_event(Event::ClipboardStore(clipboard_type, text));
            }
        }
    }

    /// 从剪贴板读取数据。
    #[inline]
    fn clipboard_load(&mut self, clipboard: u8, terminator: &str) {
        if !matches!(self.config.osc52, Osc52::OnlyPaste | Osc52::CopyPaste) {
            debug!("Denied osc52 load");
            return;
        }

        let clipboard_type = match clipboard {
            b'c' => ClipboardType::Clipboard,
            b'p' | b's' => ClipboardType::Selection,
            _ => return,
        };

        let terminator = terminator.to_owned();

        self.event_proxy.send_event(Event::ClipboardLoad(
            clipboard_type,
            Arc::new(move |text| {
                let base64 = Base64.encode(text);
                let mut s = String::with_capacity(base64.len() + 16 + terminator.len());
                let _ = write!(s, "\x1b]52;{};{}{}", clipboard as char, base64, terminator);
                s
            }),
        ));
    }

    #[inline]
    fn clear_screen(&mut self, mode: ansi::ClearMode) {
        trace!("Clearing screen: {mode:?}");
        let bg = self.grid.cursor.template.bg;

        let screen_lines = self.screen_lines();

        match mode {
            ansi::ClearMode::Above => {
                let cursor = self.grid.cursor.point;

                // 若清除多于一行。
                if cursor.line > 1 {
                    // 完全清空当前行之前的所有行。
                    self.grid.reset_region(..cursor.line);
                }

                // 清空当前行到当前列的位置。
                let end = cmp::min(cursor.column + 1, Column(self.columns()));
                for cell in &mut self.grid[cursor.line][..end] {
                    *cell = bg.into();
                }

                let range = Line(0)..=cursor.line;
                self.selection = self.selection.take().filter(|s| !s.intersects_range(range));
            },
            ansi::ClearMode::Below => {
                let cursor = self.grid.cursor.point;
                for cell in &mut self.grid[cursor.line][cursor.column..] {
                    *cell = bg.into();
                }

                if (cursor.line.0 as usize) < screen_lines - 1 {
                    self.grid.reset_region((cursor.line + 1)..);
                }

                let range = cursor.line..Line(screen_lines as i32);
                self.selection = self.selection.take().filter(|s| !s.intersects_range(range));
            },
            ansi::ClearMode::All => {
                if self.mode.contains(TermMode::ALT_SCREEN) {
                    self.grid.reset_region(..);
                } else {
                    let old_offset = self.grid.display_offset();

                    self.grid.clear_viewport();

                    // 计算清空视口所导致的滚动行数。
                    let lines = self.grid.display_offset().saturating_sub(old_offset);

                    self.vi_mode_cursor.point.line =
                        (self.vi_mode_cursor.point.line - lines).grid_clamp(self, Boundary::Grid);
                }

                self.selection = None;
            },
            ansi::ClearMode::Saved if self.history_size() > 0 => {
                self.grid.clear_history();

                self.vi_mode_cursor.point.line =
                    self.vi_mode_cursor.point.line.grid_clamp(self, Boundary::Cursor);

                self.selection = self.selection.take().filter(|s| !s.intersects_range(..Line(0)));
            },
            // 没有可清空的历史。
            ansi::ClearMode::Saved => (),
        }

        self.mark_fully_damaged();
    }

    #[inline]
    fn clear_tabs(&mut self, mode: ansi::TabulationClearMode) {
        trace!("Clearing tabs: {mode:?}");
        match mode {
            ansi::TabulationClearMode::Current => {
                self.tabs[self.grid.cursor.point.column] = false;
            },
            ansi::TabulationClearMode::All => {
                self.tabs.clear_all();
            },
        }
    }

    /// 重置 term 结构体中的所有重要字段。
    #[inline]
    fn reset_state(&mut self) {
        if self.mode.contains(TermMode::ALT_SCREEN) {
            mem::swap(&mut self.grid, &mut self.inactive_grid);
        }
        self.active_charset = Default::default();
        self.cursor_style = None;
        self.grid.reset();
        self.inactive_grid.reset();
        self.scroll_region = Line(0)..Line(self.screen_lines() as i32);
        self.tabs = TabStops::new(self.columns());
        self.title_stack = Vec::new();
        self.title = None;
        self.selection = None;
        self.vi_mode_cursor = Default::default();
        self.keyboard_mode_stack = Default::default();
        self.inactive_keyboard_mode_stack = Default::default();

        // 跨重置保留 Vi 模式。
        self.mode &= TermMode::VI;
        self.mode.insert(TermMode::default());

        self.event_proxy.send_event(Event::CursorBlinkingChange);
        self.mark_fully_damaged();
    }

    #[inline]
    fn reverse_index(&mut self) {
        trace!("Reversing index");
        // 若光标位于顶部。
        if self.grid.cursor.point.line == self.scroll_region.start {
            self.scroll_down(1);
        } else {
            self.damage_cursor();
            self.grid.cursor.point.line = cmp::max(self.grid.cursor.point.line - 1, Line(0));
            self.damage_cursor();
        }
    }

    #[inline]
    fn set_hyperlink(&mut self, hyperlink: Option<Hyperlink>) {
        trace!("Setting hyperlink: {hyperlink:?}");
        self.grid.cursor.template.set_hyperlink(hyperlink.map(|e| e.into()));
    }

    /// 设置终端属性。
    #[inline]
    fn terminal_attribute(&mut self, attr: Attr) {
        trace!("Setting attribute: {attr:?}");
        let cursor = &mut self.grid.cursor;
        match attr {
            Attr::Foreground(color) => cursor.template.fg = color,
            Attr::Background(color) => cursor.template.bg = color,
            Attr::UnderlineColor(color) => cursor.template.set_underline_color(color),
            Attr::Reset => {
                cursor.template.fg = Color::Named(NamedColor::Foreground);
                cursor.template.bg = Color::Named(NamedColor::Background);
                cursor.template.flags = Flags::empty();
                cursor.template.set_underline_color(None);
            },
            Attr::Reverse => cursor.template.flags.insert(Flags::INVERSE),
            Attr::CancelReverse => cursor.template.flags.remove(Flags::INVERSE),
            Attr::Bold => cursor.template.flags.insert(Flags::BOLD),
            Attr::CancelBold => cursor.template.flags.remove(Flags::BOLD),
            Attr::Dim => cursor.template.flags.insert(Flags::DIM),
            Attr::CancelBoldDim => cursor.template.flags.remove(Flags::BOLD | Flags::DIM),
            Attr::Italic => cursor.template.flags.insert(Flags::ITALIC),
            Attr::CancelItalic => cursor.template.flags.remove(Flags::ITALIC),
            Attr::Underline => {
                cursor.template.flags.remove(Flags::ALL_UNDERLINES);
                cursor.template.flags.insert(Flags::UNDERLINE);
            },
            Attr::DoubleUnderline => {
                cursor.template.flags.remove(Flags::ALL_UNDERLINES);
                cursor.template.flags.insert(Flags::DOUBLE_UNDERLINE);
            },
            Attr::Undercurl => {
                cursor.template.flags.remove(Flags::ALL_UNDERLINES);
                cursor.template.flags.insert(Flags::UNDERCURL);
            },
            Attr::DottedUnderline => {
                cursor.template.flags.remove(Flags::ALL_UNDERLINES);
                cursor.template.flags.insert(Flags::DOTTED_UNDERLINE);
            },
            Attr::DashedUnderline => {
                cursor.template.flags.remove(Flags::ALL_UNDERLINES);
                cursor.template.flags.insert(Flags::DASHED_UNDERLINE);
            },
            Attr::CancelUnderline => cursor.template.flags.remove(Flags::ALL_UNDERLINES),
            Attr::Hidden => cursor.template.flags.insert(Flags::HIDDEN),
            Attr::CancelHidden => cursor.template.flags.remove(Flags::HIDDEN),
            Attr::Strike => cursor.template.flags.insert(Flags::STRIKEOUT),
            Attr::CancelStrike => cursor.template.flags.remove(Flags::STRIKEOUT),
            _ => {
                debug!("Term got unhandled attr: {attr:?}");
            },
        }
    }

    #[inline]
    fn set_private_mode(&mut self, mode: PrivateMode) {
        let mode = match mode {
            PrivateMode::Named(mode) => mode,
            PrivateMode::Unknown(mode) => {
                debug!("Ignoring unknown mode {mode} in set_private_mode");
                return;
            },
        };

        trace!("Setting private mode: {mode:?}");
        match mode {
            NamedPrivateMode::UrgencyHints => self.mode.insert(TermMode::URGENCY_HINTS),
            NamedPrivateMode::SwapScreenAndSetRestoreCursor => {
                if !self.mode.contains(TermMode::ALT_SCREEN) {
                    self.swap_alt();
                }
            },
            NamedPrivateMode::ShowCursor => self.mode.insert(TermMode::SHOW_CURSOR),
            NamedPrivateMode::CursorKeys => self.mode.insert(TermMode::APP_CURSOR),
            // 鼠标协议互斥。
            NamedPrivateMode::ReportMouseClicks => {
                self.mode.remove(TermMode::MOUSE_MODE);
                self.mode.insert(TermMode::MOUSE_REPORT_CLICK);
                self.event_proxy.send_event(Event::MouseCursorDirty);
            },
            NamedPrivateMode::ReportCellMouseMotion => {
                self.mode.remove(TermMode::MOUSE_MODE);
                self.mode.insert(TermMode::MOUSE_DRAG);
                self.event_proxy.send_event(Event::MouseCursorDirty);
            },
            NamedPrivateMode::ReportAllMouseMotion => {
                self.mode.remove(TermMode::MOUSE_MODE);
                self.mode.insert(TermMode::MOUSE_MOTION);
                self.event_proxy.send_event(Event::MouseCursorDirty);
            },
            NamedPrivateMode::ReportFocusInOut => self.mode.insert(TermMode::FOCUS_IN_OUT),
            NamedPrivateMode::BracketedPaste => self.mode.insert(TermMode::BRACKETED_PASTE),
            // 鼠标编码互斥。
            NamedPrivateMode::SgrMouse => {
                self.mode.remove(TermMode::UTF8_MOUSE);
                self.mode.insert(TermMode::SGR_MOUSE);
            },
            NamedPrivateMode::Utf8Mouse => {
                self.mode.remove(TermMode::SGR_MOUSE);
                self.mode.insert(TermMode::UTF8_MOUSE);
            },
            NamedPrivateMode::AlternateScroll => self.mode.insert(TermMode::ALTERNATE_SCROLL),
            NamedPrivateMode::LineWrap => self.mode.insert(TermMode::LINE_WRAP),
            NamedPrivateMode::Origin => {
                self.mode.insert(TermMode::ORIGIN);
                self.goto(0, 0);
            },
            NamedPrivateMode::ColumnMode => self.deccolm(),
            NamedPrivateMode::BlinkingCursor => {
                let style = self.cursor_style.get_or_insert(self.config.default_cursor_style);
                style.blinking = true;
                self.event_proxy.send_event(Event::CursorBlinkingChange);
            },
            NamedPrivateMode::SyncUpdate => (),
        }
    }

    #[inline]
    fn unset_private_mode(&mut self, mode: PrivateMode) {
        let mode = match mode {
            PrivateMode::Named(mode) => mode,
            PrivateMode::Unknown(mode) => {
                debug!("Ignoring unknown mode {mode} in unset_private_mode");
                return;
            },
        };

        trace!("Unsetting private mode: {mode:?}");
        match mode {
            NamedPrivateMode::UrgencyHints => self.mode.remove(TermMode::URGENCY_HINTS),
            NamedPrivateMode::SwapScreenAndSetRestoreCursor => {
                if self.mode.contains(TermMode::ALT_SCREEN) {
                    self.swap_alt();
                }
            },
            NamedPrivateMode::ShowCursor => self.mode.remove(TermMode::SHOW_CURSOR),
            NamedPrivateMode::CursorKeys => self.mode.remove(TermMode::APP_CURSOR),
            NamedPrivateMode::ReportMouseClicks => {
                self.mode.remove(TermMode::MOUSE_REPORT_CLICK);
                self.event_proxy.send_event(Event::MouseCursorDirty);
            },
            NamedPrivateMode::ReportCellMouseMotion => {
                self.mode.remove(TermMode::MOUSE_DRAG);
                self.event_proxy.send_event(Event::MouseCursorDirty);
            },
            NamedPrivateMode::ReportAllMouseMotion => {
                self.mode.remove(TermMode::MOUSE_MOTION);
                self.event_proxy.send_event(Event::MouseCursorDirty);
            },
            NamedPrivateMode::ReportFocusInOut => self.mode.remove(TermMode::FOCUS_IN_OUT),
            NamedPrivateMode::BracketedPaste => self.mode.remove(TermMode::BRACKETED_PASTE),
            NamedPrivateMode::SgrMouse => self.mode.remove(TermMode::SGR_MOUSE),
            NamedPrivateMode::Utf8Mouse => self.mode.remove(TermMode::UTF8_MOUSE),
            NamedPrivateMode::AlternateScroll => self.mode.remove(TermMode::ALTERNATE_SCROLL),
            NamedPrivateMode::LineWrap => self.mode.remove(TermMode::LINE_WRAP),
            NamedPrivateMode::Origin => self.mode.remove(TermMode::ORIGIN),
            NamedPrivateMode::ColumnMode => self.deccolm(),
            NamedPrivateMode::BlinkingCursor => {
                let style = self.cursor_style.get_or_insert(self.config.default_cursor_style);
                style.blinking = false;
                self.event_proxy.send_event(Event::CursorBlinkingChange);
            },
            NamedPrivateMode::SyncUpdate => (),
        }
    }

    #[inline]
    fn report_private_mode(&mut self, mode: PrivateMode) {
        trace!("Reporting private mode {mode:?}");
        let state = match mode {
            PrivateMode::Named(mode) => match mode {
                NamedPrivateMode::CursorKeys => self.mode.contains(TermMode::APP_CURSOR).into(),
                NamedPrivateMode::Origin => self.mode.contains(TermMode::ORIGIN).into(),
                NamedPrivateMode::LineWrap => self.mode.contains(TermMode::LINE_WRAP).into(),
                NamedPrivateMode::BlinkingCursor => {
                    let style = self.cursor_style.get_or_insert(self.config.default_cursor_style);
                    style.blinking.into()
                },
                NamedPrivateMode::ShowCursor => self.mode.contains(TermMode::SHOW_CURSOR).into(),
                NamedPrivateMode::ReportMouseClicks => {
                    self.mode.contains(TermMode::MOUSE_REPORT_CLICK).into()
                },
                NamedPrivateMode::ReportCellMouseMotion => {
                    self.mode.contains(TermMode::MOUSE_DRAG).into()
                },
                NamedPrivateMode::ReportAllMouseMotion => {
                    self.mode.contains(TermMode::MOUSE_MOTION).into()
                },
                NamedPrivateMode::ReportFocusInOut => {
                    self.mode.contains(TermMode::FOCUS_IN_OUT).into()
                },
                NamedPrivateMode::Utf8Mouse => self.mode.contains(TermMode::UTF8_MOUSE).into(),
                NamedPrivateMode::SgrMouse => self.mode.contains(TermMode::SGR_MOUSE).into(),
                NamedPrivateMode::AlternateScroll => {
                    self.mode.contains(TermMode::ALTERNATE_SCROLL).into()
                },
                NamedPrivateMode::UrgencyHints => {
                    self.mode.contains(TermMode::URGENCY_HINTS).into()
                },
                NamedPrivateMode::SwapScreenAndSetRestoreCursor => {
                    self.mode.contains(TermMode::ALT_SCREEN).into()
                },
                NamedPrivateMode::BracketedPaste => {
                    self.mode.contains(TermMode::BRACKETED_PASTE).into()
                },
                NamedPrivateMode::SyncUpdate => ModeState::Reset,
                NamedPrivateMode::ColumnMode => ModeState::NotSupported,
            },
            PrivateMode::Unknown(_) => ModeState::NotSupported,
        };

        let mut text = String::with_capacity(24);
        let _ = write!(text, "\x1b[?{};{}$y", mode.raw(), state as u8);
        self.event_proxy.send_event(Event::PtyWrite(Arc::new(text)));
    }

    #[inline]
    fn set_mode(&mut self, mode: ansi::Mode) {
        let mode = match mode {
            ansi::Mode::Named(mode) => mode,
            ansi::Mode::Unknown(mode) => {
                debug!("Ignoring unknown mode {mode} in set_mode");
                return;
            },
        };

        trace!("Setting public mode: {mode:?}");
        match mode {
            NamedMode::Insert => self.mode.insert(TermMode::INSERT),
            NamedMode::LineFeedNewLine => self.mode.insert(TermMode::LINE_FEED_NEW_LINE),
        }
    }

    #[inline]
    fn unset_mode(&mut self, mode: ansi::Mode) {
        let mode = match mode {
            ansi::Mode::Named(mode) => mode,
            ansi::Mode::Unknown(mode) => {
                debug!("Ignoring unknown mode {mode} in unset_mode");
                return;
            },
        };

        trace!("Setting public mode: {mode:?}");
        match mode {
            NamedMode::Insert => {
                self.mode.remove(TermMode::INSERT);
                self.mark_fully_damaged();
            },
            NamedMode::LineFeedNewLine => self.mode.remove(TermMode::LINE_FEED_NEW_LINE),
        }
    }

    #[inline]
    fn report_mode(&mut self, mode: ansi::Mode) {
        trace!("Reporting mode {mode:?}");
        let state = match mode {
            ansi::Mode::Named(mode) => match mode {
                NamedMode::Insert => self.mode.contains(TermMode::INSERT).into(),
                NamedMode::LineFeedNewLine => {
                    self.mode.contains(TermMode::LINE_FEED_NEW_LINE).into()
                },
            },
            ansi::Mode::Unknown(_) => ModeState::NotSupported,
        };

        let mut text = String::with_capacity(24);
        let _ = write!(text, "\x1b[{};{}$y", mode.raw(), state as u8);
        self.event_proxy.send_event(Event::PtyWrite(Arc::new(text)));
    }

    #[inline]
    fn set_scrolling_region(&mut self, top: usize, bottom: Option<usize>) {
        // 缺省回退到最后一行。
        let bottom = bottom.unwrap_or_else(|| self.screen_lines());

        if top >= bottom {
            debug!("Invalid scrolling region: ({top};{bottom})");
            return;
        }

        // 底边界应包含在范围内,但范围终点通常
        // 不包含在内。一种做法是使用闭区间,
        // 但这里直接让半开区间终点高 1,
        // 以包含底边界。
        let start = Line(top as i32 - 1);
        let end = Line(bottom as i32);

        trace!("Setting scrolling region: ({start};{end})");

        let screen_lines = Line(self.screen_lines() as i32);
        self.scroll_region.start = cmp::min(start, screen_lines);
        self.scroll_region.end = cmp::min(end, screen_lines);
        self.goto(0, 0);
    }

    #[inline]
    fn set_keypad_application_mode(&mut self) {
        trace!("Setting keypad application mode");
        self.mode.insert(TermMode::APP_KEYPAD);
    }

    #[inline]
    fn unset_keypad_application_mode(&mut self) {
        trace!("Unsetting keypad application mode");
        self.mode.remove(TermMode::APP_KEYPAD);
    }

    #[inline]
    fn configure_charset(&mut self, index: CharsetIndex, charset: StandardCharset) {
        trace!("Configuring charset {index:?} as {charset:?}");
        self.grid.cursor.charsets[index] = charset;
    }

    #[inline]
    fn set_active_charset(&mut self, index: CharsetIndex) {
        trace!("Setting active charset {index:?}");
        self.active_charset = index;
    }

    #[inline]
    fn set_cursor_style(&mut self, style: Option<CursorStyle>) {
        trace!("Setting cursor style {style:?}");
        self.cursor_style = style;

        // 通知 UI 闪烁状态变化。
        self.event_proxy.send_event(Event::CursorBlinkingChange);
    }

    #[inline]
    fn set_cursor_shape(&mut self, shape: CursorShape) {
        trace!("Setting cursor shape {shape:?}");

        let style = self.cursor_style.get_or_insert(self.config.default_cursor_style);
        style.shape = shape;
    }

    #[inline]
    fn set_title(&mut self, title: Option<String>) {
        trace!("Setting title to '{title:?}'");

        self.title.clone_from(&title);

        let title_event = match title {
            Some(title) => Event::Title(title),
            None => Event::ResetTitle,
        };

        self.event_proxy.send_event(title_event);
    }

    #[inline]
    fn push_title(&mut self) {
        trace!("Pushing '{:?}' onto title stack", self.title);

        if self.title_stack.len() >= TITLE_STACK_MAX_DEPTH {
            let removed = self.title_stack.remove(0);
            trace!(
                "Removing '{removed:?}' from bottom of title stack that exceeds its maximum depth"
            );
        }

        self.title_stack.push(self.title.clone());
    }

    #[inline]
    fn pop_title(&mut self) {
        trace!("Attempting to pop title from stack...");

        if let Some(popped) = self.title_stack.pop() {
            trace!("Title '{popped:?}' popped from stack");
            self.set_title(popped);
        }
    }

    #[inline]
    fn text_area_size_pixels(&mut self) {
        self.event_proxy.send_event(Event::TextAreaSizeRequest(Arc::new(move |window_size| {
            let height = window_size.num_lines * window_size.cell_height;
            let width = window_size.num_cols * window_size.cell_width;
            let mut s = String::with_capacity(32);
            let _ = write!(s, "\x1b[4;{height};{width}t");
            s
        })));
    }

    #[inline]
    fn text_area_size_chars(&mut self) {
        let mut text = String::with_capacity(32);
        let _ = write!(text, "\x1b[8;{};{}t", self.screen_lines(), self.columns());
        self.event_proxy.send_event(Event::PtyWrite(Arc::new(text)));
    }
}

/// [`Mode`] 与 [`PrivateMode`] 的状态。
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
enum ModeState {
    /// 模式不受支持。
    NotSupported = 0,
    /// 模式当前已设置。
    Set = 1,
    /// 模式当前未设置。
    Reset = 2,
}

impl From<bool> for ModeState {
    fn from(value: bool) -> Self {
        if value { Self::Set } else { Self::Reset }
    }
}

/// 转义序列报告使用的终端版本号。
///
/// 基于 PureTerm 终端 crate 的 semver 版本号,
/// 返回当前终端版本对应的唯一数字;各位数字
/// 经填充,确保更高的 semver 版本总是报告更大的数字。
fn version_number(mut version: &str) -> usize {
    if let Some(separator) = version.rfind('-') {
        version = &version[..separator];
    }

    let mut version_number = 0;

    let semver_versions = version.split('.');
    for (i, semver_version) in semver_versions.rev().enumerate() {
        let semver_number = semver_version.parse::<usize>().unwrap_or(0);
        version_number += usize::pow(100, i as u32) * semver_number;
    }

    version_number
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardType {
    Clipboard,
    Selection,
}

struct TabStops {
    tabs: Vec<bool>,
}

impl TabStops {
    #[inline]
    fn new(columns: usize) -> TabStops {
        TabStops { tabs: (0..columns).map(|i| i % INITIAL_TABSTOPS == 0).collect() }
    }

    /// 移除所有制表位。
    #[inline]
    fn clear_all(&mut self) {
        unsafe {
            ptr::write_bytes(self.tabs.as_mut_ptr(), 0, self.tabs.len());
        }
    }

    /// 扩展制表位容量。
    #[inline]
    fn resize(&mut self, columns: usize) {
        let mut index = self.tabs.len();
        self.tabs.resize_with(columns, || {
            let is_tabstop = index % INITIAL_TABSTOPS == 0;
            index += 1;
            is_tabstop
        });
    }
}

impl Index<Column> for TabStops {
    type Output = bool;

    fn index(&self, index: Column) -> &bool {
        &self.tabs[index.0]
    }
}

impl IndexMut<Column> for TabStops {
    fn index_mut(&mut self, index: Column) -> &mut bool {
        self.tabs.index_mut(index.0)
    }
}

/// 终端光标渲染信息。
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct RenderableCursor {
    pub shape: CursorShape,
    pub point: Point,
}

impl RenderableCursor {
    fn new<T>(term: &Term<T>) -> Self {
        // 光标位置。
        let vi_mode = term.mode().contains(TermMode::VI);
        let mut point = if vi_mode { term.vi_mode_cursor.point } else { term.grid.cursor.point };
        if term.grid[point].flags.contains(Flags::WIDE_CHAR_SPACER) {
            point.column -= 1;
        }

        // 光标形状。
        let shape = if !vi_mode && !term.mode().contains(TermMode::SHOW_CURSOR) {
            CursorShape::Hidden
        } else {
            term.cursor_style().shape
        };

        Self { shape, point }
    }
}

/// 可见的终端内容。
///
/// 包含渲染当前终端视图所需的全部内容。
pub struct RenderableContent<'a> {
    pub display_iter: GridIterator<'a, Cell>,
    pub selection: Option<SelectionRange>,
    pub cursor: RenderableCursor,
    pub display_offset: usize,
    pub colors: &'a color::Colors,
    pub mode: TermMode,
}

impl<'a> RenderableContent<'a> {
    fn new<T>(term: &'a Term<T>) -> Self {
        Self {
            display_iter: term.grid().display_iter(),
            display_offset: term.grid().display_offset(),
            cursor: RenderableCursor::new(term),
            selection: term.selection.as_ref().and_then(|s| s.to_range(term)),
            colors: &term.colors,
            mode: *term.mode(),
        }
    }
}

/// 终端测试辅助工具。
pub mod test {
    use super::*;

    #[cfg(feature = "serde")]
    use serde::{Deserialize, Serialize};

    use crate::event::VoidListener;

    #[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
    pub struct TermSize {
        pub columns: usize,
        pub screen_lines: usize,
    }

    impl TermSize {
        pub fn new(columns: usize, screen_lines: usize) -> Self {
            Self { columns, screen_lines }
        }
    }

    impl Dimensions for TermSize {
        fn total_lines(&self) -> usize {
            self.screen_lines()
        }

        fn screen_lines(&self) -> usize {
            self.screen_lines
        }

        fn columns(&self) -> usize {
            self.columns
        }
    }

    /// 从内容字符串构造一个终端。
    ///
    /// `\n` 会换行,`\r\n` 会换行但不设置换行标志。
    ///
    /// # 示例
    ///
    /// ```rust
    /// use alacritty_terminal::term::test::mock_term;
    ///
    /// // Create a terminal with the following cells:
    /// //
    /// // [h][e][l][l][o] <- WRAPLINE flag set
    /// // [:][)][ ][ ][ ]
    /// // [t][e][s][t][ ]
    /// mock_term(
    ///     "\
    ///     hello\n:)\r\ntest",
    /// );
    /// ```
    pub fn mock_term(content: &str) -> Term<VoidListener> {
        let lines: Vec<&str> = content.split('\n').collect();
        let num_cols = lines
            .iter()
            .map(|line| line.chars().filter(|c| *c != '\r').map(|c| c.width().unwrap()).sum())
            .max()
            .unwrap_or(0);

        // 按合适的尺寸创建终端。
        let size = TermSize::new(num_cols, lines.len());
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 用内容填充终端。
        for (line, text) in lines.iter().enumerate() {
            let line = Line(line as i32);
            if !text.ends_with('\r') && line + 1 != lines.len() {
                term.grid[line][Column(num_cols - 1)].flags.insert(Flags::WRAPLINE);
            }

            let mut index = 0;
            for c in text.chars().take_while(|c| *c != '\r') {
                term.grid[line][Column(index)].c = c;

                // 处理全角字符。
                let width = c.width().unwrap();
                if width == 2 {
                    term.grid[line][Column(index)].flags.insert(Flags::WIDE_CHAR);
                    term.grid[line][Column(index + 1)].flags.insert(Flags::WIDE_CHAR_SPACER);
                }

                index += width;
            }
        }

        term
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::mem;

    use crate::event::VoidListener;
    use crate::grid::{Grid, Scroll};
    use crate::index::{Column, Point, Side};
    use crate::selection::{Selection, SelectionType};
    use crate::term::cell::{Cell, Flags};
    use crate::term::test::TermSize;
    use crate::vte::ansi::{self, CharsetIndex, Handler, StandardCharset};

    #[test]
    fn scroll_display_page_up() {
        let size = TermSize::new(5, 10);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 创建 11 行滚动历史。
        for _ in 0..20 {
            term.newline();
        }

        // 向上可滚动 11 行。
        term.scroll_display(Scroll::PageUp);
        assert_eq!(term.vi_mode_cursor.point, Point::new(Line(-1), Column(0)));
        assert_eq!(term.grid.display_offset(), 10);

        // 向上可滚动 1 行。
        term.scroll_display(Scroll::PageUp);
        assert_eq!(term.vi_mode_cursor.point, Point::new(Line(-2), Column(0)));
        assert_eq!(term.grid.display_offset(), 11);

        // 向上可滚动 0 行。
        term.scroll_display(Scroll::PageUp);
        assert_eq!(term.vi_mode_cursor.point, Point::new(Line(-2), Column(0)));
        assert_eq!(term.grid.display_offset(), 11);
    }

    #[test]
    fn scroll_display_page_down() {
        let size = TermSize::new(5, 10);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 创建 11 行滚动历史。
        for _ in 0..20 {
            term.newline();
        }

        // 把 display_offset 改为最顶部。
        term.grid_mut().scroll_display(Scroll::Top);
        term.vi_mode_cursor = ViModeCursor::new(Point::new(Line(-11), Column(0)));

        // 向下可滚动 11 行。
        term.scroll_display(Scroll::PageDown);
        assert_eq!(term.vi_mode_cursor.point, Point::new(Line(-1), Column(0)));
        assert_eq!(term.grid.display_offset(), 1);

        // 向下可滚动 1 行。
        term.scroll_display(Scroll::PageDown);
        assert_eq!(term.vi_mode_cursor.point, Point::new(Line(0), Column(0)));
        assert_eq!(term.grid.display_offset(), 0);

        // 向下可滚动 0 行。
        term.scroll_display(Scroll::PageDown);
        assert_eq!(term.vi_mode_cursor.point, Point::new(Line(0), Column(0)));
        assert_eq!(term.grid.display_offset(), 0);
    }

    #[test]
    fn simple_selection_works() {
        let size = TermSize::new(5, 5);
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let grid = term.grid_mut();
        for i in 0..4 {
            if i == 1 {
                continue;
            }

            grid[Line(i)][Column(0)].c = '"';

            for j in 1..4 {
                grid[Line(i)][Column(j)].c = 'a';
            }

            grid[Line(i)][Column(4)].c = '"';
        }
        grid[Line(2)][Column(0)].c = ' ';
        grid[Line(2)][Column(4)].c = ' ';
        grid[Line(2)][Column(4)].flags.insert(Flags::WRAPLINE);
        grid[Line(3)][Column(0)].c = ' ';

        // 多行选区中包含空行。
        term.selection = Some(Selection::new(
            SelectionType::Simple,
            Point { line: Line(0), column: Column(0) },
            Side::Left,
        ));
        if let Some(s) = term.selection.as_mut() {
            s.update(Point { line: Line(2), column: Column(4) }, Side::Right);
        }
        assert_eq!(term.selection_to_string(), Some(String::from("\"aaa\"\n\n aaa ")));

        // 跨换行行的选区。
        term.selection = Some(Selection::new(
            SelectionType::Simple,
            Point { line: Line(2), column: Column(0) },
            Side::Left,
        ));
        if let Some(s) = term.selection.as_mut() {
            s.update(Point { line: Line(3), column: Column(4) }, Side::Right);
        }
        assert_eq!(term.selection_to_string(), Some(String::from(" aaa  aaa\"")));
    }

    #[test]
    fn semantic_selection_works() {
        let size = TermSize::new(5, 3);
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut grid: Grid<Cell> = Grid::new(3, 5, 0);
        for i in 0..5 {
            for j in 0..2 {
                grid[Line(j)][Column(i)].c = 'a';
            }
        }
        grid[Line(0)][Column(0)].c = '"';
        grid[Line(0)][Column(3)].c = '"';
        grid[Line(1)][Column(2)].c = '"';
        grid[Line(0)][Column(4)].flags.insert(Flags::WRAPLINE);

        let mut escape_chars = String::from("\"");

        mem::swap(&mut term.grid, &mut grid);
        mem::swap(&mut term.config.semantic_escape_chars, &mut escape_chars);

        {
            term.selection = Some(Selection::new(
                SelectionType::Semantic,
                Point { line: Line(0), column: Column(1) },
                Side::Left,
            ));
            // 智能选区匹配带引号的字符串 "aa",而非仅内部文本。
            assert_eq!(term.selection_to_string(), Some(String::from("\"aa\"")));
        }

        {
            term.selection = Some(Selection::new(
                SelectionType::Semantic,
                Point { line: Line(0), column: Column(4) },
                Side::Left,
            ));
            assert_eq!(term.selection_to_string(), Some(String::from("aaa")));
        }

        {
            term.selection = Some(Selection::new(
                SelectionType::Semantic,
                Point { line: Line(1), column: Column(1) },
                Side::Left,
            ));
            assert_eq!(term.selection_to_string(), Some(String::from("aaa")));
        }
    }

    #[test]
    fn line_selection_works() {
        let size = TermSize::new(5, 1);
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut grid: Grid<Cell> = Grid::new(1, 5, 0);
        for i in 0..5 {
            grid[Line(0)][Column(i)].c = 'a';
        }
        grid[Line(0)][Column(0)].c = '"';
        grid[Line(0)][Column(3)].c = '"';

        mem::swap(&mut term.grid, &mut grid);

        term.selection = Some(Selection::new(
            SelectionType::Lines,
            Point { line: Line(0), column: Column(3) },
            Side::Left,
        ));
        assert_eq!(term.selection_to_string(), Some(String::from("\"aa\"a\n")));
    }

    #[test]
    fn block_selection_works() {
        let size = TermSize::new(5, 5);
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let grid = term.grid_mut();
        for i in 1..4 {
            grid[Line(i)][Column(0)].c = '"';

            for j in 1..4 {
                grid[Line(i)][Column(j)].c = 'a';
            }

            grid[Line(i)][Column(4)].c = '"';
        }
        grid[Line(2)][Column(2)].c = ' ';
        grid[Line(2)][Column(4)].flags.insert(Flags::WRAPLINE);
        grid[Line(3)][Column(4)].c = ' ';

        term.selection = Some(Selection::new(
            SelectionType::Block,
            Point { line: Line(0), column: Column(3) },
            Side::Left,
        ));

        // 同一列。
        if let Some(s) = term.selection.as_mut() {
            s.update(Point { line: Line(3), column: Column(3) }, Side::Right);
        }
        assert_eq!(term.selection_to_string(), Some(String::from("\na\na\na")));

        // 第一列。
        if let Some(s) = term.selection.as_mut() {
            s.update(Point { line: Line(3), column: Column(0) }, Side::Left);
        }
        assert_eq!(term.selection_to_string(), Some(String::from("\n\"aa\n\"a\n\"aa")));

        // 最后一列。
        if let Some(s) = term.selection.as_mut() {
            s.update(Point { line: Line(3), column: Column(4) }, Side::Right);
        }
        assert_eq!(term.selection_to_string(), Some(String::from("\na\"\na\"\na")));
    }

    /// 验证网格可以无损地往返序列化。
    ///
    /// 此测试放在 term 模块而非 grid 模块,因为我们想
    /// 在 T=Cell 下测试该属性。
    #[test]
    #[cfg(feature = "serde")]
    fn grid_serde() {
        let grid: Grid<Cell> = Grid::new(24, 80, 0);
        let serialized = serde_json::to_string(&grid).expect("ser");
        let deserialized = serde_json::from_str::<Grid<Cell>>(&serialized).expect("de");

        assert_eq!(deserialized, grid);
    }

    #[test]
    fn input_line_drawing_character() {
        let size = TermSize::new(7, 17);
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let cursor = Point::new(Line(0), Column(0));
        term.configure_charset(CharsetIndex::G0, StandardCharset::SpecialCharacterAndLineDrawing);
        term.input('a');

        assert_eq!(term.grid()[cursor].c, '▒');
    }

    #[test]
    fn clearing_viewport_keeps_history_position() {
        let size = TermSize::new(10, 20);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 创建 10 行滚动历史。
        for _ in 0..29 {
            term.newline();
        }

        // 改变显示区域。
        term.scroll_display(Scroll::Top);

        assert_eq!(term.grid.display_offset(), 10);

        // 清空视口。
        term.clear_screen(ansi::ClearMode::All);

        assert_eq!(term.grid.display_offset(), 10);
    }

    #[test]
    fn clearing_viewport_with_vi_mode_keeps_history_position() {
        let size = TermSize::new(10, 20);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 创建 10 行滚动历史。
        for _ in 0..29 {
            term.newline();
        }

        // 启用 Vi 模式。
        term.toggle_vi_mode();

        // 改变显示区域与 Vi 光标位置。
        term.scroll_display(Scroll::Top);
        term.vi_mode_cursor.point = Point::new(Line(-5), Column(3));

        assert_eq!(term.grid.display_offset(), 10);

        // 清空视口。
        term.clear_screen(ansi::ClearMode::All);

        assert_eq!(term.grid.display_offset(), 10);
        assert_eq!(term.vi_mode_cursor.point, Point::new(Line(-5), Column(3)));
    }

    #[test]
    fn clearing_scrollback_resets_display_offset() {
        let size = TermSize::new(10, 20);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 创建 10 行滚动历史。
        for _ in 0..29 {
            term.newline();
        }

        // 改变显示区域。
        term.scroll_display(Scroll::Top);

        assert_eq!(term.grid.display_offset(), 10);

        // 清空滚动历史缓冲区。
        term.clear_screen(ansi::ClearMode::Saved);

        assert_eq!(term.grid.display_offset(), 0);
    }

    #[test]
    fn clearing_scrollback_sets_vi_cursor_into_viewport() {
        let size = TermSize::new(10, 20);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 创建 10 行滚动历史。
        for _ in 0..29 {
            term.newline();
        }

        // 启用 Vi 模式。
        term.toggle_vi_mode();

        // 改变显示区域与 Vi 光标位置。
        term.scroll_display(Scroll::Top);
        term.vi_mode_cursor.point = Point::new(Line(-5), Column(3));

        assert_eq!(term.grid.display_offset(), 10);

        // 清空滚动历史缓冲区。
        term.clear_screen(ansi::ClearMode::Saved);

        assert_eq!(term.grid.display_offset(), 0);
        assert_eq!(term.vi_mode_cursor.point, Point::new(Line(0), Column(3)));
    }

    #[test]
    fn clear_saved_lines() {
        let size = TermSize::new(7, 17);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 添加一行滚动历史。
        term.grid.scroll_up(&(Line(0)..Line(1)), 1);

        // 清空历史。
        term.clear_screen(ansi::ClearMode::Saved);

        // 确保滚动不会改变网格。
        let mut scrolled_grid = term.grid.clone();
        scrolled_grid.scroll_display(Scroll::Top);

        // 截断网格以便比较。
        scrolled_grid.truncate();
        term.grid.truncate();

        assert_eq!(term.grid, scrolled_grid);
    }

    #[test]
    fn vi_cursor_keep_pos_on_scrollback_buffer() {
        let size = TermSize::new(5, 10);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 创建 11 行滚动历史。
        for _ in 0..20 {
            term.newline();
        }

        // 启用 Vi 模式。
        term.toggle_vi_mode();

        term.scroll_display(Scroll::Top);
        term.vi_mode_cursor.point.line = Line(-11);

        term.linefeed();
        assert_eq!(term.vi_mode_cursor.point.line, Line(-12));
    }

    #[test]
    fn grow_lines_updates_active_cursor_pos() {
        let mut size = TermSize::new(100, 10);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 创建 10 行滚动历史。
        for _ in 0..19 {
            term.newline();
        }
        assert_eq!(term.history_size(), 10);
        assert_eq!(term.grid.cursor.point, Point::new(Line(9), Column(0)));

        // 增加可见行数。
        size.screen_lines = 30;
        term.resize(size);

        assert_eq!(term.history_size(), 0);
        assert_eq!(term.grid.cursor.point, Point::new(Line(19), Column(0)));
    }

    #[test]
    fn grow_lines_updates_inactive_cursor_pos() {
        let mut size = TermSize::new(100, 10);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 创建 10 行滚动历史。
        for _ in 0..19 {
            term.newline();
        }
        assert_eq!(term.history_size(), 10);
        assert_eq!(term.grid.cursor.point, Point::new(Line(9), Column(0)));

        // 进入备用屏幕。
        term.set_private_mode(NamedPrivateMode::SwapScreenAndSetRestoreCursor.into());

        // 增加可见行数。
        size.screen_lines = 30;
        term.resize(size);

        // 离开备用屏幕。
        term.unset_private_mode(NamedPrivateMode::SwapScreenAndSetRestoreCursor.into());

        assert_eq!(term.history_size(), 0);
        assert_eq!(term.grid.cursor.point, Point::new(Line(19), Column(0)));
    }

    #[test]
    fn shrink_lines_updates_active_cursor_pos() {
        let mut size = TermSize::new(100, 10);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 创建 10 行滚动历史。
        for _ in 0..19 {
            term.newline();
        }
        assert_eq!(term.history_size(), 10);
        assert_eq!(term.grid.cursor.point, Point::new(Line(9), Column(0)));

        // 增加可见行数。
        size.screen_lines = 5;
        term.resize(size);

        assert_eq!(term.history_size(), 15);
        assert_eq!(term.grid.cursor.point, Point::new(Line(4), Column(0)));
    }

    #[test]
    fn shrink_lines_updates_inactive_cursor_pos() {
        let mut size = TermSize::new(100, 10);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 创建 10 行滚动历史。
        for _ in 0..19 {
            term.newline();
        }
        assert_eq!(term.history_size(), 10);
        assert_eq!(term.grid.cursor.point, Point::new(Line(9), Column(0)));

        // 进入备用屏幕。
        term.set_private_mode(NamedPrivateMode::SwapScreenAndSetRestoreCursor.into());

        // 增加可见行数。
        size.screen_lines = 5;
        term.resize(size);

        // 离开备用屏幕。
        term.unset_private_mode(NamedPrivateMode::SwapScreenAndSetRestoreCursor.into());

        assert_eq!(term.history_size(), 15);
        assert_eq!(term.grid.cursor.point, Point::new(Line(4), Column(0)));
    }

    #[test]
    fn damage_public_usage() {
        let size = TermSize::new(10, 10);
        let mut term = Term::new(Config::default(), &size, VoidListener);
        // 重置终端以进行部分受损测试(初始为整体受损)。
        term.reset_damage();

        // 测试 [`Term::input`] 输入会产生受损标记。

        let left = term.grid.cursor.point.column.0;
        term.input('d');
        term.input('a');
        term.input('m');
        term.input('a');
        term.input('g');
        term.input('e');
        let right = term.grid.cursor.point.column.0;

        let mut damaged_lines = match term.damage() {
            TermDamage::Full => panic!("Expected partial damage, however got Full"),
            TermDamage::Partial(damaged_lines) => damaged_lines,
        };
        assert_eq!(damaged_lines.next(), Some(LineDamageBounds { line: 0, left, right }));
        assert_eq!(damaged_lines.next(), None);
        term.reset_damage();

        // 创建滚动历史。
        for _ in 0..20 {
            term.newline();
        }

        match term.damage() {
            TermDamage::Full => (),
            TermDamage::Partial(_) => panic!("Expected Full damage, however got Partial "),
        };
        term.reset_damage();

        term.scroll_display(Scroll::Delta(10));
        term.reset_damage();

        // 滚动进视口后无受损。
        for idx in 0..term.columns() {
            term.goto(idx as i32, idx);
        }
        let mut damaged_lines = match term.damage() {
            TermDamage::Full => panic!("Expected partial damage, however got Full"),
            TermDamage::Partial(damaged_lines) => damaged_lines,
        };
        assert_eq!(damaged_lines.next(), None);

        // 滚回视口,使终端有 2 行可见行
        // 可供写入。
        term.scroll_display(Scroll::Delta(-2));
        term.reset_damage();

        term.goto(0, 0);
        term.goto(1, 0);
        term.goto(2, 0);
        let display_offset = term.grid().display_offset();
        let mut damaged_lines = match term.damage() {
            TermDamage::Full => panic!("Expected partial damage, however got Full"),
            TermDamage::Partial(damaged_lines) => damaged_lines,
        };
        assert_eq!(
            damaged_lines.next(),
            Some(LineDamageBounds { line: display_offset, left: 0, right: 0 })
        );
        assert_eq!(
            damaged_lines.next(),
            Some(LineDamageBounds { line: display_offset + 1, left: 0, right: 0 })
        );
        assert_eq!(damaged_lines.next(), None);
    }

    #[test]
    fn damage_cursor_movements() {
        let size = TermSize::new(10, 10);
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let num_cols = term.columns();
        // 重置终端以进行部分受损测试(初始为整体受损)。
        term.reset_damage();

        term.goto(1, 1);

        // 注意:虽然可以用 `[Term::damage]` 访问终端受损信息,
        // 但下面的测试直接访问 `term.damage.lines`,以免
        // 引入额外受损信息(如光标与 Vi 光标,不在测试范围内)。

        assert_eq!(term.damage.lines[0], LineDamageBounds { line: 0, left: 0, right: 0 });
        assert_eq!(term.damage.lines[1], LineDamageBounds { line: 1, left: 1, right: 1 });
        term.damage.reset(num_cols);

        term.move_forward(3);
        assert_eq!(term.damage.lines[1], LineDamageBounds { line: 1, left: 1, right: 4 });
        term.damage.reset(num_cols);

        term.move_backward(8);
        assert_eq!(term.damage.lines[1], LineDamageBounds { line: 1, left: 0, right: 4 });
        term.goto(5, 5);
        term.damage.reset(num_cols);

        term.backspace();
        term.backspace();
        assert_eq!(term.damage.lines[5], LineDamageBounds { line: 5, left: 3, right: 5 });
        term.damage.reset(num_cols);

        term.move_up(1);
        assert_eq!(term.damage.lines[5], LineDamageBounds { line: 5, left: 3, right: 3 });
        assert_eq!(term.damage.lines[4], LineDamageBounds { line: 4, left: 3, right: 3 });
        term.damage.reset(num_cols);

        term.move_down(1);
        term.move_down(1);
        assert_eq!(term.damage.lines[4], LineDamageBounds { line: 4, left: 3, right: 3 });
        assert_eq!(term.damage.lines[5], LineDamageBounds { line: 5, left: 3, right: 3 });
        assert_eq!(term.damage.lines[6], LineDamageBounds { line: 6, left: 3, right: 3 });
        term.damage.reset(num_cols);

        term.wrapline();
        assert_eq!(term.damage.lines[6], LineDamageBounds { line: 6, left: 3, right: 3 });
        assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 0, right: 0 });
        term.move_forward(3);
        term.move_up(1);
        term.damage.reset(num_cols);

        term.linefeed();
        assert_eq!(term.damage.lines[6], LineDamageBounds { line: 6, left: 3, right: 3 });
        assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 3, right: 3 });
        term.damage.reset(num_cols);

        term.carriage_return();
        assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 0, right: 3 });
        term.damage.reset(num_cols);

        term.erase_chars(5);
        assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 0, right: 5 });
        term.damage.reset(num_cols);

        term.delete_chars(3);
        let right = term.columns() - 1;
        assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 0, right });
        term.move_forward(term.columns());
        term.damage.reset(num_cols);

        term.move_backward_tabs(1);
        assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 8, right });
        term.save_cursor_position();
        term.goto(1, 1);
        term.damage.reset(num_cols);

        term.restore_cursor_position();
        assert_eq!(term.damage.lines[1], LineDamageBounds { line: 1, left: 1, right: 1 });
        assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 8, right: 8 });
        term.damage.reset(num_cols);

        term.clear_line(ansi::LineClearMode::All);
        assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 0, right });
        term.damage.reset(num_cols);

        term.clear_line(ansi::LineClearMode::Left);
        assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 0, right: 8 });
        term.damage.reset(num_cols);

        term.clear_line(ansi::LineClearMode::Right);
        assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 8, right });
        term.damage.reset(num_cols);

        term.reverse_index();
        assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 8, right: 8 });
        assert_eq!(term.damage.lines[6], LineDamageBounds { line: 6, left: 8, right: 8 });
    }

    #[test]
    fn full_damage() {
        let size = TermSize::new(100, 10);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        assert!(term.damage.full);
        for _ in 0..20 {
            term.newline();
        }
        term.reset_damage();

        term.clear_screen(ansi::ClearMode::Above);
        assert!(term.damage.full);
        term.reset_damage();

        term.scroll_display(Scroll::Top);
        assert!(term.damage.full);
        term.reset_damage();

        // 连续调用滚动显示但无变化时不应产生受损。
        term.scroll_display(Scroll::Top);
        assert!(!term.damage.full);
        term.reset_damage();

        term.set_options(Config::default());
        assert!(term.damage.full);
        term.reset_damage();

        term.scroll_down_relative(Line(5), 2);
        assert!(term.damage.full);
        term.reset_damage();

        term.scroll_up_relative(Line(3), 2);
        assert!(term.damage.full);
        term.reset_damage();

        term.deccolm();
        assert!(term.damage.full);
        term.reset_damage();

        term.decaln();
        assert!(term.damage.full);
        term.reset_damage();

        term.set_mode(NamedMode::Insert.into());
        // 仅设置 `Insert` 模式不应把终端标记为受损。
        assert!(!term.damage.full);
        term.reset_damage();

        let color_index = 257;
        term.set_color(color_index, Rgb::default());
        assert!(term.damage.full);
        term.reset_damage();

        // 再次设置相同颜色不应触发整体受损。
        term.set_color(color_index, Rgb::default());
        assert!(!term.damage.full);

        term.reset_color(color_index);
        assert!(term.damage.full);
        term.reset_damage();

        // 更新光标颜色时不应触发整体受损。
        term.set_color(NamedColor::Cursor as usize, Rgb::default());
        assert!(!term.damage.full);

        // 但在 `Insert` 模式下请求终端受损应把
        // 终端标记为整体受损。
        let _ = term.damage();
        assert!(term.damage.full);
        term.reset_damage();

        term.unset_mode(NamedMode::Insert.into());
        assert!(term.damage.full);
        term.reset_damage();

        // 把它作为最后一项检查,避免处理备用屏幕恢复。
        term.swap_alt();
        assert!(term.damage.full);
        term.reset_damage();

        let size = TermSize::new(10, 10);
        term.resize(size);
        assert!(term.damage.full);
    }

    #[test]
    fn window_title() {
        let size = TermSize::new(7, 17);
        let mut term = Term::new(Config::default(), &size, VoidListener);

        // 默认标题为 None。
        assert_eq!(term.title, None);

        // 可以设置标题。
        term.set_title(Some("Test".into()));
        assert_eq!(term.title, Some("Test".into()));

        // 标题可以压入栈。
        term.push_title();
        term.set_title(Some("Next".into()));
        assert_eq!(term.title, Some("Next".into()));
        assert_eq!(term.title_stack.first().unwrap(), &Some("Test".into()));

        // 标题可以从栈弹出并设为窗口标题。
        term.pop_title();
        assert_eq!(term.title, Some("Test".into()));
        assert!(term.title_stack.is_empty());

        // 标题栈不会无限增长。
        for _ in 0..4097 {
            term.push_title();
        }
        assert_eq!(term.title_stack.len(), 4096);

        // 终端状态重置时标题与标题栈一并重置。
        term.push_title();
        term.reset_state();
        assert_eq!(term.title, None);
        assert!(term.title_stack.is_empty());

        // 标题栈可弹回默认(None)。
        term.title = None;
        term.push_title();
        term.set_title(Some("Test".into()));
        term.pop_title();
        assert_eq!(term.title, None);

        // 标题可以重置为默认。
        term.title = Some("Test".into());
        term.set_title(None);
        assert_eq!(term.title, None);
    }

    #[test]
    fn parse_cargo_version() {
        assert!(version_number(env!("CARGO_PKG_VERSION")) >= 10_01);
        assert_eq!(version_number("0.0.1-dev"), 1);
        assert_eq!(version_number("0.1.2-dev"), 1_02);
        assert_eq!(version_number("1.2.3-dev"), 1_02_03);
        assert_eq!(version_number("999.99.99"), 9_99_99_99);
    }
}
