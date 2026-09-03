use std::mem::transmute;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::runtime::{AnyClass, AnyObject, Imp};
use objc2::{ffi, MainThreadMarker};

/// 当 macOS 在委托上调用 `applicationShouldHandleReopen:hasVisibleWindows:`
///(即点击 Dock 图标)时置为 `true`。
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

/// 给 winit 的委托类添加 `applicationShouldHandleReopen:hasVisibleWindows:`。
/// 必须在主线程、且 winit 已设置委托之后调用。
pub fn add_reopen_method() {
    let Some(_mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(class) = AnyClass::get(c"WinitApplicationDelegate") else {
        return;
    };

    let sel = objc2::sel!(applicationShouldHandleReopen:hasVisibleWindows:);
    let types = b"B@:@B\0";
    let imp: Imp = unsafe { transmute(handle_reopen as *const () as usize) };

    unsafe {
        let _success = ffi::class_addMethod(
            class as *const _ as *mut _,
            sel,
            imp,
            types.as_ptr() as *const i8,
        );
    }
}

/// 检查 Dock 图标是否被点击,并消费该标志。
/// 每次 Dock 点击恰好返回一次 `true`。
pub fn consume_dock_reopen() -> bool {
    DOCK_REOPEN_REQUESTED.swap(false, Ordering::SeqCst)
}
