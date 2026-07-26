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

/// Config-driven keyboard binding table.
///
/// `bindings` maps `(Modifiers, BindingKey) → Action` for single-stroke
/// shortcuts. `chord_bindings` maps `(prefix_mods, prefix_key, second_mods,
/// second_key) → Action` for two-key sequences (e.g. `Ctrl+K` then `j`).
pub struct Keymap {
    pub(crate) bindings: HashMap<(Modifiers, BindingKey), Action>,
    chord_bindings: HashMap<(Modifiers, BindingKey, Modifiers, BindingKey), Action>,
    pending_chord: Option<(Modifiers, BindingKey, Instant)>,
    chord_timeout_ms: u64,
}

impl Keymap {
    /// Create a keymap from the hardcoded defaults that exactly reproduce
    /// the current `handle_keyboard` inline match behaviour.
    pub fn from_defaults() -> Self {
        Self {
            bindings: default_bindings(),
            chord_bindings: HashMap::new(),
            pending_chord: None,
            chord_timeout_ms: 1000,
        }
    }

    /// Resolve a keypress event to an action.
    ///
    /// Checks pending chord state first, then single-stroke bindings.
    pub fn resolve(&mut self, key: &Key, mods: Modifiers, now: Instant) -> ResolveResult {
        let binding_key = match key_to_binding_key(key) {
            Some(bk) => bk,
            None => return ResolveResult::Passthrough,
        };

        // ── Chord resolution ────────────────────────────────────────
        if let Some((prefix_mods, prefix_key, ts)) = self.pending_chord.take() {
            let elapsed = now.duration_since(ts).as_millis() as u64;
            if elapsed <= self.chord_timeout_ms {
                // Escape cancels chord
                if binding_key == BindingKey::Named(BindingNamedKey::Escape) {
                    return ResolveResult::Passthrough;
                }
                // Look up chord completion
                if let Some(action) = self
                    .chord_bindings
                    .get(&(prefix_mods, prefix_key.clone(), mods, binding_key.clone()))
                {
                    return ResolveResult::ChordAction(action.clone());
                }
                // No chord match — the chord prefix was consumed; treat
                // this key as a fresh keypress below.
            }
            // Chord timed out or didn't match — fall through to normal lookup
        }

        // ── Single-stroke lookup ────────────────────────────────────
        if let Some(action) = self.bindings.get(&(mods, binding_key.clone())) {
            return ResolveResult::Action(action.clone());
        }

        // ── Check if this key is a chord prefix ─────────────────────
        // A key is a chord prefix if there exist chord_bindings that
        // start with (mods, binding_key).
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

    /// Load bindings from TOML `[[keyboard.bindings]]` raw data.
    /// User bindings override defaults for the same key combo.
    /// Unknown actions or unparseable keys are silently skipped (with log::warn).
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

    /// GAP-6.4: register user-defined `[[commands]]` as keymap bindings.
    ///
    /// Each command with a `key` becomes an `Action::CustomCommand(id)`
    /// binding. Commands without a `key` are palette-only and skipped here.
    ///
    /// Letter-key normalization (mirrors how the default bindings are stored
    /// so user shortcuts match the actual key events the OS delivers):
    /// - **Shift + letter** → register both forms, since macOS may report
    ///   the event as lowercase+shift OR uppercase without shift;
    /// - **no Shift + letter** → store lowercase (the OS reports unshifted
    ///   letters as lowercase), regardless of the case the user wrote.
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

            // Letter-key normalization (see method doc).
            if let BindingKey::Character(ch) = &key {
                if ch.len() == 1 && ch.chars().next().unwrap().is_ascii_alphabetic() {
                    let c0 = ch.chars().next().unwrap();
                    if mods.contains(Modifiers::SHIFT) {
                        // macOS dual-form: report lowercase+shift OR uppercase
                        // without shift. Register both so the binding fires
                        // regardless of which form the OS delivers.
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
                        // Unshifted letter: the OS reports it lowercase, so
                        // store lowercase to match (mirrors the defaults).
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

    /// Return a snapshot of the current bindings for display / CLI listing.
    pub fn list_bindings(&self) -> Vec<(Modifiers, BindingKey, Action)> {
        self.bindings
            .iter()
            .map(|((mods, bk), action)| (*mods, bk.clone(), action.clone()))
            .collect()
    }
}
