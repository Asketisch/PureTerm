use objc2::runtime::AnyObject;
use objc2_foundation::{NSDictionary, NSString, NSUserDefaults, ns_string};

pub mod locale;
pub mod proc;

pub fn disable_autofill() {
    unsafe {
        NSUserDefaults::standardUserDefaults().registerDefaults(
            &NSDictionary::<NSString, AnyObject>::from_slices(
                &[ns_string!("NSAutoFillHeuristicControllerEnabled")],
                &[ns_string!("NO")],
            ),
        );
}
}

/// Set up the macOS application menu.
///
/// winit provides a default app menu (About, Hide, Quit). We don't override
/// it here because custom NSMenu action selectors require `define_class!`
/// (objc2 0.6) which adds significant complexity. Instead, settings are
/// accessed via `Cmd+,` keybinding handled through the winit event loop.
pub fn setup_menu() {
    // Currently a no-op: winit's default menu is sufficient.
    // The `Cmd+,` shortcut is handled via winit's keyboard event → Action::OpenSettings.
}
