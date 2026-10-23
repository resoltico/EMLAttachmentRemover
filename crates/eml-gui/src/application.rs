//! Necessary application lifecycle, file-open batches and Services; preserves Winit's delegate.
use crate::native::{self, NativeError};
use objc2::rc::Retained;
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSPasteboard};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString, ns_string};
use slint::winit_030::winit::platform::macos::ApplicationEvent;
use std::{
    cell::RefCell,
    collections::VecDeque,
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};
const QUIT: u8 = 1;
const REOPEN: u8 = 2;
const BROKEN: u8 = 4;
static EVENTS: AtomicU8 = AtomicU8::new(0);
static WAKE_READY: AtomicBool = AtomicBool::new(false);
fn drain_control(events: &AtomicU8, collecting: bool) -> u8 {
    if collecting {
        events.fetch_and(QUIT | REOPEN, Ordering::AcqRel) & BROKEN
    } else {
        events.swap(0, Ordering::AcqRel)
    }
}
fn wake_event_loop() {
    // Delegate creation precedes Slint platform installation. Startup intents
    // remain queued until the pump is installed; missing startup wake is normal.
    if !WAKE_READY.load(Ordering::Acquire) {
        return;
    }
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
    closed: bool,
    collecting: bool,
    refusal: Option<NativeError>,
}
impl PendingIngress {
    fn begin(&mut self) -> Result<(), NativeError> {
        let refusal = if self.closed || self.collecting {
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

/// The only conversion between the fork's and application's objc2 versions.
/// No native pointer or URL borrow escapes this synchronous callback.
fn paths_from_borrowed(
    urls: &slint::winit_030::winit::platform::macos::BorrowedFileUrls<'_>,
) -> Result<Vec<PathBuf>, NativeError> {
    // SAFETY: the fork retains the NSArray for the callback lifetime on the main
    // thread. paths_from_objects checks each actual NSURL type before conversion.
    let objects = unsafe { &*urls.as_ptr().cast::<NSArray<objc2::runtime::AnyObject>>() };
    native::paths_from_objects(objects)
}
pub(crate) fn receive_application(event: ApplicationEvent<'_>) {
    match event {
        ApplicationEvent::QuitRequested => {
            EVENTS.fetch_or(QUIT, Ordering::Release);
        }
        ApplicationEvent::ReopenRequested => {
            EVENTS.fetch_or(REOPEN, Ordering::Release);
        }
        ApplicationEvent::OpenUrls(urls) => {
            let _ = deliver(|| paths_from_borrowed(&urls));
        }
        ApplicationEvent::CallbackPanicked => {
            EVENTS.fetch_or(BROKEN, Ordering::Release);
        }
    }
    wake_event_loop();
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct ServicesProvider;
    unsafe impl NSObjectProtocol for ServicesProvider {}
    impl ServicesProvider {
        #[unsafe(method(createEMLCopies:userData:error:))]
        fn create_copies(&self, board: Option<&NSPasteboard>, _user_data: Option<&NSString>, error: *mut *const NSString) {
            let accepted = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
                deliver(|| native::paths_from_pasteboard(board.ok_or(NativeError::LocalFilesRequired)?))
            )) {
                Ok(result) => result.is_ok(),
                Err(_) => { EVENTS.fetch_or(BROKEN, Ordering::Release); false }
            };
            if !accepted && !error.is_null() {
                // SAFETY: AppKit's optional error slot is writable for this call;
                // the immutable NSString constant has process lifetime.
                unsafe { error.write(ns_string!("Choose only local file URLs within the supported selection limits. Open EML Attachment Remover to inspect the refusal.")); }
            }
        }
    }
);
pub(crate) struct ApplicationHooks {
    provider: Retained<ServicesProvider>,
    main: MainThreadMarker,
}
pub(crate) struct ApplicationEvents {
    pub(crate) quit: bool,
    pub(crate) reopen: bool,
    pub(crate) selections: VecDeque<Result<Vec<PathBuf>, NativeError>>,
    pub(crate) diagnostics: Vec<&'static str>,
}
impl ApplicationHooks {
    pub(crate) fn install() -> Result<Self, Box<dyn std::error::Error>> {
        let main = MainThreadMarker::new().ok_or("Application hooks need the main thread")?;
        let app = NSApplication::sharedApplication(main);
        if app.servicesProvider().is_some() {
            return Err("Application Services provider is already occupied".into());
        }
        let allocated = ServicesProvider::alloc(main).set_ivars(());
        // SAFETY: initialize the allocated main-thread NSObject subclass.
        let provider: Retained<ServicesProvider> = unsafe { msg_send![super(allocated), init] };
        // SAFETY: this retained, declared NSObject subclass implements the Services
        // selector on the main thread and outlives its application registration.
        unsafe {
            app.setServicesProvider(Some(provider.as_super()));
        }
        WAKE_READY.store(true, Ordering::Release);
        Ok(Self { provider, main })
    }
    pub(crate) fn take_events(&self) -> ApplicationEvents {
        let mut diagnostics = Vec::new();
        let app = NSApplication::sharedApplication(self.main);
        if !app
            .servicesProvider()
            .is_some_and(|current| std::ptr::eq(&*current, self.provider.as_super().as_ref()))
        {
            diagnostics
                .push("Application Services ownership changed; further native ingress is closed");
            EVENTS.fetch_or(BROKEN, Ordering::Release);
        }
        // A native collector may run a nested event loop. Preserve controls until
        // its reservation unwinds, then drain controls independently of diagnostics.
        let collecting = INGRESS.with(|pending| {
            pending
                .try_borrow()
                .map_or(true, |pending| pending.collecting)
        });
        let events = drain_control(&EVENTS, collecting);
        if events & BROKEN != 0 {
            diagnostics
                .push("Native application integration failed; further native ingress is closed");
            INGRESS.with(|pending| {
                if let Ok(mut pending) = pending.try_borrow_mut() {
                    pending.closed = true;
                }
            });
        }
        let selections = INGRESS.with(|pending| {
            let Ok(mut pending) = pending.try_borrow_mut() else {
                return VecDeque::new();
            };
            if pending.collecting {
                return VecDeque::new();
            }
            let mut batches = std::mem::take(&mut pending.batches);
            if let Some(error) = pending.refusal.take() {
                batches.push_back(Err(error));
            }
            batches
        });
        ApplicationEvents {
            quit: events & QUIT != 0,
            reopen: events & REOPEN != 0,
            selections,
            diagnostics,
        }
    }
}
impl Drop for ApplicationHooks {
    fn drop(&mut self) {
        WAKE_READY.store(false, Ordering::Release);
        let app = NSApplication::sharedApplication(self.main);
        if app
            .servicesProvider()
            .is_some_and(|current| std::ptr::eq(&*current, self.provider.as_super().as_ref()))
        {
            // SAFETY: main-thread teardown removes only our own still-retained
            // provider after verifying its exact Objective-C object identity.
            unsafe {
                app.setServicesProvider(None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_survive_reentrant_collection_and_drain_with_failure() {
        let events = AtomicU8::new(QUIT | REOPEN | BROKEN);
        assert_eq!(drain_control(&events, true), BROKEN);
        assert_eq!(events.load(Ordering::Acquire), QUIT | REOPEN);
        events.fetch_or(BROKEN, Ordering::Release);
        assert_eq!(drain_control(&events, false), QUIT | REOPEN | BROKEN);
        assert_eq!(drain_control(&events, false), 0);
    }
    #[test]
    fn failed_ingress_closes_only_native_collection() {
        let mut ingress = PendingIngress {
            closed: true,
            ..PendingIngress::default()
        };
        assert!(matches!(
            ingress.begin(),
            Err(NativeError::IngressUnavailable)
        ));
        assert!(!ingress.collecting);
        assert!(ingress.batches.is_empty());
    }
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
