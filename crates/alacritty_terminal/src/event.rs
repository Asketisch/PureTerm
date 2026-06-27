use std::borrow::Cow;
use std::fmt::{self, Debug, Formatter};
use std::process::ExitStatus;
use std::sync::Arc;

use crate::term::ClipboardType;
use crate::vte::ansi::Rgb;

/// Semantic prompt marker kind from OSC 133 shell integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellMarkKind {
    /// OSC 133;A — prompt starts.
    PromptStart,
    /// OSC 133;B — command line starts.
    CommandStart,
    /// OSC 133;C — command output starts.
    OutputStart,
    /// OSC 133;D — command output ends.
    OutputEnd,
}

/// A semantic prompt mark detected from OSC 133 sequences.
//
// PureTerm (GAP-15): added `PartialEq, Eq` so this can ride `TerminalEvent`,
// whose enum requires them (used in update-loop matching / dedup). All fields
// are already `Eq`, so this is a safe additive derive with no behavior change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellMark {
    /// The kind of marker.
    pub kind: ShellMarkKind,
    /// Cursor row when the mark was detected (grid line index).
    pub row: usize,
    /// Exit code, only present for OutputEnd marks.
    pub exit_code: Option<i32>,
    /// Duration in microseconds, only present for OutputEnd marks.
    pub duration_us: Option<u64>,
}

/// Terminal event.
///
/// These events instruct the UI over changes that can't be handled by the terminal emulation layer
/// itself.
#[derive(Clone)]
pub enum Event {
    /// Grid has changed possibly requiring a mouse cursor shape change.
    MouseCursorDirty,

    /// Window title change.
    Title(String),

    /// Reset to the default window title.
    ResetTitle,

    /// Request to store a text string in the clipboard.
    ClipboardStore(ClipboardType, String),

    /// Request to write the contents of the clipboard to the PTY.
    ///
    /// The attached function is a formatter which will correctly transform the clipboard content
    /// into the expected escape sequence format.
    ClipboardLoad(ClipboardType, Arc<dyn Fn(&str) -> String + Sync + Send + 'static>),

    /// Request to write the RGB value of a color to the PTY.
    ///
    /// The attached function is a formatter which will correctly transform the RGB color into the
    /// expected escape sequence format.
    ColorRequest(usize, Arc<dyn Fn(Rgb) -> String + Sync + Send + 'static>),

    /// Write some text to the PTY.
    ///
    /// Wrapped in `Arc` so multi-window fan-out clones a pointer instead of
    /// duplicating the heap allocation. Hot path: DSR/CPR responses, device
    /// attribute queries, and similar PTY-output escape sequences that may
    /// be sent to every terminal window.
    PtyWrite(Arc<String>),

    /// Request to write the text area size.
    TextAreaSizeRequest(Arc<dyn Fn(WindowSize) -> String + Sync + Send + 'static>),

    /// Cursor blinking state has changed.
    CursorBlinkingChange,

    /// New terminal content available.
    Wakeup,

    /// Terminal bell ring.
    Bell,

    /// Shutdown request.
    Exit,

    /// Child process exited.
    ChildExit(ExitStatus),

    /// Shell integration semantic mark (OSC 133).
    ShellMark(ShellMark),

    /// Working directory changed via OSC 7.
    CwdChanged(String),
}

impl Debug for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::ClipboardStore(ty, text) => write!(f, "ClipboardStore({ty:?}, {text})"),
            Event::ClipboardLoad(ty, _) => write!(f, "ClipboardLoad({ty:?})"),
            Event::TextAreaSizeRequest(_) => write!(f, "TextAreaSizeRequest"),
            Event::ColorRequest(index, _) => write!(f, "ColorRequest({index})"),
            Event::PtyWrite(text) => write!(f, "PtyWrite({text})"),
            Event::Title(title) => write!(f, "Title({title})"),
            Event::CursorBlinkingChange => write!(f, "CursorBlinkingChange"),
            Event::MouseCursorDirty => write!(f, "MouseCursorDirty"),
            Event::ResetTitle => write!(f, "ResetTitle"),
            Event::Wakeup => write!(f, "Wakeup"),
            Event::Bell => write!(f, "Bell"),
            Event::Exit => write!(f, "Exit"),
            Event::ChildExit(status) => write!(f, "ChildExit({status:?})"),
            Event::ShellMark(mark) => write!(f, "ShellMark({:?}, row={})", mark.kind, mark.row),
            Event::CwdChanged(path) => write!(f, "CwdChanged({path})"),
        }
    }
}

/// Byte sequences are sent to a `Notify` in response to some events.
pub trait Notify {
    /// Notify that an escape sequence should be written to the PTY.
    ///
    /// TODO this needs to be able to error somehow.
    fn notify<B: Into<Cow<'static, [u8]>>>(&self, _: B);
}

#[derive(Copy, Clone, Debug)]
pub struct WindowSize {
    pub num_lines: u16,
    pub num_cols: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

/// Types that are interested in when the display is resized.
pub trait OnResize {
    fn on_resize(&mut self, window_size: WindowSize);
}

/// Event Loop for notifying the renderer about terminal events.
pub trait EventListener {
    fn send_event(&self, _event: Event) {}
}

/// Null sink for events.
pub struct VoidListener;

impl EventListener for VoidListener {}
