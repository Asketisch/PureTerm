//! GAP-21: macOS Dock right-click menu — recent workspaces list.
//!
//! Mirrors `macos_reopen.rs` / `macos_menu.rs`: hooks into winit's
//! `WinitApplicationDelegate` via `class_addMethod`. Workspace picks are
//! queued atomically and consumed on the iced `TerminalWakeup` tick.

use std::mem::transmute;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp};
use objc2::{ffi, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSMenu, NSMenuItem};
use objc2_foundation::NSString;

/// Sentinel: no pending workspace switch.
const NO_PENDING: u64 = u64::MAX;

static WORKSPACE_ENTRIES: Mutex<Vec<(u64, String)>> = Mutex::new(Vec::new());
static PENDING_WORKSPACE_ID: AtomicU64 = AtomicU64::new(NO_PENDING);

/// Replace the workspace entries shown in the Dock menu (most recent first).
/// Skips work when the list is unchanged.
pub fn set_workspace_entries(entries: Vec<(u64, String)>) {
    let mut guard = WORKSPACE_ENTRIES.lock().unwrap();
    if *guard == entries {
        return;
    }
    *guard = entries;
}

/// Take a pending workspace switch requested from the Dock menu, if any.
pub fn consume_workspace_switch() -> Option<u64> {
    let id = PENDING_WORKSPACE_ID.swap(NO_PENDING, Ordering::SeqCst);
    if id == NO_PENDING {
        None
    } else {
        Some(id)
    }
}

unsafe extern "C" fn handle_dock_workspace(
    _obj: *mut AnyObject,
    _sel: objc2::runtime::Sel,
    sender: *mut AnyObject,
) {
    let item = &*(sender as *const NSMenuItem);
    let tag = item.tag();
    if tag >= 0 {
        PENDING_WORKSPACE_ID.store(tag as u64, Ordering::SeqCst);
    }
}

unsafe extern "C" fn handle_dock_menu(
    _obj: *mut AnyObject,
    _sel: objc2::runtime::Sel,
    _app: *mut AnyObject,
) -> *mut AnyObject {
    let Some(mtm) = MainThreadMarker::new() else {
        return std::ptr::null_mut();
    };

    let entries = WORKSPACE_ENTRIES.lock().unwrap().clone();
    if entries.is_empty() {
        return std::ptr::null_mut();
    }

    let menu = NSMenu::new(mtm);
    for (id, name) in entries {
        let title = NSString::from_str(&name);
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &title,
                Some(objc2::sel!(puretermDockWorkspace:)),
                &NSString::from_str(""),
            )
        };
        item.setTag(id as isize);
        menu.addItem(&item);
    }

    Retained::into_raw(menu) as *mut AnyObject
}

/// Add `applicationDockMenu:` + `puretermDockWorkspace:` to winit's delegate class.
/// Must be called on the main thread, AFTER winit has set its delegate.
pub fn add_dock_menu_methods() {
    let Some(_mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(class) = AnyClass::get(c"WinitApplicationDelegate") else {
        return;
    };

    let types_void = b"v@:@\0";
    let imp_workspace: Imp = unsafe { transmute(handle_dock_workspace as usize) };
    unsafe {
        let _ = ffi::class_addMethod(
            class as *const _ as *mut _,
            objc2::sel!(puretermDockWorkspace:),
            imp_workspace,
            types_void.as_ptr() as *const i8,
        );
    }

    let types_menu = b"@@:@\0";
    let imp_menu: Imp = unsafe { transmute(handle_dock_menu as usize) };
    unsafe {
        let _ = ffi::class_addMethod(
            class as *const _ as *mut _,
            objc2::sel!(applicationDockMenu:),
            imp_menu,
            types_menu.as_ptr() as *const i8,
        );
    }
}
