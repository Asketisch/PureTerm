//! `Keymap` — 配置驱动的键绑定查找表 + chord（双键序列）状态机。
//!
//! 设计：keypress → `Keymap::resolve()` → `Action` → `dispatch_action()`
//! → `Message` → `update()`。
//!
//! Overlay UI 守卫（Escape 关闭、方向键导航、Enter 确认）仍硬编码在
//! `handle_keyboard` —— 它们依赖 UI 状态，不可由用户定制。

use std::collections::HashMap;
use std::time::Instant;

use iced::keyboard::{Key, Modifiers};

use super::action::Action;
use super::defaults::default_bindings;
use super::parse::{parse_raw_binding, split_mods_and_key, CustomCommand, RawBinding};
use super::types::{key_to_binding_key, BindingKey, BindingNamedKey, ResolveResult};

/// 配置驱动的键盘绑定表。
///
/// `bindings` 以 `(Modifiers, BindingKey) → Action` 映射单击
/// 快捷键。`chord_bindings` 以 `(prefix_mods, prefix_key, second_mods,
/// second_key) → Action` 映射双键序列(例如先 `Ctrl+K` 再 `j`)。
pub struct Keymap {
    pub(crate) bindings: HashMap<(Modifiers, BindingKey), Action>,
    chord_bindings: HashMap<(Modifiers, BindingKey, Modifiers, BindingKey), Action>,
    pending_chord: Option<(Modifiers, BindingKey, Instant)>,
    chord_timeout_ms: u64,
}

impl Keymap {
    /// 由硬编码默认值创建键位映射,精确复现
    /// 当前 `handle_keyboard` 内联 match 的行为。
    pub fn from_defaults() -> Self {
        Self {
            bindings: default_bindings(),
            chord_bindings: HashMap::new(),
            pending_chord: None,
            chord_timeout_ms: 1000,
        }
    }

    /// 将一次按键事件解析为动作。
    ///
    /// 先检查挂起的和弦状态,再检查单击绑定。
    pub fn resolve(&mut self, key: &Key, mods: Modifiers, now: Instant) -> ResolveResult {
        let binding_key = match key_to_binding_key(key) {
            Some(bk) => bk,
            None => return ResolveResult::Passthrough,
        };

        // ── 和弦解析 ────────────────────────────────────────────────────
        if let Some((prefix_mods, prefix_key, ts)) = self.pending_chord.take() {
            let elapsed = now.duration_since(ts).as_millis() as u64;
            if elapsed <= self.chord_timeout_ms {
                // Escape 取消和弦
                if binding_key == BindingKey::Named(BindingNamedKey::Escape) {
                    return ResolveResult::Passthrough;
                }
                // 查找和弦补全
                if let Some(action) = self
                    .chord_bindings
                    .get(&(prefix_mods, prefix_key.clone(), mods, binding_key.clone()))
                {
                    return ResolveResult::ChordAction(action.clone());
                }
                // 无和弦匹配——前缀已被消费;
                // 下面的逻辑把此键当作全新按键处理。
            }
            // 和弦超时或未匹配——回落到正常查找
        }

        // ── 单击查找 ────────────────────────────────────────────────────
        if let Some(action) = self.bindings.get(&(mods, binding_key.clone())) {
            return ResolveResult::Action(action.clone());
        }

        // ── 检查按键是否为和弦前缀 ─────────────────────────────────────────────
        // 若存在以 (mods, binding_key) 开头的 chord_bindings,
        // 则此键就是和弦前缀。
        let is_prefix = self
            .chord_bindings
            .keys()
            .any(|(pm, pk, _, _)| *pm == mods && *pk == binding_key);

        if is_prefix {
            self.pending_chord = Some((mods, binding_key, now));
            return ResolveResult::ChordPending;
        }

        ResolveResult::Passthrough
    }

    /// 从 TOML `[[keyboard.bindings]]` 原始数据加载绑定。
    /// 用户绑定会覆盖同一组合键的默认绑定。
    /// 未知动作或无法解析的按键被静默跳过(并 log::warn)。
    pub fn from_toml_overrides(raw: &[RawBinding], defaults: &Keymap) -> Self {
        let mut bindings = defaults.bindings.clone();
        for rb in raw {
            match parse_raw_binding(rb) {
                Ok(((mods, bk), action)) => {
                    bindings.insert((mods, bk), action);
                }
                Err(e) => {
                    log::warn!(
                        "keymap: skipping binding key={} mods={} action={}: {}",
                        rb.key,
                        rb.mods,
                        rb.action,
                        e
                    );
                }
            }
        }
        Self {
            bindings,
            chord_bindings: defaults.chord_bindings.clone(),
            pending_chord: None,
            chord_timeout_ms: defaults.chord_timeout_ms,
        }
    }

    /// GAP-6.4:把用户自定义 `[[commands]]` 注册为键位映射绑定。
    ///
    /// 每个带 `key` 的命令变成 `Action::CustomCommand(id)` 绑定。
    /// 没有 `key` 的命令仅用于命令面板,在此跳过。
    ///
    /// 字母键规范化(与默认绑定的存储方式一致,
    /// 使自定义快捷键匹配操作系统实际派发的按键事件):
    /// - **Shift + 字母** → 注册两种形式,因为 macOS 可能上报
    ///   小写+Shift 或大写+无 Shift 的事件;
    /// - **无 Shift + 字母** → 存储小写(操作系统上报未按 Shift 的
    ///   字母为小写),与用户书写的大小写无关。
    pub fn register_custom_commands(&mut self, cmds: &[CustomCommand]) {
        for c in cmds {
            let Some(key_str) = c.key.as_deref() else {
                continue;
            };
            let (mods, key) = split_mods_and_key(key_str, c.mods.as_deref());
            let (Some(mods), Some(key)) = (mods, key) else {
                continue;
            };

            let action = Action::CustomCommand(c.id.clone());

            // 字母键规范化(见方法文档)。
            if let BindingKey::Character(ch) = &key {
                if ch.len() == 1 && ch.chars().next().unwrap().is_ascii_alphabetic() {
                    let c0 = ch.chars().next().unwrap();
                    if mods.contains(Modifiers::SHIFT) {
                        // macOS 双形态:上报小写+Shift 或
                        // 大写+无 Shift。同时注册两种形式,
                        // 无论操作系统派发哪种形式都能触发。
                        let lower = c0.to_ascii_lowercase().to_string();
                        let upper = c0.to_ascii_uppercase().to_string();
                        self.bindings
                            .insert((mods, BindingKey::Character(lower)), action.clone());
                        self.bindings.insert(
                            (mods ^ Modifiers::SHIFT, BindingKey::Character(upper)),
                            action,
                        );
                        continue;
                    } else {
                        // 未按 Shift 的字母:操作系统上报小写,所以
                        // 存储小写以匹配(与默认值一致)。
                        let lower = c0.to_ascii_lowercase().to_string();
                        self.bindings
                            .insert((mods, BindingKey::Character(lower)), action);
                        continue;
                    }
                }
            }
            self.bindings.insert((mods, key), action);
        }
    }

    /// 返回当前绑定的快照,供界面展示 / CLI 列举。
    pub fn list_bindings(&self) -> Vec<(Modifiers, BindingKey, Action)> {
        self.bindings
            .iter()
            .map(|((mods, bk), action)| (*mods, bk.clone(), action.clone()))
            .collect()
    }
}
