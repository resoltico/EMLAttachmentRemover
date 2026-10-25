//! Keep native Quit immediate when idle and confirm only during a run or export.
#![expect(
    unsafe_code,
    reason = "The bounded Quit adapter owns its Objective-C boundary"
)]
use objc2::{
    Encode, MainThreadMarker, ffi,
    runtime::{AnyClass, AnyObject, Imp, Sel},
    sel,
};
use objc2_app_kit::NSApplication;
use std::{
    ffi::CString,
    sync::atomic::{AtomicBool, Ordering},
};
static BUSY: AtomicBool = AtomicBool::new(false);
static QUIT: AtomicBool = AtomicBool::new(false);
pub(super) fn set_busy(busy: bool) {
    BUSY.store(busy, Ordering::Release);
}
pub(super) fn take_quit() -> bool {
    QUIT.swap(false, Ordering::AcqRel)
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
    Ok(())
}
