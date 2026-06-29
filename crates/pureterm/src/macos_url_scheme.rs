//! GAP-48: macOS `pureterm://` URL handler — hooks `application:openURLs:` on
//! winit's delegate and queues URLs for the iced `TerminalWakeup` tick.

use std::mem::transmute;
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp};
use objc2::{ffi, MainThreadMarker};
use objc2_foundation::{NSArray, NSURL};

static PENDING_URLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Take the next queued `pureterm://` URL, if any.
pub fn consume_pending_url() -> Option<String> {
    let mut guard = PENDING_URLS.lock().unwrap();
    if guard.is_empty() {
        None
    } else {
        Some(guard.remove(0))
    }
}

unsafe extern "C" fn handle_open_urls(
    _obj: *mut AnyObject,
    _sel: objc2::runtime::Sel,
    _app: *mut AnyObject,
    urls: *mut AnyObject,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let urls = &*(urls as *const Retained<NSArray<NSURL>>);
    for url in urls.iter() {
        let Some(abs) = url.absoluteString() else {
            continue;
        };
        let s = abs.to_string();
        if s.starts_with("pureterm://") {
            PENDING_URLS.lock().unwrap().push(s);
        }
    }
    let _ = mtm;
}

/// Add `application:openURLs:` to winit's delegate class.
pub fn add_open_urls_method() {
    let Some(_mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(class) = AnyClass::get(c"WinitApplicationDelegate") else {
        return;
    };

    let sel = objc2::sel!(application:openURLs:);
    let types = b"v@:@@\0";
    let imp: Imp = unsafe { transmute(handle_open_urls as usize) };

    unsafe {
        let _ = ffi::class_addMethod(
            class as *const _ as *mut _,
            sel,
            imp,
            types.as_ptr() as *const i8,
        );
    }
}

/// GAP-48: register PureTerm as the default handler for `pureterm://`.
/// Requires a bundled `.app` with `CFBundleURLTypes` in Info.plist.
#[cfg(target_os = "macos")]
pub fn register_default_handler() -> Result<(), String> {
    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn LSSetDefaultHandlerForURLScheme(
            scheme: *const std::ffi::c_void,
            bundle_id: *const std::ffi::c_void,
        ) -> i32;
    }

    use objc2::rc::Retained;

    let scheme = objc2_foundation::NSString::from_str("pureterm");
    let bundle_id = objc2_foundation::NSString::from_str("com.pureterm.app");
    let status = unsafe {
        LSSetDefaultHandlerForURLScheme(
            Retained::as_ptr(&scheme) as *const std::ffi::c_void,
            Retained::as_ptr(&bundle_id) as *const std::ffi::c_void,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(format!(
            "LSSetDefaultHandlerForURLScheme failed (status {status}); \
             install PureTerm.app with CFBundleURLTypes first"
        ))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn register_default_handler() -> Result<(), String> {
    Err("url scheme registration is macOS-only".into())
}

#[cfg(not(target_os = "macos"))]
pub fn consume_pending_url() -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn add_open_urls_method() {}
