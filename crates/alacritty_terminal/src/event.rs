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
//
// PureTerm (GAP-15): added `command`. The exact command text can't be recovered
// from the grid (by the time the B-event is handled, the command's output is
// already in the grid from the same PTY read, and scrolling drifts the row), so
// the shell sends it hex-encoded in the B mark's `cmd=` param and the scanner
// decodes it here. Only present for CommandStart marks the shell chose to annotate.
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
    /// Exact command text (hex-decoded from the B mark's `cmd=` param), only
    /// present for CommandStart marks. See struct doc for why this is sent
    /// rather than recovered from the grid.
    pub command: Option<String>,
}

/// GAP-09: when a desktop notification should be shown (maps to the kitty OSC 99 `o` key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyOccasion {
    /// `o=always` (default) — toast when the source pane is not visible.
    Always,
    /// `o=unfocused` — toast only when the application window is not focused.
    Unfocused,
    /// `o=invisible` — toast only when the source pane is invisible
    /// (unfocused app OR in a non-active tab).
    Invisible,
}

/// GAP-09: origin of a desktop notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifySource {
    /// iTerm2 legacy OSC 9 (`\x1b]9;<msg>`).
    Osc9,
    /// kitty OSC 99 (chunked, `p`/`i`/`d`/`e`/`o` metadata).
    Osc99,
    /// urxvt OSC 777 (`\x1b]777;notify;<title>;<body>`).
    Osc777,
    /// Terminal bell (0x07), surfaced via bell passthrough.
    Bell,
    /// `pureterm notify` CLI (posted over the GUI IPC socket).
    Cli,
}

/// T-core-graphics: protocol used to transfer a graphics payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsProtocol {
    /// DEC Sixel (`\x1bPq ... \x1b\\`) — pixel-based, 6-bit palette.
    Sixel,
    /// Kitty graphics protocol (`\x1b_G ... \x1b\\`) — chunked, base64.
    Kitty,
    /// iTerm2 inline image (`\x1b]1337;File=...`) — legacy.
    ITerm2,
}

/// T-core-graphics: encoded payload format for a graphics image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsFormat {
    /// 32-bit RGBA pixels (already decoded).
    Rgba,
    /// PNG (still encoded; will be decoded by the renderer).
    Png,
    /// Raw RGB (24-bit).
    Rgb,
}

/// T-core-graphics: a placement (anchor) of an image onto the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsPlacement {
    /// Unique id of the image.
    pub id: u32,
    /// Top-left grid column.
    pub col: u16,
    /// Top-left grid row (grid-relative, not screen-relative).
    pub row: u16,
    /// Width in cells.
    pub cols: u16,
    /// Height in cells.
    pub rows: u16,
    /// Z-order (newer overlays older). Monotonically increasing.
    pub z: u32,
}

/// T-core-graphics: a fully-decoded graphics image ready for the renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsImage {
    /// Protocol that produced the image.
    pub protocol: GraphicsProtocol,
    /// Payload encoding.
    pub format: GraphicsFormat,
    /// Placement onto the grid.
    pub placement: GraphicsPlacement,
    /// Pixel width (or image width when format = Rgba/Rgb).
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Decoded RGBA bytes (length = width * height * 4). `None` if the
    /// renderer should decode from `encoded` itself (e.g. PNG).
    pub pixels: Option<Vec<u8>>,
    /// Still-encoded bytes (e.g. PNG when format = Png). Empty when `pixels`
    /// is `Some`.
    pub encoded: Vec<u8>,
}

/// T-core-plugin-runtime v3: a parser-hook OSC sequence destined for a
/// named plugin. Emitted by `event_loop::scan_plugin_osc` whenever the
/// PTY byte stream contains
/// `\x1b]1337;PureTerm-Plugin-<name>;<payload>\x07` (or ST-terminated).
/// The host dispatches on `name` to route `payload` to the plugin's
/// `HostFn::Parser` callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginOsc {
    /// Plugin manifest name (lowercase `[a-z0-9_-]+`).
    pub name: String,
    /// Raw payload bytes between the second `;` and the terminator.
    /// Untouched — the plugin decides how to interpret it.
    pub payload: Vec<u8>,
}

/// GAP-09: a desktop notification surfaced from the PTY (OSC 9/99/777),
/// synthesized from a bell, or posted via the CLI.
//
// PureTerm (GAP-09): added so notifications ride `Event::Notification`, exactly
// like `ShellMark` rides `Event::ShellMark`. Derives `PartialEq, Eq` because the
// pureterm-crate `TerminalEvent` wraps it and derives the same — every field is
// `Option<String>`/`String`/`Copy`-enum, so this is a safe additive derive.
// Timestamps are deliberately NOT here (they are mutable/GUI-side state and
// would break `Eq`); they live only on the pureterm-side `NotificationEntry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// Best-effort title; `None` for OSC 9 and bell.
    pub title: Option<String>,
    /// Message body.
    pub body: String,
    /// kitty OSC 99 `i`, sanitized to `[a-zA-Z0-9_-+.]`, used to dedup/update
    /// in-flight notifications. `None` for OSC 9/777/bell/CLI.
    pub id: Option<String>,
    /// kitty OSC 99 `o`; defaults to `Always` when unset.
    pub occasion: NotifyOccasion,
    /// Where the notification came from.
    pub source: NotifySource,
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

    /// GAP-09: desktop notification (OSC 9/99/777 or bell-derived).
    Notification(Notification),

    /// T-core-graphics: decoded graphics image (Sixel / Kitty / iTerm2).
    /// Carries either decoded RGBA pixels or a still-encoded payload plus
    /// the grid placement; the renderer decides when to blit.
    GraphicsImage(GraphicsImage),

    /// T-core-plugin-runtime v3: parser hook output. Plugins register a
    /// `TerminalParserHook` Provides capability; the host forwards any
    /// `\x1b]1337;PureTerm-Plugin-<name>;<payload>\x07` sequence here so
    /// the plugin's `HostFn::Parser` callback can act on it. `name` is
    /// the plugin's manifest name (lowercase, `[a-z0-9_-]+`); `payload`
    /// is the raw bytes between the second `;` and the terminator.
    PluginOsc(PluginOsc),
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
            Event::Notification(n) => write!(f, "Notification({:?})", n),
            Event::GraphicsImage(img) => write!(
                f,
                "GraphicsImage({:?} {}x{} @{},{} {}x{} cells)",
                img.protocol, img.width, img.height, img.placement.col, img.placement.row,
                img.placement.cols, img.placement.rows,
            ),
            Event::PluginOsc(p) => write!(f, "PluginOsc({}, {} bytes)", p.name, p.payload.len()),
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
