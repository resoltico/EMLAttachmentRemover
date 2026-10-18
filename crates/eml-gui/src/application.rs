//! Minimal application lifecycle bridge; preserves Winit's actual delegate.
use objc2::rc::Retained;
use objc2::{
    Encode, MainThreadMarker, ffi,
    runtime::{AnyClass, AnyObject, Bool, Imp, ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::{NSApplication, NSApplicationDelegate};
use std::{
    ffi::CString,
    sync::atomic::{AtomicU8, Ordering},
};
const QUIT: u8 = 1;
const REOPEN: u8 = 2;
static EVENTS: AtomicU8 = AtomicU8::new(0);
extern "C" fn should_terminate(_: &AnyObject, _: Sel, _: &NSApplication) -> usize {
    EVENTS.fetch_or(QUIT, Ordering::Release);
    // Always cancel native termination. The retained application controller
    // confirms intent, settles its actors, then exits Slint's event loop.
    0
}
extern "C" fn should_reopen(_: &AnyObject, _: Sel, _: &NSApplication, _: Bool) -> Bool {
    EVENTS.fetch_or(REOPEN, Ordering::Release);
    Bool::NO
}
extern "C" fn terminate_after_close(_: &AnyObject, _: Sel, _: &NSApplication) -> Bool {
    Bool::NO
}
pub(crate) struct ApplicationHooks {
    delegate: Retained<ProtocolObject<dyn NSApplicationDelegate>>,
    main: MainThreadMarker,
}
pub(crate) struct ApplicationEvents {
    pub(crate) quit: bool,
    pub(crate) reopen: bool,
}
impl ApplicationHooks {
    pub(crate) fn install() -> Result<Self, Box<dyn std::error::Error>> {
        let main = MainThreadMarker::new().ok_or("Application hooks need the main thread")?;
        let app = NSApplication::sharedApplication(main);
        let delegate = app
            .delegate()
            .ok_or("Winit application delegate is absent")?;
        let object: &AnyObject = (*delegate).as_ref();
        let class = object.class();
        if class.name() != c"WinitApplicationDelegate" {
            return Err("Unexpected application delegate class".into());
        }
        let selectors = [
            sel!(applicationShouldTerminate:),
            sel!(applicationShouldHandleReopen:hasVisibleWindows:),
            sel!(applicationShouldTerminateAfterLastWindowClosed:),
        ];
        // Check the complete set before installing anything. No method or
        // existing implementation is replaced, including inherited methods.
        if selectors
            .iter()
            .any(|selector| class.instance_method(*selector).is_some())
        {
            return Err("Application lifecycle selector already has an implementation".into());
        }
        // SAFETY: these functions have exactly the documented delegate ABI;
        // BOOL encodings below derive from objc2 for each native CPU.
        let implementations = unsafe {
            [
                std::mem::transmute::<extern "C" fn(&AnyObject, Sel, &NSApplication) -> usize, Imp>(
                    should_terminate,
                ),
                std::mem::transmute::<
                    extern "C" fn(&AnyObject, Sel, &NSApplication, Bool) -> Bool,
                    Imp,
                >(should_reopen),
                std::mem::transmute::<extern "C" fn(&AnyObject, Sel, &NSApplication) -> Bool, Imp>(
                    terminate_after_close,
                ),
            ]
        };
        let returns = [
            usize::ENCODING.to_string(),
            Bool::ENCODING.to_string(),
            Bool::ENCODING.to_string(),
        ];
        for index in 0..3 {
            let extra = if index == 1 {
                Bool::ENCODING.to_string()
            } else {
                String::new()
            };
            let types = CString::new(format!("{}@:@{extra}", returns[index]))?;
            // SAFETY: only the verified actual delegate class is extended on
            // its main thread, before ingress/workers. class_addMethod refuses
            // an occupied slot; it never changes an existing implementation.
            if !unsafe {
                ffi::class_addMethod(
                    class as *const AnyClass as *mut AnyClass,
                    selectors[index],
                    implementations[index],
                    types.as_ptr(),
                )
            }
            .as_bool()
            {
                return Err("Could not install an absent application lifecycle selector".into());
            }
            let method = class
                .instance_method(selectors[index])
                .ok_or("Installed lifecycle selector disappeared")?;
            let mut arguments = vec!["@", ":", "@"];
            if index == 1 {
                arguments.push(&extra);
            }
            if method.return_type().to_bytes() != returns[index].as_bytes()
                || method.arguments_count() != if index == 1 { 4 } else { 3 }
                || arguments.iter().enumerate().any(|(index, expected)| {
                    method
                        .argument_type(index)
                        .is_none_or(|actual| actual.to_bytes() != expected.as_bytes())
                })
                || !std::ptr::fn_addr_eq(method.implementation(), implementations[index])
            {
                return Err("Application lifecycle implementation or ABI readback differs".into());
            }
        }
        Ok(Self { delegate, main })
    }
    pub(crate) fn take_events(&self) -> Result<ApplicationEvents, &'static str> {
        let app = NSApplication::sharedApplication(self.main);
        if !app
            .delegate()
            .is_some_and(|current| std::ptr::eq(&*current, &*self.delegate))
        {
            return Err("Application delegate ownership changed");
        }
        let events = EVENTS.swap(0, Ordering::AcqRel);
        Ok(ApplicationEvents {
            quit: events & QUIT != 0,
            reopen: events & REOPEN != 0,
        })
    }
}
