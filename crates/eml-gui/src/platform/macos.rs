//! Admit native Quit and receive document-opening events.
#![expect(
    unsafe_code,
    reason = "The Quit and document-opening adapters own their Objective-C boundary"
)]
use objc2::{
    Encode, MainThreadMarker,
    encode::EncodeReturn,
    ffi, msg_send,
    runtime::{AnyClass, AnyObject, Imp, Sel},
    sel,
};
use objc2_app_kit::NSApplication;
use std::{
    ffi::{CStr, CString},
    os::unix::ffi::OsStringExt,
    path::PathBuf,
    sync::Mutex,
    sync::atomic::{AtomicBool, Ordering},
};
static BUSY: AtomicBool = AtomicBool::new(false);
static QUIT: AtomicBool = AtomicBool::new(false);
static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
pub(super) fn take_opened() -> Vec<PathBuf> {
    OPENED
        .lock()
        .map(|mut paths| std::mem::take(&mut *paths))
        .unwrap_or_default()
}
extern "C" fn open_urls(_: &AnyObject, _: Sel, _: &NSApplication, urls: &AnyObject) {
    // Only immutable foreign-array reads cross the unwind boundary; all Rust
    // mutations are callback-local or protected by the queue mutex.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: NSApplicationDelegate supplies NSArray<NSURL *> for this exact
        // selector. Each object is borrowed for this callback; no pointer escapes.
        let count: usize = unsafe { msg_send![urls, count] };
        let mut paths = Vec::new();
        for index in 0..count {
            let url: *mut AnyObject = unsafe { msg_send![urls, objectAtIndex: index] };
            if url.is_null() {
                continue;
            }
            let file: objc2::runtime::Bool = unsafe { msg_send![url, isFileURL] };
            if !file.as_bool() {
                continue;
            }
            let absolute: *mut AnyObject = unsafe { msg_send![url, absoluteURL] };
            if absolute.is_null() {
                continue;
            }
            let bytes: *const std::ffi::c_char =
                unsafe { msg_send![absolute, fileSystemRepresentation] };
            if bytes.is_null() {
                continue;
            }
            // NSURL's filesystem representation is NUL-terminated and remains
            // valid for this autorelease scope. Copy losslessly before returning.
            let path = PathBuf::from(std::ffi::OsString::from_vec(
                unsafe { CStr::from_ptr(bytes) }.to_bytes().to_vec(),
            ));
            if path.is_absolute() {
                paths.push(path);
            }
        }
        if let Ok(mut opened) = OPENED.lock() {
            opened.extend(paths);
        }
        let _ = slint::invoke_from_event_loop(|| {});
    }));
}
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
    let main = MainThreadMarker::new().ok_or(crate::wording::QUIT_MAIN_THREAD)?;
    let app = NSApplication::sharedApplication(main);
    let delegate = app.delegate().ok_or(crate::wording::DELEGATE_MISSING)?;
    let object: &AnyObject = (*delegate).as_ref();
    let class = object.class();
    let selector = sel!(applicationShouldTerminate:);
    if class.name() != c"WinitApplicationDelegate" || class.instance_method(selector).is_some() {
        return Err(crate::wording::QUIT_DELEGATE_UNSUPPORTED.into());
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
        return Err(crate::wording::QUIT_INSTALL_FAILED.into());
    }
    let open_selector = sel!(application:openURLs:);
    if class.instance_method(open_selector).is_some() {
        return Err(crate::wording::DOCUMENT_DELEGATE_UNSUPPORTED.into());
    }
    let open_types = CString::new(format!("{}@:@@", <()>::ENCODING_RETURN))?;
    // SAFETY: the absent selector is installed only on the pinned Winit delegate.
    // Its void/object/object ABI matches NSApplicationDelegate and catches unwind.
    let open_implementation = unsafe {
        std::mem::transmute::<extern "C" fn(&AnyObject, Sel, &NSApplication, &AnyObject), Imp>(
            open_urls,
        )
    };
    if !unsafe {
        ffi::class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            open_selector,
            open_implementation,
            open_types.as_ptr(),
        )
    }
    .as_bool()
    {
        return Err(crate::wording::DOCUMENT_INSTALL_FAILED.into());
    }
    Ok(())
}
