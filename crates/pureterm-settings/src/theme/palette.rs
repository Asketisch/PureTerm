//! GAP-72：界面调色板与外观预设 —— 全局唯一真源。
//!
//! 主应用(`pureterm`)与设置窗口(`pureterm-settings`)是两个进程,
//! 但要呈现同一套赛博朋克观感,所以调色板定义放在**两个 crate 都能访问**
//! 的位置 —— 即本 crate(`pureterm-settings`)里,主 crate 已依赖它。
//! 反向依赖会成环,故不能放在 `pureterm` 里。
//!
//! 设计要点:
//! - [`UiPalette`] 只是一个**不可变值**(全是 `Copy` 的 `Color`),无锁、
//!   无共享可变状态;主应用把它存进 `PureTermApp`,设置窗口把它存进
//!   进程级全局(见 `crate::theme::tokens`)。
//! - `classic` 预设**逐值等于今天的效果**:主应用取 `Theme::Dark` 派生色 +
//!   现有内联硬编码色,设置窗口取现有 `theme/mod.rs` 常量。默认路径零回归。
//! - `cyberpunk` 预设是霓虹赛博朋克:深靛底 + 青/品红/紫强调色,
//!   终端 ANSI 16 色一并重做。
//!
//! 文件布局:
//! - `palette.rs` —— 类型定义与预设分发(本文件)。
//! - `palette/classic.rs` —— classic 预设的色值(与改动前逐值一致)。
//! - `palette/cyberpunk.rs` —— cyberpunk 预设的色值。
//! - `palette/tests.rs` —— 单元测试(含"classic 等价性"防回归护栏)。
//!
//! 色值同步:HTML 视觉稿 `docs/preview/cyberpunk-ui.html` 里的 hex 与本文件
//! 一一对应,改色后两边都要同步(`tests.rs` 里有断言护栏)。

// 本模块是**共享设计令牌层**:主应用(`pureterm` crate)消费其中大部分,
// 设置二进制只用得到其中一部分(它只渲染设置窗口)。模块级
// `dead_code` 豁免是为了不把「另一个 crate 还没用到的 token」报成告警
// —— CI 以 `-D warnings` 编译,没有豁免就会红。
#![allow(dead_code)]

use iced::{Color, Theme};

mod classic;
mod cyberpunk;

pub use classic::classic_tokens;
pub use cyberpunk::cyberpunk_tokens;

/// 外观预设。
///
/// 解析规则:去掉首尾空白后**精确匹配** `classic` / `cyberpunk`;
/// 任何其它取值(含大小写不一致)一律回落 [`ThemePreset::Classic`]
/// (坏配置永远不能让应用启动失败)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemePreset {
    /// 经典深色 —— 默认值,行为与引入本特性之前完全一致。
    #[default]
    Classic,
    /// 霓虹赛博朋克 —— 外壳 + 终端 ANSI 16 色整体重做。
    Cyberpunk,
}

impl ThemePreset {
    /// 从配置字符串解析预设。未知/空值/大小写不一致 → `Classic`。
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "cyberpunk" => Self::Cyberpunk,
            _ => Self::Classic,
        }
    }

    /// 配置里写入的字符串形式。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Cyberpunk => "cyberpunk",
        }
    }

    /// 设置窗口下拉框显示名。
    pub fn label(self) -> &'static str {
        match self {
            Self::Classic => "Classic(经典深色)",
            Self::Cyberpunk => "Cyberpunk(霓虹赛博朋克)",
        }
    }

    /// 设置窗口下拉框的可选项。
    pub fn options() -> &'static [Self] {
        &[Self::Classic, Self::Cyberpunk]
    }
}

/// 外壳(非终端区域)语义色。
///
/// 字段名即语义,便于调用点直接替换硬编码色值。全部字段在 classic
/// 预设下都等于「改动前散落在各 `view_*` 里的字面量」。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShellTokens {
    // ── 底与面 ──
    /// 应用底色。
    pub bg: Color,
    /// 面板底色(侧栏、卡片)。
    pub surface: Color,
    /// 面板 hover 底色。
    pub surface_hover: Color,
    /// 高层容器底色(顶栏/底栏/浮层面板)。
    pub elevated: Color,
    /// 1px 描边 / 分隔线。
    pub hairline: Color,
    /// 玻璃面板底色。
    pub glass: Color,
    /// 输入框底色。
    pub input_bg: Color,

    // ── 强调 ──
    /// 主强调色。
    pub accent: Color,
    /// 主强调色的 hover 变体。
    pub accent_hover: Color,
    /// 次强调色。
    pub accent_secondary: Color,
    /// 第三强调色。
    pub accent_tertiary: Color,
    /// 信息蓝(链接、运行中状态)。
    pub info: Color,
    /// 霓虹发光色(`Shadow::color`)。
    pub glow: Color,
    /// 青绿强调(焦点环 / vi 搜索描边)。
    pub teal: Color,

    // ── 状态 ──
    /// 成功色。
    pub success: Color,
    /// 警告色。
    pub warning: Color,
    /// 错误色。
    pub danger: Color,
    /// 警示文字色(比 danger 更亮)。
    pub alert_text: Color,
    /// 列表项里的错误文字色。
    pub error_text: Color,
    /// 错误提示里的柔和红。
    pub error_text_soft: Color,
    /// 错误面板底色。
    pub error_bg: Color,
    /// 错误面板描边。
    pub error_border: Color,
    /// 已合并 / PR 紫。
    pub merged: Color,

    // ── 文本 ──
    /// 主文本色。
    pub text_primary: Color,
    /// 次级文本 / 分组标签。
    pub text_secondary: Color,
    /// 弱化文本 / 占位符。
    pub text_dim: Color,
    /// 极弱文本(快捷键角标)。
    pub text_faint: Color,
    /// 中性灰(占位图形、次级状态)。
    pub muted: Color,
    /// 正文灰(比 text_primary 略暗的整段文字)。
    pub text_body: Color,
    /// 亮琥珀(通知徽标等高饱和点缀)。
    pub amber_bright: Color,
    /// 中性灰(占位图形)。
    pub neutral: Color,
    /// 代码片段文字色。
    pub code_text: Color,
    /// 柔和绿。
    pub soft_green: Color,
    /// 柔和蓝。
    pub soft_blue: Color,
    /// 柔和绿(列表项状态)。
    pub ok_soft: Color,

    // ── 高亮 ──
    /// 激活项底色(高亮)。
    pub highlight: Color,
    /// 弱高亮(hover)。
    pub highlight_soft: Color,
    /// 更弱的激活底色。
    pub highlight_faint: Color,

    // ── 浮层 ──
    /// 浮层面板底色。
    pub overlay_bg: Color,
    /// 浮层面板描边。
    pub overlay_border: Color,
    /// 浮层背后的遮罩。
    pub backdrop: Color,
    /// 浮层投影色。
    pub shadow: Color,
    /// 更重的投影色(命令面板等大浮层)。
    pub shadow_strong: Color,

    // ── agent 指示 ──
    /// agent 活动(暖琥珀)。
    pub agent_active: Color,
    /// agent 休眠(冷蓝)。
    pub agent_sleep: Color,
    /// agent 待处理徽标(警示琥珀)。
    pub agent_pending: Color,

    /// 是否启用「赛博朋克专属装饰」(当前:激活标签底部的霓虹条 +
    /// 激活标签标题用强调色)。classic 关闭 → 与改造前逐像素一致。
    pub neon_active_tab: bool,
}

impl ShellTokens {
    /// 派生 Iced 主题。Iced 的 `Palette` 只有 6 个槽位,这里把主强调/
    /// 成功/警告/错误映射过去,其余语义色仍走 [`ShellTokens`]。
    pub fn to_iced_theme(self) -> Theme {
        Theme::custom(
            "PureTerm".to_string(),
            iced::theme::Palette {
                background: self.bg,
                text: self.text_primary,
                primary: self.accent,
                success: self.success,
                danger: self.danger,
                warning: self.warning,
            },
        )
    }
}

/// 状态栏里按语义着色的强调色(进行中 / 成功 / 失败 / …)。
///
/// 与分组色(chip_groups)分开:分组色表达「这是哪一簇」,语义色表达
/// 「这一项现在是什么状态」。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatusSemantics {
    /// 进行中的操作(琥珀)。
    pub running: Color,
    /// 正常 / 成功(绿)。
    pub ok: Color,
    /// 失败 / 关闭(红)。
    pub error: Color,
    /// 已合并(紫)。
    pub merged: Color,
    /// vi 模式(青)。
    pub vi: Color,
    /// LSP 有错误(橙红)。
    pub lsp_error: Color,
    /// 未读通知(橙)。
    pub notification: Color,
    /// CPU 走势线。
    pub cpu: Color,
    /// 内存走势线。
    pub memory: Color,
    /// 网络走势线。
    pub network: Color,
    /// 电池低电量。
    pub battery_low: Color,
    /// 电池正常。
    pub battery_ok: Color,
}

/// 状态栏 chip 的专用色。
///
/// 与外壳色分开,因为 chip 有自己的分组配色与半透明底 —— 把它们塞进
/// `ShellTokens` 会让外壳语义色表膨胀到难以维护。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatusBarTokens {
    /// 栏体底色(顶栏与底栏共用)。
    pub bar_bg: Color,
    /// 栏体上方的分隔线色。
    pub bar_border: Color,
    /// chip 描边色。
    pub chip_border: Color,
    /// chip 默认文字色。
    pub chip_text: Color,
    /// 四个分组的 chip 底色 —— 顺序:Context / Runtime / System / Meta。
    pub chip_groups: [Color; 4],
    /// 语义强调色。
    pub semantics: StatusSemantics,
}

/// 终端语义色(前景/背景/ANSI 16 色/叠加层)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalTokens {
    /// 终端背景。
    pub bg: Color,
    /// 终端前景。
    pub fg: Color,
    /// 光标色。
    pub cursor: Color,
    /// 选区高亮色。
    pub selection: Color,
    /// 搜索命中高亮色。
    pub search_hit: Color,
    /// 触发器折叠区的遮罩色。
    pub fold_conceal: Color,
    /// vi 模式光标色。
    pub vi_cursor: Color,
    /// vi 模式下被压暗的主光标色。
    pub vi_dim: Color,
    /// ANSI 16 色(normal 8 色 + bright 8 色)。
    pub ansi: [Color; 16],
}

/// 完整色板 = 主应用外壳 + 设置窗口外壳 + 状态栏 + 终端。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiPalette {
    /// 主应用外壳色(顶栏/侧栏/状态栏/浮层)。
    pub app: ShellTokens,
    /// 设置窗口外壳色。
    pub settings: ShellTokens,
    /// 状态栏 chip 色。
    pub status_bar: StatusBarTokens,
    /// 终端色。
    pub terminal: TerminalTokens,
}

impl UiPalette {
    /// 取某个预设对应的完整色板。
    pub fn for_preset(preset: ThemePreset) -> Self {
        match preset {
            ThemePreset::Classic => classic_tokens(),
            ThemePreset::Cyberpunk => cyberpunk_tokens(),
        }
    }

    /// 主应用外壳对应的 Iced 主题。
    pub fn to_iced_theme(self) -> Theme {
        self.app.to_iced_theme()
    }
}

/// 取色板里的取色器预置色(12 色)。属于**内容**而非主题色,同样集中在此
/// 以便主 crate 不再散落字面量。
pub const PICKER_SWATCHES: [(Color, &str); 12] = [
    (Color::from_rgb(0.90, 0.30, 0.30), "红"),
    (Color::from_rgb(1.00, 0.60, 0.20), "橙"),
    (Color::from_rgb(0.95, 0.85, 0.25), "黄"),
    (Color::from_rgb(0.40, 0.80, 0.30), "绿"),
    (Color::from_rgb(0.20, 0.75, 0.75), "青"),
    (Color::from_rgb(0.30, 0.50, 0.95), "蓝"),
    (Color::from_rgb(0.60, 0.30, 0.85), "紫"),
    (Color::from_rgb(0.95, 0.45, 0.70), "粉"),
    (Color::from_rgb(0.70, 0.45, 0.20), "棕"),
    (Color::from_rgb(0.55, 0.55, 0.55), "灰"),
    (Color::from_rgb(0.20, 0.65, 0.90), "浅蓝"),
    (Color::from_rgb(0.85, 0.85, 0.85), "白"),
];

/// 归一化 RGBA 数组 → `iced::Color`(方便从 `[f32; 4]` 反推)。
pub fn color_from_rgba(rgba: [f32; 4]) -> Color {
    Color { r: rgba[0], g: rgba[1], b: rgba[2], a: rgba[3] }
}

/// `iced::Color` → 归一化 RGBA 数组。
pub fn rgba_from_color(c: Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

/// 便捷构造:不透明 RGB。
pub fn rgb(r: f32, g: f32, b: f32) -> Color {
    Color::from_rgb(r, g, b)
}

/// 便捷构造:带 alpha 的 RGB。
pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color::from_rgba(r, g, b, a)
}

#[cfg(test)]
#[path = "palette/tests.rs"]
mod tests;
