use std::mem::transmute;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::runtime::{AnyClass, AnyObject, Imp};
use objc2::{ffi, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

/// 当 macOS 编辑菜单 → 粘贴被触发时置为 `true`。
static PASTE_REQUESTED: AtomicBool = AtomicBool::new(false);
/// 当 macOS 编辑菜单 → 拷贝被触发时置为 `true`。
static COPY_REQUESTED: AtomicBool = AtomicBool::new(false);
/// 当 macOS 编辑菜单 → 全选被触发时置为 `true`。
static SELECT_ALL_REQUESTED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn handle_paste(
    _obj: *mut AnyObject,
    _sel: objc2::runtime::Sel,
    _sender: *mut AnyObject,
) {
    PASTE_REQUESTED.store(true, Ordering::SeqCst);
}

unsafe extern "C" fn handle_copy(
    _obj: *mut AnyObject,
    _sel: objc2::runtime::Sel,
    _sender: *mut AnyObject,
) {
    COPY_REQUESTED.store(true, Ordering::SeqCst);
}

unsafe extern "C" fn handle_select_all(
    _obj: *mut AnyObject,
    _sel: objc2::runtime::Sel,
    _sender: *mut AnyObject,
) {
    SELECT_ALL_REQUESTED.store(true, Ordering::SeqCst);
}

/// 安装一个含编辑子菜单(粘贴 / 拷贝 / 全选)的精简 macOS 菜单栏。
///
/// 必须在主线程调用,**在** winit 已设置好它的
/// `WinitApplicationDelegate` **之后**。菜单项选择器被添加到该
/// 委托类上,从而沿标准响应链触发。
pub fn install_edit_menu() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    // 向 winit 的委托类添加 paste:/copy:/selectAll: 方法。
    if let Some(class) = AnyClass::get(c"WinitApplicationDelegate") {
        let types = b"v@:@\0";

        let imp_paste: Imp = unsafe { transmute(handle_paste as *const ()) };
        let imp_copy: Imp = unsafe { transmute(handle_copy as *const ()) };
        let imp_selall: Imp = unsafe { transmute(handle_select_all as *const ()) };

        unsafe {
            let _ = ffi::class_addMethod(
                class as *const _ as *mut _,
                objc2::sel!(paste:),
                imp_paste,
                types.as_ptr() as *const i8,
            );
            let _ = ffi::class_addMethod(
                class as *const _ as *mut _,
                objc2::sel!(copy:),
                imp_copy,
                types.as_ptr() as *const i8,
            );
            let _ = ffi::class_addMethod(
                class as *const _ as *mut _,
                objc2::sel!(selectAll:),
                imp_selall,
                types.as_ptr() as *const i8,
            );
        }
    }

    let app = NSApplication::sharedApplication(mtm);

    // ── 菜单栏 ───────────────────────────────────────────────────────
    let menubar = NSMenu::new(mtm);

    // ── 应用菜单(每个 macOS 应用都需要) ─────────────────────────────────────
    let app_menu_title = NSString::from_str("PureTerm");
    let app_menu = unsafe {
        NSMenu::initWithTitle(NSMenu::alloc(mtm), &app_menu_title)
    };
    let app_menu_item = NSMenuItem::new(mtm);
    app_menu_item.setSubmenu(Some(&app_menu));

    // 隐藏 PureTerm  ⌘H
    let hide = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Hide PureTerm"),
            Some(objc2::sel!(hide:)),
            &NSString::from_str("h"),
        )
    };
    app_menu.addItem(&hide);

    // 退出 PureTerm  ⌘Q
    let quit = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Quit PureTerm"),
            Some(objc2::sel!(terminate:)),
            &NSString::from_str("q"),
        )
    };
    app_menu.addItem(&quit);

    // ── 编辑菜单 ─────────────────────────────────────────────────────
    let edit_menu_title = NSString::from_str("Edit");
    let edit_menu = unsafe {
        NSMenu::initWithTitle(NSMenu::alloc(mtm), &edit_menu_title)
    };
    let edit_menu_item = NSMenuItem::new(mtm);
    edit_menu_item.setTitle(&NSString::from_str("Edit"));
    edit_menu_item.setSubmenu(Some(&edit_menu));

    // 粘贴  ⌘V
    let paste = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Paste"),
            Some(objc2::sel!(paste:)),
            &NSString::from_str("v"),
        )
    };
    edit_menu.addItem(&paste);

    // 拷贝  ⌘C
    let copy = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Copy"),
            Some(objc2::sel!(copy:)),
            &NSString::from_str("c"),
        )
    };
    edit_menu.addItem(&copy);

    // 全选  ⌘A
    let select_all = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Select All"),
            Some(objc2::sel!(selectAll:)),
            &NSString::from_str("a"),
        )
    };
    edit_menu.addItem(&select_all);

    menubar.addItem(&app_menu_item);
    menubar.addItem(&edit_menu_item);

    app.setMainMenu(Some(&menubar));
}

/// 检查编辑 → 粘贴是否被触发,并消费该标志。
pub fn consume_paste() -> bool {
    PASTE_REQUESTED.swap(false, Ordering::SeqCst)
}

/// 检查编辑 → 拷贝是否被触发,并消费该标志。
pub fn consume_copy() -> bool {
    COPY_REQUESTED.swap(false, Ordering::SeqCst)
}

/// 检查编辑 → 全选是否被触发,并消费该标志。
pub fn consume_select_all() -> bool {
    SELECT_ALL_REQUESTED.swap(false, Ordering::SeqCst)
}
