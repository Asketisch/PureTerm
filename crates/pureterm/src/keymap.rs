//! GAP-3.7: 键盘自定义框架。
//!
//! 把所有用户可触发操作集中到单一 `Action` 枚举，并提供配置驱动的
//! `Keymap` 查找表，取代 `handle_keyboard()` 内联 match 分派。
//!
//! 模块布局：
//! - `action`: `Action` 枚举（操作词汇表）
//! - `types`: `BindingKey` / `BindingNamedKey` / `ResolveResult` + iced 转换
//! - `defaults`: macOS 出厂默认绑定表（纯数据）
//! - `keymap_impl`: `Keymap` struct + resolve / chord 状态机
//! - `parse`: TOML `RawBinding` / `CustomCommand` + 解析辅助
//! - `maps`: 调色板 / 上下文菜单 → `Action`
//! - `tests`: 单元测试（`#[path]` 引入，不计入主文件行数）

mod action;
mod defaults;
mod keymap_impl;
mod maps;
mod parse;
mod types;

pub use action::Action;
pub use keymap_impl::Keymap;
pub use maps::{action_from_context_menu, action_from_palette_id};
pub use parse::{CustomCommand, RawBinding};
pub use types::{BindingKey, BindingNamedKey, ResolveResult};

#[cfg(test)]
#[path = "keymap/tests.rs"]
mod tests;
