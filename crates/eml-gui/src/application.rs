//! Necessary application lifecycle, file-open batches and Services; preserves Winit's delegate.
use crate::native::{self, NativeError};
use objc2::rc::Retained;
use objc2::{
    Encode, MainThreadMarker, ffi,
    runtime::{AnyClass, AnyObject, Bool, Imp, ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::{NSApplication, NSApplicationDelegate, NSPasteboard};
use objc2_foundation::{NSArray, NSString, ns_string};
use std::{
    cell::RefCell,
    collections::VecDeque,
    ffi::CString,
    path::PathBuf,
    sync::atomic::{AtomicU8, Ordering},
};
const QUIT: u8 = 1;
const REOPEN: u8 = 2;
const BROKEN: u8 = 4;
static EVENTS: AtomicU8 = AtomicU8::new(0);
fn wake_event_loop() {
    // Native application callbacks can arrive with every window hidden. Post a
    // real backend event rather than relying only on a pending polling timer.
    // Keep a platform/library failure inside the native ABI boundary.
    if !std::panic::catch_unwind(|| slint::invoke_from_event_loop(|| {}))
        .is_ok_and(|result| result.is_ok())
    {
        EVENTS.fetch_or(BROKEN, Ordering::Release);
    }
}
#[derive(Default)]
struct PendingIngress {
    // Native callbacks retain at most the waiting-selection limit before the
    // next UI turn. Only DesktopSession can admit a request/start a worker.
    batches: VecDeque<Result<Vec<PathBuf>, NativeError>>,
    collecting: bool,
    refusal: Option<NativeError>,
}
impl PendingIngress {
    fn begin(&mut self) -> Result<(), NativeError> {
        let refusal = if self.collecting {
            Some(NativeError::IngressUnavailable)
        } else if self.batches.len() >= eml_app::MAX_WAITING_REQUESTS {
            Some(NativeError::SelectionLimit)
        } else {
            None
        };
        if let Some(error) = refusal {
            self.refusal.get_or_insert(error);
            return Err(error);
        }
        self.collecting = true;
        Ok(())
    }
    fn complete(&mut self, batch: Result<Vec<PathBuf>, NativeError>) -> Result<(), NativeError> {
        self.collecting = false;
        let failure = batch.as_ref().err().copied();
        self.batches.push_back(batch);
        failure.map_or(Ok(()), Err)
    }
}
thread_local! { static INGRESS: RefCell<PendingIngress> = RefCell::new(PendingIngress::default()); }
fn deliver(collect: impl FnOnce() -> Result<Vec<PathBuf>, NativeError>) -> Result<(), NativeError> {
    collect_native(collect, true)
}
fn collect_native(
    collect: impl FnOnce() -> Result<Vec<PathBuf>, NativeError>,
    commit: bool,
) -> Result<(), NativeError> {
    if MainThreadMarker::new().is_none() {
        EVENTS.fetch_or(BROKEN, Ordering::Release);
        return Err(NativeError::IngressUnavailable);
    }
    INGRESS.with(|pending| {
        pending
            .try_borrow_mut()
            .map_err(|_| NativeError::IngressUnavailable)?
            .begin()
    })?;
    let collected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(collect));
    let result = INGRESS.with(|pending| {
        let mut pending = pending
            .try_borrow_mut()
            .map_err(|_| NativeError::IngressUnavailable)?;
        match collected {
            Ok(batch) if commit => pending.complete(batch),
            Ok(batch) => {
                pending.collecting = false;
                batch.map(|_| ())
            }
            Err(_) => {
                pending.collecting = false;
                EVENTS.fetch_or(BROKEN, Ordering::Release);
                Err(NativeError::IngressUnavailable)
            }
        }
    });
    if commit {
        wake_event_loop();
    }
    result
}

pub(crate) fn receive_drop(board: *mut std::ffi::c_void, commit: bool) -> bool {
    match std::panic::catch_unwind(|| {
        collect_native(
            || {
                // SAFETY: the pinned Winit bridge retains this native pasteboard for
                // this synchronous main-thread callback. No pointer escapes; native
                // bytes are copied only after the common bounded-batch reservation.
                let board = unsafe { board.cast::<NSPasteboard>().as_ref() }
                    .ok_or(NativeError::LocalFilesRequired)?;
                native::paths_from_pasteboard(board)
            },
            commit,
        )
    }) {
        Ok(result) => result.is_ok(),
        Err(_) => {
            EVENTS.fetch_or(BROKEN, Ordering::Release);
            false
        }
    }
}

extern "C" fn should_terminate(_: &AnyObject, _: Sel, _: &NSApplication) -> usize {
    EVENTS.fetch_or(QUIT, Ordering::Release);
    wake_event_loop();
    // Native termination is cancelled until the Slint controller confirms
    // intent, settles its actors, and exits the event loop.
    0
}
extern "C" fn should_reopen(_: &AnyObject, _: Sel, _: &NSApplication, _: Bool) -> Bool {
    EVENTS.fetch_or(REOPEN, Ordering::Release);
    wake_event_loop();
    Bool::NO
}
extern "C" fn terminate_after_close(_: &AnyObject, _: Sel, _: &NSApplication) -> Bool {
    Bool::NO
}
extern "C" fn open_urls(
    _: &AnyObject,
    _: Sel,
    _: &NSApplication,
    urls: Option<&NSArray<AnyObject>>,
) {
    let _ = deliver(|| native::paths_from_objects(urls.ok_or(NativeError::LocalFilesRequired)?));
}
extern "C" fn create_copies(
    _: &AnyObject,
    _: Sel,
    board: Option<&NSPasteboard>,
    _: Option<&NSString>,
    error: *mut *const NSString,
) {
    if deliver(|| native::paths_from_pasteboard(board.ok_or(NativeError::LocalFilesRequired)?))
        .is_err()
        && !error.is_null()
    {
        // SAFETY: Services supplies an optional writable NSString* error slot.
        // The immutable constant has process lifetime; no borrowed String escapes.
        unsafe {
            error.write(ns_string!("Choose only local file URLs within the supported selection limits. Open EML Attachment Remover to inspect the refusal."));
        }
    }
}
pub(crate) struct ApplicationHooks {
    delegate: Retained<ProtocolObject<dyn NSApplicationDelegate>>,
    main: MainThreadMarker,
}
pub(crate) struct ApplicationEvents {
    pub(crate) quit: bool,
    pub(crate) reopen: bool,
    pub(crate) selections: VecDeque<Result<Vec<PathBuf>, NativeError>>,
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
            sel!(application:openURLs:),
            sel!(createEMLCopies:userData:error:),
        ];
        // Check all slots first, including inherited implementations. Installation
        // adds only absent methods; no implementation or delegate is replaced.
        if selectors
            .iter()
            .any(|selector| class.instance_method(*selector).is_some())
        {
            return Err("Application integration selector already has an implementation".into());
        }
        // SAFETY: these C function signatures match the documented delegate /
        // Services ABI. Native BOOL encodings derive from objc2 for each CPU.
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
                std::mem::transmute::<
                    extern "C" fn(&AnyObject, Sel, &NSApplication, Option<&NSArray<AnyObject>>),
                    Imp,
                >(open_urls),
                std::mem::transmute::<
                    extern "C" fn(
                        &AnyObject,
                        Sel,
                        Option<&NSPasteboard>,
                        Option<&NSString>,
                        *mut *const NSString,
                    ),
                    Imp,
                >(create_copies),
            ]
        };
        let native_bool = Bool::ENCODING.to_string();
        let returns = [
            usize::ENCODING.to_string(),
            native_bool.clone(),
            native_bool.clone(),
            "v".into(),
            "v".into(),
        ];
        let arguments = [
            vec!["@", ":", "@"],
            vec!["@", ":", "@", &native_bool],
            vec!["@", ":", "@"],
            vec!["@", ":", "@", "@"],
            vec!["@", ":", "@", "@", "^@"],
        ];
        for index in 0..selectors.len() {
            let types = CString::new(format!("{}{}", returns[index], arguments[index].join("")))?;
            // SAFETY: verified actual delegate class on its main thread, before
            // file ingress/processing. class_addMethod refuses an occupied slot.
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
                return Err("Could not add an absent application integration selector".into());
            }
            let method = class
                .instance_method(selectors[index])
                .ok_or("Installed integration selector disappeared")?;
            if method.return_type().to_bytes() != returns[index].as_bytes()
                || method.arguments_count() != arguments[index].len()
                || arguments[index]
                    .iter()
                    .enumerate()
                    .any(|(index, expected)| {
                        method
                            .argument_type(index)
                            .is_none_or(|actual| actual.to_bytes() != expected.as_bytes())
                    })
                || !std::ptr::fn_addr_eq(method.implementation(), implementations[index])
            {
                return Err(
                    "Application integration implementation or ABI readback differs".into(),
                );
            }
        }
        if app.servicesProvider().is_some() {
            return Err("Application Services provider is already occupied".into());
        }
        // SAFETY: this retained object implements the exact checked Services
        // entry point above and remains owned through the application event loop.
        unsafe {
            app.setServicesProvider(Some(object));
        }
        if !app
            .servicesProvider()
            .is_some_and(|provider| std::ptr::eq(&*provider, object))
        {
            return Err("Application Services provider readback differs".into());
        }
        Ok(Self { delegate, main })
    }
    pub(crate) fn take_events(&self) -> Result<ApplicationEvents, &'static str> {
        // Native pasteboard providers may run a nested event loop. Keep delivery
        // and Quit/reopen intent pending until the one collector has returned.
        if INGRESS.with(|pending| {
            pending
                .try_borrow()
                .map(|pending| pending.collecting)
                .unwrap_or(true)
        }) {
            return Ok(ApplicationEvents {
                quit: false,
                reopen: false,
                selections: VecDeque::new(),
            });
        }
        let app = NSApplication::sharedApplication(self.main);
        let object: &AnyObject = (*self.delegate).as_ref();
        if !app
            .delegate()
            .is_some_and(|current| std::ptr::eq(&*current, &*self.delegate))
            || !app
                .servicesProvider()
                .is_some_and(|current| std::ptr::eq(&*current, object))
        {
            return Err("Application delegate or Services ownership changed");
        }
        let events = EVENTS.swap(0, Ordering::AcqRel);
        if events & BROKEN != 0 {
            return Err(
                "Native application integration failed at the platform boundary; further admission is closed",
            );
        }
        let selections = INGRESS.with(|pending| {
            let mut pending = pending
                .try_borrow_mut()
                .map_err(|_| "Native file delivery is reentrant")?;
            let mut batches = std::mem::take(&mut pending.batches);
            if let Some(error) = pending.refusal.take() {
                batches.push_back(Err(error));
            }
            Ok::<_, &'static str>(batches)
        })?;
        Ok(ApplicationEvents {
            quit: events & QUIT != 0,
            reopen: events & REOPEN != 0,
            selections,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collector_reserves_before_payload_and_refuses_reentrancy_and_full_queue() {
        let mut ingress = PendingIngress::default();
        ingress.begin().unwrap();
        assert!(matches!(
            ingress.begin(),
            Err(NativeError::IngressUnavailable)
        ));
        assert!(ingress.batches.is_empty());
        ingress
            .complete(Ok(vec![PathBuf::from("/owned/first.eml")]))
            .unwrap();
        assert!(!ingress.collecting);
        assert!(matches!(
            ingress.refusal.take(),
            Some(NativeError::IngressUnavailable)
        ));
        while ingress.batches.len() < eml_app::MAX_WAITING_REQUESTS {
            ingress.begin().unwrap();
            ingress
                .complete(Ok(vec![PathBuf::from("/owned/next.eml")]))
                .unwrap();
        }
        assert!(matches!(ingress.begin(), Err(NativeError::SelectionLimit)));
        assert!(!ingress.collecting);
        assert_eq!(ingress.batches.len(), eml_app::MAX_WAITING_REQUESTS);
        assert!(matches!(ingress.refusal, Some(NativeError::SelectionLimit)));
    }
}
