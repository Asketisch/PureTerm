use std::mem::transmute;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::runtime::{AnyClass, AnyObject, Imp};
use objc2::{ffi, MainThreadMarker};

/// Set to `true` when macOS calls `applicationShouldHandleReopen:hasVisibleWindows:`
/// on the delegate (Dock icon click).
static DOCK_REOPEN_REQUESTED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn handle_reopen(
    _obj: *mut AnyObject,
    _sel: objc2::runtime::Sel,
    _sender: *mut AnyObject,
    _has_visible_windows: bool,
) -> bool {
    DOCK_REOPEN_REQUESTED.store(true, Ordering::SeqCst);
    true
}

/// Add `applicationShouldHandleReopen:hasVisibleWindows:` to winit's delegate class.
/// Must be called on the main thread, AFTER winit has set its delegate.
pub fn add_reopen_method() {
    let Some(_mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(class) = AnyClass::get(c"WinitApplicationDelegate") else {
        return;
    };

    let sel = objc2::sel!(applicationShouldHandleReopen:hasVisibleWindows:);
    let types = b"B@:@B\0";
    let imp: Imp = unsafe { transmute(handle_reopen as usize) };

    unsafe {
        let _success = ffi::class_addMethod(
            class as *const _ as *mut _,
            sel,
            imp,
            types.as_ptr() as *const i8,
        );
    }
}

/// Check whether the Dock icon was clicked, consuming the flag.
/// Returns `true` exactly once per Dock click.
pub fn consume_dock_reopen() -> bool {
    DOCK_REOPEN_REQUESTED.swap(false, Ordering::SeqCst)
}
