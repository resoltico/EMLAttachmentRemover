//! Route the standard application-menu About action and admit native Quit.
#![expect(
    unsafe_code,
    reason = "The bounded application menu and Quit adapters own their Objective-C boundary"
)]
use objc2::{
    Encode, MainThreadMarker, ffi, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, ClassBuilder, Imp, Sel},
    sel,
};
use objc2_app_kit::NSApplication;
use std::{
    cell::OnceCell,
    ffi::CString,
    sync::atomic::{AtomicBool, Ordering},
};
static BUSY: AtomicBool = AtomicBool::new(false);
static QUIT: AtomicBool = AtomicBool::new(false);
static ABOUT: AtomicBool = AtomicBool::new(false);
thread_local! {
    // NSMenuItem does not retain its target. Own it until the UI thread exits.
    static ABOUT_TARGET: OnceCell<Retained<AnyObject>> = const { OnceCell::new() };
}
pub(super) fn set_busy(busy: bool) {
    BUSY.store(busy, Ordering::Release);
}
pub(super) fn take_quit() -> bool {
    QUIT.swap(false, Ordering::AcqRel)
}
pub(super) fn take_about() -> bool {
    ABOUT.swap(false, Ordering::AcqRel)
}
extern "C" fn show_about(_: *mut AnyObject, _: Sel, _: *mut AnyObject) {
    ABOUT.store(true, Ordering::Release);
    let _ = std::panic::catch_unwind(|| slint::invoke_from_event_loop(|| {}));
}
extern "C" fn should_terminate(_: &AnyObject, _: Sel, _: &NSApplication) -> usize {
    if !BUSY.load(Ordering::Acquire) {
        return 1;
    }
    QUIT.store(true, Ordering::Release);
    let _ = std::panic::catch_unwind(|| slint::invoke_from_event_loop(|| {}));
    0
}
pub(super) fn install() -> Result<(), Box<dyn std::error::Error>> {
    let main = MainThreadMarker::new().ok_or("Quit adapter requires the main thread")?;
    let app = NSApplication::sharedApplication(main);
    let delegate = app
        .delegate()
        .ok_or("Winit application delegate is absent")?;
    let object: &AnyObject = (*delegate).as_ref();
    let class = object.class();
    let selector = sel!(applicationShouldTerminate:);
    if class.name() != c"WinitApplicationDelegate" || class.instance_method(selector).is_some() {
        return Err("Cannot install the Quit adapter on this application delegate".into());
    }
    let types = CString::new(format!("{}@:@", usize::ENCODING))?;
    // SAFETY: exact NSUInteger-returning NSApplicationDelegate signature; the
    // callback catches unwinding and the encoding derives the native integer ABI.
    let implementation = unsafe {
        std::mem::transmute::<extern "C" fn(&AnyObject, Sel, &NSApplication) -> usize, Imp>(
            should_terminate,
        )
    };
    // SAFETY: verified actual delegate class, absent selector, matching ABI;
    // class_addMethod refuses an occupied slot and no delegate is replaced.
    if !unsafe {
        ffi::class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            selector,
            implementation,
            types.as_ptr(),
        )
    }
    .as_bool()
    {
        return Err("Could not install the Quit adapter".into());
    }
    let about_selector = sel!(emlShowAbout:);
    let base = AnyClass::get(c"NSObject").ok_or("NSObject is unavailable")?;
    let mut about_class = ClassBuilder::new(c"EmlAboutMenuTarget", base)
        .ok_or("Cannot register the application About target")?;
    // SAFETY: NSObject has no subclassing requirements; this new application-
    // owned class implements a void-returning object-sender action. No toolkit
    // class or method is replaced; ClassBuilder derives the matching encoding.
    unsafe {
        about_class.add_method(
            about_selector,
            show_about as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
        );
    }
    let about_class = about_class.register();
    ABOUT_TARGET.with(|target| {
        target.get_or_init(|| {
            // SAFETY: NSObject's inherited +new returns an owned initialized
            // instance of our registered class, retained for the menu lifetime.
            unsafe { msg_send![about_class, new] }
        });
    });
    refresh_menu();
    Ok(())
}
/// Slint installs the focused window's menu, so route that menu's standard About
/// item after focus/menu changes as part of the existing application event tick.
pub(super) fn refresh_menu() {
    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(main);
    let Some(item) = app
        .mainMenu()
        .and_then(|menu| menu.itemAtIndex(0))
        .and_then(|item| item.submenu())
        .and_then(|menu| menu.itemAtIndex(0))
    else {
        return;
    };
    let action = sel!(emlShowAbout:);
    if item.action() == Some(action) {
        return;
    }
    // Both pinned Winit and Slint/Muda place their standard About action first
    // in the application submenu. Do not retarget any unrelated menu command.
    if !item.title().to_string().starts_with("About ") || item.action().is_none() {
        return;
    }
    ABOUT_TARGET.with(|target| {
        if let Some(target) = target.get() {
            // SAFETY: the installed action has the object-sender ABI and its
            // application-owned target remains retained for the UI thread lifetime.
            unsafe {
                item.setTarget(Some(target));
                item.setAction(Some(action));
            }
        }
    });
}
