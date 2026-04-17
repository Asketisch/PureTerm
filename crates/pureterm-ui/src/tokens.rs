//! PureTerm UI 的设计令牌。
//!
//! 集中化的间距、大小、圆角和笔画值。
//! 改编自 notedeck 设计系统。

// --- 间距 ---

/// 超小间距 (4px)
pub const SPACING_XS: f32 = 4.0;
/// 小间距 (8px)
pub const SPACING_SM: f32 = 8.0;
/// 中等间距 (12px)
pub const SPACING_MD: f32 = 12.0;
/// 大间距 (16px)
pub const SPACING_LG: f32 = 16.0;
/// 超大间距 (24px)
pub const SPACING_XL: f32 = 24.0;
/// 双特大间距 (32px)
pub const SPACING_XXL: f32 = 32.0;

// --- 圆角 ---

/// 小圆角 (4px) - 卡片、输入框的轻微圆角
pub const RADIUS_SM: f32 = 4.0;
/// 中圆角 (8px) - 按钮、面板
pub const RADIUS_MD: f32 = 8.0;
/// 大圆角 (12px) - 模态框、对话框
pub const RADIUS_LG: f32 = 12.0;
/// 药丸圆角 (18px) - 搜索栏、标签、胶囊
pub const RADIUS_PILL: f32 = 18.0;

// --- 笔画宽度 ---

/// 细笔画 (1.0px) - 边框、分隔线
pub const STROKE_THIN: f32 = 1.0;
/// 中等笔画 (1.5px) - 图标、强调边框
pub const STROKE_MEDIUM: f32 = 1.5;
/// 粗笔画 (2.0px) - 激活状态、强调
pub const STROKE_THICK: f32 = 2.0;

// --- 图标大小 ---

/// 小图标 (16px) - 内联图标、搜索图标
pub const ICON_SM: f32 = 16.0;
/// 中图标 (24px) - 工具栏、导航图标
pub const ICON_MD: f32 = 24.0;
/// 大图标 (32px) - 功能图标、空状态
pub const ICON_LG: f32 = 32.0;

// --- 按钮高度 ---

/// 小按钮高度 (28px) - 紧凑操作
pub const BUTTON_SM: f32 = 28.0;
/// 中按钮高度 (34px) - 标准操作
pub const BUTTON_MD: f32 = 34.0;
/// 大按钮高度 (44px) - 主要操作、触控目标
pub const BUTTON_LG: f32 = 44.0;

// --- 透明度 ---

/// 禁用状态透明度
pub const OPACITY_DISABLED: f32 = 0.38;
/// 次要/静音内容透明度
pub const OPACITY_MUTED: f32 = 0.6;
/// 叠加背景透明度
pub const OPACITY_OVERLAY: f32 = 0.5;

// --- 动画 ---

/// 悬停/交互过渡的默认动画速度
pub const ANIM_SPEED: f32 = 0.05;

// --- 框架边距 ---

/// 默认框架边距
pub const FRAME_MARGIN: f32 = 8.0;
