use std::mem::transmute;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::runtime::{AnyClass, AnyObject, Imp};
use objc2::{ffi, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

/// Set to `true` when macOS Edit → Paste is triggered.
static PASTE_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Set to `true` when macOS Edit → Copy is triggered.
static COPY_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Set to `true` when macOS Edit → Select All is triggered.
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

/// Install a minimal macOS menu bar with an Edit submenu (Paste / Copy / Select All).
///
/// This must be called on the main thread, **after** winit has set up its
/// `WinitApplicationDelegate`. The menu-item selectors are added to that
/// delegate class so they fire through the standard responder chain.
pub fn install_edit_menu() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    // Add paste:/copy:/selectAll: methods to winit's delegate class.
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

    // ── Menu bar ──────────────────────────────────────────────
    let menubar = NSMenu::new(mtm);

    // ── Application menu (required for every macOS app) ──────
    let app_menu_title = NSString::from_str("PureTerm");
    let app_menu = unsafe {
        NSMenu::initWithTitle(NSMenu::alloc(mtm), &app_menu_title)
    };
    let app_menu_item = NSMenuItem::new(mtm);
    app_menu_item.setSubmenu(Some(&app_menu));

    // Hide PureTerm  ⌘H
    let hide = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Hide PureTerm"),
            Some(objc2::sel!(hide:)),
            &NSString::from_str("h"),
        )
    };
    app_menu.addItem(&hide);

    // Quit PureTerm  ⌘Q
    let quit = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Quit PureTerm"),
            Some(objc2::sel!(terminate:)),
            &NSString::from_str("q"),
        )
    };
    app_menu.addItem(&quit);

    // ── Edit menu ────────────────────────────────────────────
    let edit_menu_title = NSString::from_str("Edit");
    let edit_menu = unsafe {
        NSMenu::initWithTitle(NSMenu::alloc(mtm), &edit_menu_title)
    };
    let edit_menu_item = NSMenuItem::new(mtm);
    edit_menu_item.setTitle(&NSString::from_str("Edit"));
    edit_menu_item.setSubmenu(Some(&edit_menu));

    // Paste  ⌘V
    let paste = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Paste"),
            Some(objc2::sel!(paste:)),
            &NSString::from_str("v"),
        )
    };
    edit_menu.addItem(&paste);

    // Copy  ⌘C
    let copy = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Copy"),
            Some(objc2::sel!(copy:)),
            &NSString::from_str("c"),
        )
    };
    edit_menu.addItem(&copy);

    // Select All  ⌘A
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

/// Check whether Edit → Paste was triggered, consuming the flag.
pub fn consume_paste() -> bool {
    PASTE_REQUESTED.swap(false, Ordering::SeqCst)
}

/// Check whether Edit → Copy was triggered, consuming the flag.
pub fn consume_copy() -> bool {
    COPY_REQUESTED.swap(false, Ordering::SeqCst)
}

/// Check whether Edit → Select All was triggered, consuming the flag.
pub fn consume_select_all() -> bool {
    SELECT_ALL_REQUESTED.swap(false, Ordering::SeqCst)
}
