use std::borrow::Cow;
use std::fmt::{self, Debug, Formatter};
use std::process::ExitStatus;
use std::sync::Arc;

use crate::term::ClipboardType;
use crate::vte::ansi::Rgb;

/// OSC 133 shell 集成的语义提示标记类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellMarkKind {
    /// OSC 133;A —— 提示符开始。
    PromptStart,
    /// OSC 133;B —— 命令行开始。
    CommandStart,
    /// OSC 133;C —— 命令输出开始。
    OutputStart,
    /// OSC 133;D —— 命令输出结束。
    OutputEnd,
}

/// 从 OSC 133 序列检测到的语义提示标记。
//
// PureTerm(GAP-15):加了 `PartialEq, Eq`,使其能搭 `TerminalEvent`,
// 后者的枚举要求它们(用于 update 循环匹配/去重)。所有字段
// 本就 `Eq`,这是安全的增量派生,行为不变。
//
// PureTerm(GAP-15):加了 `command`。精确命令文本无法从网格恢复
//(B 事件被处理时,命令输出已随同一次 PTY 读进入网格,
// 滚动还会让行漂移),因此 shell 在 B 标记的 `cmd=` 参数里
// 以 hex 编码发送,扫描器在此解码。仅出现在 shell 选择
// 附加注释的 CommandStart 标记上。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellMark {
    /// 标记的类型。
    pub kind: ShellMarkKind,
    /// 检测到标记时的光标行(网格行索引)。
    pub row: usize,
    /// 退出码,仅 OutputEnd 标记有。
    pub exit_code: Option<i32>,
    /// 时长(微秒),仅 OutputEnd 标记有。
    pub duration_us: Option<u64>,
    /// 精确命令文本(从 B 标记的 `cmd=` 参数 hex 解码),仅
    /// CommandStart 标记有。为何发送而非从网格恢复,
    /// 见结构体文档。
    pub command: Option<String>,
}

/// GAP-09:何时应显示桌面通知(映射 kitty OSC 99 的 `o` 键)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyOccasion {
    /// `o=always`(默认)——源窗格不可见时通知。
    Always,
    /// `o=unfocused` —— 仅应用窗口未聚焦时通知。
    Unfocused,
    /// `o=invisible` —— 仅源窗格不可见
    ///(应用未聚焦或在非活动标签页)时通知。
    Invisible,
}

/// GAP-09:桌面通知的来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifySource {
    /// iTerm2 遗留 OSC 9(`\x1b]9;<msg>`)。
    Osc9,
    /// kitty OSC 99(分块,`p`/`i`/`d`/`e`/`o` 元数据)。
    Osc99,
    /// urxvt 的 OSC 777(`\x1b]777;notify;<title>;<body>`)。
    Osc777,
    /// 终端响铃(0x07),经响铃直通呈现。
    Bell,
    /// `pureterm notify` CLI(经 GUI IPC socket 发布)。
    Cli,
}

/// T-core-graphics:传输图形载荷所用协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsProtocol {
    /// DEC Sixel(`\x1bPq ... \x1b\\`)——像素式,6 位调色板。
    Sixel,
    /// Kitty 图形协议(`\x1b_G ... \x1b\\`)——分块,base64。
    Kitty,
    /// iTerm2 内联图像(`\x1b]1337;File=...`)——遗留。
    ITerm2,
}

/// T-core-graphics:图形图像的载荷编码格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsFormat {
    /// 32 位 RGBA 像素(已解码)。
    Rgba,
    /// PNG(仍为编码态;由渲染器解码)。
    Png,
    /// 原始 RGB(24 位)。
    Rgb,
}

/// T-core-graphics:图像到网格的一个放置(锚点)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsPlacement {
    /// 图像的唯一 id。
    pub id: u32,
    /// 左上角网格列。
    pub col: u16,
    /// 左上角网格行(相对网格而非屏幕)。
    pub row: u16,
    /// 宽度(单元格)。
    pub cols: u16,
    /// 高度(单元格)。
    pub rows: u16,
    /// Z 序(新者覆盖旧者)。单调递增。
    pub z: u32,
}

/// T-core-graphics:已完全解码、可供渲染器的图形图像。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsImage {
    /// 产出该图像的协议。
    pub protocol: GraphicsProtocol,
    /// 载荷编码。
    pub format: GraphicsFormat,
    /// 到网格的放置。
    pub placement: GraphicsPlacement,
    /// 像素宽(或 format = Rgba/Rgb 时的图像宽)。
    pub width: u32,
    /// 像素高。
    pub height: u32,
    /// 已解码 RGBA 字节(长度 = width * height * 4)。渲染器
    /// 应自行从 `encoded` 解码(如 PNG)时为 `None`。
    pub pixels: Option<Vec<u8>>,
    /// 仍为编码态的字节(如 format = Png 时的 PNG)。`pixels`
    /// 为 `Some` 时为空。
    pub encoded: Vec<u8>,
}

/// T-core-plugin-runtime v3:发给指定插件的解析器钩子 OSC 序列。
/// 由 `event_loop::scan_plugin_osc` 在 PTY 字节流包含
/// `\x1b]1337;PureTerm-Plugin-<name>;<payload>\x07`(或 ST 终止)时发出。
/// 宿主按 `name` 分派,把 `payload` 路由到插件的
/// `HostFn::Parser` 回调。
/// `HostFn::Parser` 回调。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginOsc {
    /// 插件清单名(小写 `[a-z0-9_-]+`)。
    pub name: String,
    /// 第二个 `;` 与终止符之间的原始载荷字节。
    /// 原样传递——由插件决定如何解释。
    pub payload: Vec<u8>,
}

/// GAP-09:从 PTY(OSC 9/99/777)浮出的桌面通知、
/// 由响铃合成、或经 CLI 发布。
//
// PureTerm(GAP-09):加入以便通知搭 `Event::Notification`,正如
// `ShellMark` 搭 `Event::ShellMark`。派生 `PartialEq, Eq` 是因为
// pureterm crate 的 `TerminalEvent` 包装它并派生相同——每个字段都是
// `Option<String>`/`String`/`Copy` 枚举,派生安全且只增不改。
// 时间戳有意不放在这里(它们是可变的 GUI 侧状态,会
// 破坏 `Eq`);只存在于 pureterm 侧的 `NotificationEntry`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// 尽力而为的标题;OSC 9 与响铃为 `None`。
    pub title: Option<String>,
    /// 消息正文。
    pub body: String,
    /// kitty OSC 99 的 `i`,净化为 `[a-zA-Z0-9_-.+]`,用于去重/更新
    /// 在途通知。OSC 9/777/响铃/CLI 为 `None`。
    pub id: Option<String>,
    /// kitty OSC 99 的 `o`;未设置时默认 `Always`。
    pub occasion: NotifyOccasion,
    /// 通知来源。
    pub source: NotifySource,
}

/// 终端事件。
///
/// 这些事件就终端仿真层自身无法处理的变更
/// 通知 UI。
#[derive(Clone)]
pub enum Event {
    /// 网格变化,可能需要改变鼠标光标形状。
    MouseCursorDirty,

    /// 窗口标题变化。
    Title(String),

    /// 重置为默认窗口标题。
    ResetTitle,

    /// 请求把文本串存入剪贴板。
    ClipboardStore(ClipboardType, String),

    /// 请求把剪贴板内容写入 PTY。
    ///
    /// 附带的函数是格式化器,把剪贴板内容正确变换为
    /// 期望的转义序列格式。
    ClipboardLoad(ClipboardType, Arc<dyn Fn(&str) -> String + Sync + Send + 'static>),

    /// 请求把某颜色的 RGB 值写入 PTY。
    ///
    /// 附带的函数是格式化器,把 RGB 颜色正确变换为
    /// 期望的转义序列格式。
    ColorRequest(usize, Arc<dyn Fn(Rgb) -> String + Sync + Send + 'static>),

    /// 向 PTY 写一些文本。
    ///
    /// 包在 `Arc` 里,使多窗口扇出克隆指针而非复制
    /// 堆分配。热路径:DSR/CPR 应答、设备
    /// 属性查询等可能发给每个终端窗口的
    /// PTY 输出转义序列。
    PtyWrite(Arc<String>),

    /// 请求写入文本区尺寸。
    TextAreaSizeRequest(Arc<dyn Fn(WindowSize) -> String + Sync + Send + 'static>),

    /// 光标闪烁状态变化。
    CursorBlinkingChange,

    /// 新的终端内容可用。
    Wakeup,

    /// 终端响铃。
    Bell,

    /// 关闭请求。
    Exit,

    /// 子进程退出。
    ChildExit(ExitStatus),

    /// shell 集成语义标记(OSC 133)。
    ShellMark(ShellMark),

    /// 工作目录经 OSC 7 变化。
    CwdChanged(String),

    /// GAP-09:桌面通知(OSC 9/99/777 或响铃合成)。
    Notification(Notification),

    /// T-core-graphics:解码后的图形图像(Sixel / Kitty / iTerm2)。
    /// 携带已解码 RGBA 像素或仍为编码态的载荷,以及
    /// 网格放置;渲染器决定何时 blit。
    GraphicsImage(GraphicsImage),

    /// T-core-plugin-runtime v3:解析器钩子输出。插件注册
    /// `TerminalParserHook` Provides 能力;宿主转发任何
    /// `\x1b]1337;PureTerm-Plugin-<name>;<payload>\x07` 序列到这里,
    /// 由插件的 `HostFn::Parser` 回调处理。`name` 为
    /// 插件清单名(小写、`[a-z0-9_-]+`);`payload` 为
    /// 第二个 `;` 与终止符之间的原始字节。
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

/// 字节序列以响应某些事件发送给 `Notify`。
pub trait Notify {
    /// 通知把一段转义序列写入 PTY。
    ///
    /// TODO:这需要某种错误报告能力。
    fn notify<B: Into<Cow<'static, [u8]>>>(&self, _: B);
}

#[derive(Copy, Clone, Debug)]
pub struct WindowSize {
    pub num_lines: u16,
    pub num_cols: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

/// 关注显示尺寸何时变化的所有类型。
pub trait OnResize {
    fn on_resize(&mut self, window_size: WindowSize);
}

/// 把终端事件通知渲染器的事件循环。
pub trait EventListener {
    fn send_event(&self, _event: Event) {}
}

/// 事件的空接收器。
pub struct VoidListener;

impl EventListener for VoidListener {}
