//! Input handling for the terminal.
//!
//! This module handles keyboard, mouse, and paste input encoding.

pub mod key;
pub mod mods;
pub mod mouse;
pub mod paste;
pub mod binding;
pub mod key_encode;

pub use key::{Key, KeyAction, KeyEvent};
pub use mods::Mods;
pub use mouse::{MouseButton, MouseEvent, MouseEncoding};
pub use paste::PasteEncoding;
pub use binding::{Binding, BindingAction, BindingFlags, BindingSet, Trigger, TriggerKey};
pub use key_encode::encode_key_event;
