//! GAP-48:macOS `pureterm://` URL 处理——在 winit 委托上
//! 钩住 `application:openURLs:` 并把 URL 排队给 iced 的 `TerminalWakeup` tick。

#[cfg(target_os = "macos")]
use std::mem::transmute;
#[cfg(target_os = "macos")]
use std::sync::Mutex;

#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::runtime::{AnyClass, AnyObject, Imp};
#[cfg(target_os = "macos")]
use objc2::{ffi, MainThreadMarker};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSArray, NSURL};

#[cfg(target_os = "macos")]
static PENDING_URLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// 取出下一个排队的 `pureterm://` URL(若有)。
#[cfg(target_os = "macos")]
pub fn consume_pending_url() -> Option<String> {
    let mut guard = PENDING_URLS.lock().unwrap();
    if guard.is_empty() {
        None
    } else {
        Some(guard.remove(0))
    }
}

#[cfg(target_os = "macos")]
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

/// 给 winit 的委托类添加 `application:openURLs:`。
#[cfg(target_os = "macos")]
pub fn add_open_urls_method() {
    let Some(_mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(class) = AnyClass::get(c"WinitApplicationDelegate") else {
        return;
    };

    let sel = objc2::sel!(application:openURLs:);
    let types = b"v@:@@\0";
    let imp: Imp = unsafe { transmute(handle_open_urls as *const () as usize) };

    unsafe {
        let _ = ffi::class_addMethod(
            class as *const _ as *mut _,
            sel,
            imp,
            types.as_ptr() as *const i8,
        );
    }
}

/// GAP-48:把 PureTerm 注册为 `pureterm://` 的默认处理器。
/// 需要 Info.plist 中带 `CFBundleURLTypes` 的捆绑 `.app`。
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
