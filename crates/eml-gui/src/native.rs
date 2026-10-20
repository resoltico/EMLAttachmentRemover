//! Parented system dialogs, responder editing and lossless local-file URL transport.
use block2::RcBlock;
use objc2::{
    MainThreadMarker, Message,
    rc::{Retained, Weak},
    sel,
};
use objc2_app_kit::{
    NSModalResponse, NSModalResponseCancel, NSModalResponseOK, NSOpenPanel, NSSavePanel, NSView,
    NSWindow,
};
use objc2_foundation::{NSArray, NSString, NSURL};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::winit_030::WinitWindowAccessor;
use std::{
    cell::Cell,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr::NonNull,
    rc::Rc,
    sync::{Arc, OnceLock},
};

unsafe extern "C" {
    fn NSLog(format: &NSString, ...);
}

/// Records lifecycle boundaries without selections, paths or preference values.
pub(crate) fn lifecycle_observation(event: &'static str) {
    // SAFETY: Foundation's variadic format has exactly one Objective-C object
    // argument. The format and retained message remain alive throughout the call.
    unsafe {
        NSLog(
            objc2_foundation::ns_string!("EML lifecycle: %@"),
            &*NSString::from_str(event),
        );
    }
}

/// Persists only the source location; panic payloads may contain user input.
pub(crate) fn panic_observation(info: &std::panic::PanicHookInfo<'_>) {
    if let Some(location) = info.location() {
        // SAFETY: the fixed format receives one live NSString and two C unsigned
        // integers. Foundation logging is safe from background threads as well.
        unsafe {
            NSLog(
                objc2_foundation::ns_string!("EML panic source: %@:%u:%u"),
                &*NSString::from_str(location.file()),
                location.line(),
                location.column(),
            );
        }
    }
}

pub(crate) enum EditAction {
    Copy,
    SelectAll,
}
pub(crate) fn edit(action: EditAction) -> Result<(), NativeError> {
    let main = MainThreadMarker::new().ok_or(NativeError::MainThreadRequired)?;
    let selector = match action {
        EditAction::Copy => sel!(copy:),
        EditAction::SelectAll => sel!(selectAll:),
    };
    // SAFETY: these fixed AppKit standard actions take one nullable sender.
    // Nil target asks AppKit to resolve the current native first responder.
    unsafe {
        objc2_app_kit::NSApplication::sharedApplication(main)
            .sendAction_to_from(selector, None, None);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum NativeError {
    MainThreadRequired,
    ParentUnavailable,
    LocalFilesRequired,
    SelectionLimit,
    InvalidNativePath,
    DialogAborted,
    IngressUnavailable,
    EventLoopUnavailable,
}
impl std::fmt::Display for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MainThreadRequired => "A native operation needs the main application thread",
            Self::ParentUnavailable => {
                "The parent window is unavailable; open the main window and try again"
            }
            Self::LocalFilesRequired => {
                "Choose only local file URLs without host, credentials, query or fragment"
            }
            Self::SelectionLimit => "The native selection exceeds the supported path/count limits",
            Self::InvalidNativePath => "The native file URL cannot be represented losslessly",
            Self::DialogAborted => "The native file dialog could not complete",
            Self::IngressUnavailable => {
                "Native file delivery could not be retained. Retry from the main window."
            }
            Self::EventLoopUnavailable => {
                "The application event loop cannot accept Finder navigation"
            }
        })
    }
}
impl std::error::Error for NativeError {}
pub(crate) fn path_from_url(url: &NSURL) -> Result<PathBuf, NativeError> {
    if !url.isFileURL()
        || url.host().is_some_and(|host| host.length() != 0)
        || url.user().is_some()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(NativeError::LocalFilesRequired);
    }
    // Native-byte NSURL values can have no NSString path. Inspect a bounded
    // encoded representation for forbidden authority/NUL, without using it to
    // construct an operation path; only the checked native buffer below does so.
    let encoded = url.dataRepresentation();
    check_url_data(&encoded)?;
    let mut buffer = [0u8; eml_core::MAX_PATH_UNITS + 1];
    // SAFETY: writable owned buffer includes space for the required terminator.
    // Check the native Boolean and original URL NUL separately.
    if !unsafe {
        url.getFileSystemRepresentation_maxLength(
            NonNull::new(buffer.as_mut_ptr().cast()).expect("nonempty buffer"),
            buffer.len(),
        )
    } {
        return Err(NativeError::InvalidNativePath);
    }
    let length = buffer
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(NativeError::InvalidNativePath)?;
    if length == 0 || length > eml_core::MAX_PATH_UNITS || buffer[0] != b'/' {
        return Err(NativeError::InvalidNativePath);
    }
    // Retain only the supplied native bytes, not one maximum-sized allocation
    // for every short name in a multi-file selection.
    Ok(std::ffi::OsString::from_vec(buffer[..length].to_vec()).into())
}
fn url_from_path(path: &Path) -> Result<Retained<NSURL>, NativeError> {
    let raw = path.as_os_str().as_bytes();
    if !path.is_absolute() || raw.len() > eml_core::MAX_PATH_UNITS || raw.contains(&0) {
        return Err(NativeError::InvalidNativePath);
    }
    let mut bytes = raw.to_vec();
    bytes.push(0);
    // SAFETY: absolute native bytes have no interior NUL and remain alive for
    // the constructor, which copies them. No display String becomes authority.
    Ok(unsafe {
        NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
            NonNull::new(bytes.as_mut_ptr().cast()).expect("terminated path"),
            false,
            None,
        )
    })
}
pub(crate) struct NativeReveal {
    completed: Arc<OnceLock<Result<(), NativeError>>>,
}
impl NativeReveal {
    pub(crate) fn result(&self) -> Option<Result<(), NativeError>> {
        self.completed.get().copied()
    }
}
pub(crate) fn reveal(path: PathBuf) -> Result<NativeReveal, NativeError> {
    MainThreadMarker::new().ok_or(NativeError::MainThreadRequired)?;
    let completed = Arc::new(OnceLock::new());
    let observation = Arc::clone(&completed);
    // NSWorkspace can pump native events. Run outside the Slint timer callback,
    // retaining completion until the native call has actually returned. Nested
    // events can observe the reservation without borrowing the controller here.
    slint::invoke_from_event_loop(move || {
        let _ = observation.set(reveal_now(&path));
    })
    .map_err(|_| NativeError::EventLoopUnavailable)?;
    Ok(NativeReveal { completed })
}
fn reveal_now(path: &Path) -> Result<(), NativeError> {
    MainThreadMarker::new().ok_or(NativeError::MainThreadRequired)?;
    let url = url_from_path(path)?;
    lifecycle_observation("Finder selection dispatch entered");
    objc2_app_kit::NSWorkspace::sharedWorkspace()
        .activateFileViewerSelectingURLs(&NSArray::from_slice(&[&*url]));
    lifecycle_observation("Finder selection dispatch returned");
    Ok(())
}
fn collect_paths(
    count: usize,
    paths: impl IntoIterator<Item = Result<PathBuf, NativeError>>,
) -> Result<Vec<PathBuf>, NativeError> {
    if count == 0 || count > eml_core::MAX_INPUT_PATHS {
        return Err(NativeError::SelectionLimit);
    }
    let mut result = Vec::with_capacity(count);
    let mut bytes = 0usize;
    for path in paths {
        let path = path?;
        bytes = bytes
            .checked_add(path.as_os_str().as_bytes().len())
            .filter(|bytes| *bytes <= eml_core::MAX_INPUT_PATH_BYTES)
            .ok_or(NativeError::SelectionLimit)?;
        result.push(path);
    }
    Ok(result)
}
fn paths(urls: &NSArray<NSURL>) -> Result<Vec<PathBuf>, NativeError> {
    collect_paths(urls.len(), urls.into_iter().map(|url| path_from_url(&url)))
}
pub(crate) fn paths_from_objects(
    urls: &NSArray<objc2::runtime::AnyObject>,
) -> Result<Vec<PathBuf>, NativeError> {
    collect_paths(
        urls.len(),
        urls.into_iter().map(|object| {
            let url = object
                .downcast::<NSURL>()
                .map_err(|_| NativeError::LocalFilesRequired)?;
            path_from_url(&url)
        }),
    )
}
fn check_url_data(data: &objc2_foundation::NSData) -> Result<(), NativeError> {
    if data.len() > 3 * eml_core::MAX_PATH_UNITS + 16 {
        return Err(NativeError::SelectionLimit);
    }
    // SAFETY: bounded immutable Foundation data, retained by the caller during
    // this inspection. No operation path is constructed from these bytes.
    let encoded = unsafe { data.as_bytes_unchecked() };
    let Some(scheme) = encoded.get(..5) else {
        return Err(NativeError::LocalFilesRequired);
    };
    let rest = &encoded[5..];
    if !scheme.eq_ignore_ascii_case(b"file:")
        || !rest.starts_with(b"/")
        || (rest.starts_with(b"//") && !rest.starts_with(b"///"))
        || encoded.iter().any(|byte| [0, b'?', b'#'].contains(byte))
        || encoded.windows(3).any(|bytes| bytes == b"%00")
    {
        return Err(NativeError::LocalFilesRequired);
    }
    Ok(())
}
pub(crate) fn paths_from_pasteboard(
    board: &objc2_app_kit::NSPasteboard,
) -> Result<Vec<PathBuf>, NativeError> {
    use objc2::ClassType;
    use objc2_foundation::{NSDictionary, NSNumber};
    let before = board.changeCount();
    let items = board
        .pasteboardItems()
        .ok_or(NativeError::LocalFilesRequired)?;
    if items.is_empty() || items.len() > eml_core::MAX_INPUT_PATHS {
        return Err(NativeError::SelectionLimit);
    }
    // Percent encoding needs at most three bytes per native byte; account for
    // each file-URL prefix separately. Refuse the whole representation before
    // asking the native reader to materialize every URL object.
    let encoded_budget = 3 * eml_core::MAX_INPUT_PATH_BYTES + 16 * items.len();
    let mut encoded_bytes = 0usize;
    for item in &*items {
        // SAFETY: immutable system file-URL type identifier. Check every item's
        // original representation before the native reader can filter a batch.
        let data = item
            .dataForType(unsafe { objc2_app_kit::NSPasteboardTypeFileURL })
            .ok_or(NativeError::LocalFilesRequired)?;
        check_url_data(&data)?;
        encoded_bytes = encoded_bytes
            .checked_add(data.len())
            .filter(|total| *total <= encoded_budget)
            .ok_or(NativeError::SelectionLimit)?;
    }
    // SAFETY: immutable system reading-option key; NSURL implements
    // NSPasteboardReading, and this option requires file URL objects only.
    let key = unsafe { objc2_app_kit::NSPasteboardURLReadingFileURLsOnlyKey };
    let yes = NSNumber::new_bool(true);
    let value: &objc2::runtime::AnyObject = &yes;
    let options = NSDictionary::from_slices(&[key], &[value]);
    let objects = unsafe {
        board.readObjectsForClasses_options(&NSArray::from_slice(&[NSURL::class()]), Some(&options))
    }
    .ok_or(NativeError::LocalFilesRequired)?;
    if objects.len() != items.len() {
        return Err(NativeError::LocalFilesRequired);
    }
    let paths = paths_from_objects(&objects)?;
    if board.changeCount() != before {
        return Err(NativeError::IngressUnavailable);
    }
    Ok(paths)
}
fn parent(window: &slint::Window) -> Result<Retained<NSWindow>, NativeError> {
    let _main = MainThreadMarker::new().ok_or(NativeError::MainThreadRequired)?;
    window
        .with_winit_window(|window| {
            let handle = window
                .window_handle()
                .map_err(|_| NativeError::ParentUnavailable)?;
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                return Err(NativeError::ParentUnavailable);
            };
            // SAFETY: raw-window-handle provides a live NSView while the borrowed
            // Winit handle is valid; retain the native view, then its current window.
            // No Winit Arc or borrowed raw pointer escapes this callback.
            let view = unsafe { (&*handle.ns_view.as_ptr().cast::<NSView>()).retain() };
            view.window().ok_or(NativeError::ParentUnavailable)
        })
        .ok_or(NativeError::ParentUnavailable)?
}
pub(crate) fn restore(window: &slint::Window) -> Result<(), Box<dyn std::error::Error>> {
    window.show()?;
    let parent = parent(window)?;
    if parent.isMiniaturized() {
        parent.deminiaturize(None);
    }
    parent.makeKeyAndOrderFront(None);
    Ok(())
}
/// Read system appearance only; never change owner appearance preferences.
pub(crate) fn appearance() -> Result<(bool, bool), NativeError> {
    use objc2_app_kit::{
        NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSWorkspace,
    };
    let main = MainThreadMarker::new().ok_or(NativeError::MainThreadRequired)?;
    // SAFETY: immutable Foundation appearance-name constants.
    let (light, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
    let appearance = NSApplication::sharedApplication(main).effectiveAppearance();
    let matched =
        appearance.bestMatchFromAppearancesWithNames(&NSArray::from_slice(&[light, dark]));
    Ok((
        matched.is_some_and(|name| &*name == dark),
        NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast(),
    ))
}
pub(crate) enum OpenKind {
    Files,
    Folders,
    Destination,
}
pub(crate) enum DialogResult {
    Chosen(Vec<PathBuf>),
    Cancelled,
    Failed(NativeError),
}
enum Panel {
    Open(Retained<NSOpenPanel>),
    Save(Retained<NSSavePanel>),
}
pub(crate) struct NativeDialog {
    panel: Panel,
    parent: Retained<NSWindow>,
    result: Rc<Cell<Option<DialogResult>>>,
    _block: RcBlock<dyn Fn(NSModalResponse)>,
}
impl NativeDialog {
    pub(crate) fn open(
        window: &slint::Window,
        kind: OpenKind,
        initial: Option<&Path>,
    ) -> Result<Self, NativeError> {
        let main = MainThreadMarker::new().ok_or(NativeError::MainThreadRequired)?;
        let parent = parent(window)?;
        lifecycle_observation("Open panel constructor entered");
        let panel = NSOpenPanel::openPanel(main);
        lifecycle_observation("Open panel constructor returned");
        panel.setCanChooseFiles(matches!(kind, OpenKind::Files));
        panel.setCanChooseDirectories(!matches!(kind, OpenKind::Files));
        panel.setAllowsMultipleSelection(!matches!(kind, OpenKind::Destination));
        panel.setCanCreateDirectories(false);
        if let Some(path) = initial {
            let url = url_from_path(path)?;
            panel.setDirectoryURL(Some(&url));
        }
        panel.setMessage(Some(&NSString::from_str(match kind {
            OpenKind::Files => "Choose EML messages",
            OpenKind::Folders => "Choose folders containing EML messages",
            OpenKind::Destination => "Choose an existing folder for copies",
        })));
        let result = Rc::new(Cell::new(None));
        let completed = Rc::clone(&result);
        let weak = Weak::new(&*panel);
        let called = Cell::new(false);
        let block = RcBlock::new(move |response: NSModalResponse| {
            if called.replace(true) {
                return;
            }
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let Some(panel) = weak.load() else {
                    return DialogResult::Failed(NativeError::DialogAborted);
                };
                let value = if response == NSModalResponseCancel {
                    DialogResult::Cancelled
                } else if response == NSModalResponseOK {
                    match paths(&panel.URLs()) {
                        Ok(paths) => DialogResult::Chosen(paths),
                        Err(error) => DialogResult::Failed(error),
                    }
                } else {
                    DialogResult::Failed(NativeError::DialogAborted)
                };
                // SAFETY: nil sender is valid; sheet completion can precede its
                // disappearance, so order it out before publishing completion.
                panel.orderOut(None);
                value
            }))
            .unwrap_or(DialogResult::Failed(NativeError::DialogAborted));
            completed.set(Some(outcome));
        });
        lifecycle_observation("Open panel presentation entered");
        panel.beginSheetModalForWindow_completionHandler(&parent, &block);
        lifecycle_observation("Open panel presentation returned");
        Ok(Self {
            panel: Panel::Open(panel),
            parent,
            result,
            _block: block,
        })
    }
    pub(crate) fn save(window: &slint::Window) -> Result<Self, NativeError> {
        let main = MainThreadMarker::new().ok_or(NativeError::MainThreadRequired)?;
        let parent = parent(window)?;
        let panel = NSSavePanel::savePanel(main);
        panel.setCanCreateDirectories(false);
        panel.setNameFieldStringValue(&NSString::from_str("EML-report.json"));
        panel.setMessage(Some(&NSString::from_str(
            "Export metadata only. Existing files will not be replaced.",
        )));
        let result = Rc::new(Cell::new(None));
        let completed = Rc::clone(&result);
        let weak = Weak::new(&*panel);
        let called = Cell::new(false);
        let block = RcBlock::new(move |response: NSModalResponse| {
            if called.replace(true) {
                return;
            }
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let Some(panel) = weak.load() else {
                    return DialogResult::Failed(NativeError::DialogAborted);
                };
                let value = if response == NSModalResponseCancel {
                    DialogResult::Cancelled
                } else if response == NSModalResponseOK {
                    match panel
                        .URL()
                        .ok_or(NativeError::InvalidNativePath)
                        .and_then(|url| path_from_url(&url))
                    {
                        Ok(path) => DialogResult::Chosen(vec![path]),
                        Err(error) => DialogResult::Failed(error),
                    }
                } else {
                    DialogResult::Failed(NativeError::DialogAborted)
                };
                // SAFETY: nil is a valid sender for this owned panel.
                panel.orderOut(None);
                value
            }))
            .unwrap_or(DialogResult::Failed(NativeError::DialogAborted));
            completed.set(Some(outcome));
        });
        panel.beginSheetModalForWindow_completionHandler(&parent, &block);
        Ok(Self {
            panel: Panel::Save(panel),
            parent,
            result,
            _block: block,
        })
    }
    pub(crate) fn take_result(&self, window: &slint::Window) -> Option<DialogResult> {
        // Release the Rust borrow before querying/reentering native window APIs.
        let result = self.result.take()?;
        if parent(window).is_ok_and(|current| std::ptr::eq(&*current, &*self.parent)) {
            Some(result)
        } else {
            Some(DialogResult::Failed(NativeError::ParentUnavailable))
        }
    }
    pub(crate) fn cancel(&self) {
        // SAFETY: cancel only this owned system panel; nil sender is valid.
        unsafe {
            match &self.panel {
                Panel::Open(panel) => panel.cancel(None),
                Panel::Save(panel) => panel.cancel(None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_native_pasteboard_keeps_file_objects_and_refuses_filtered_mixed_batches() {
        use objc2::runtime::ProtocolObject;
        use objc2_app_kit::{NSPasteboard, NSPasteboardWriting};
        struct OwnedBoard(Retained<NSPasteboard>);
        impl Drop for OwnedBoard {
            fn drop(&mut self) {
                self.0.clearContents();
            }
        }
        let board = OwnedBoard(NSPasteboard::pasteboardWithUniqueName());
        let path = Path::new("/owned-synthetic/a?b#c%\n\u{1f600}.eml");
        let url = url_from_path(path).unwrap();
        let writer: &ProtocolObject<dyn NSPasteboardWriting> = ProtocolObject::from_ref(&*url);
        assert!(board.0.writeObjects(&NSArray::from_slice(&[writer])));
        assert_eq!(paths_from_pasteboard(&board.0).unwrap(), [path]);
        board.0.clearContents();
        let text = NSString::from_str("not a file URL item");
        let text_writer: &ProtocolObject<dyn NSPasteboardWriting> =
            ProtocolObject::from_ref(&*text);
        assert!(
            board
                .0
                .writeObjects(&NSArray::from_slice(&[writer, text_writer]))
        );
        assert!(
            paths_from_pasteboard(&board.0).is_err(),
            "The native reader must not silently filter out the non-file item"
        );
    }
    #[test]
    fn file_open_objects_reject_a_mixed_batch_without_returning_partial_paths() {
        use objc2::runtime::AnyObject;
        let path_buf = PathBuf::from(std::ffi::OsString::from_vec(
            b"/owned-synthetic/message-\xff.eml".to_vec(),
        ));
        let path = path_buf.as_path();
        let url = url_from_path(path).unwrap();
        let text = NSString::from_str("not a file URL object");
        let url_object: &AnyObject = &url;
        let text_object: &AnyObject = &text;
        assert_eq!(
            paths_from_objects(&NSArray::from_slice(&[url_object])).unwrap(),
            [path]
        );
        assert!(paths_from_objects(&NSArray::from_slice(&[url_object, text_object])).is_err());
        assert!(paths_from_objects(&NSArray::<AnyObject>::new()).is_err());
    }
    #[test]
    fn local_file_urls_preserve_native_bytes_and_reject_ambiguous_authority() {
        let raw = b"/owned-synthetic/a\xff\n.eml".to_vec();
        let path = PathBuf::from(std::ffi::OsString::from_vec(raw.clone()));
        assert_eq!(
            path_from_url(&url_from_path(&path).unwrap())
                .unwrap()
                .as_os_str()
                .as_bytes(),
            raw
        );
        for text in [
            "https://example.invalid/mail.eml",
            "file://remote.invalid/a.eml",
            "file:///a.eml?query",
            "file:///a.eml#fragment",
            "file:///a%00b.eml",
            "file://user@/a.eml",
            "file://user:password@localhost/a.eml",
            "file://localhost:123/a.eml",
        ] {
            let url = NSURL::URLWithString(&NSString::from_str(text)).unwrap();
            assert!(path_from_url(&url).is_err(), "Ambiguous file URL accepted");
        }
        assert!(url_from_path(Path::new("relative.eml")).is_err());
        let literal = Path::new("/owned-synthetic/literal%00.eml");
        assert_eq!(
            path_from_url(&url_from_path(literal).unwrap()).unwrap(),
            literal
        );
    }
    #[test]
    fn native_url_factory_rejects_nul_and_oversized_paths_before_native_construction() {
        let nul = PathBuf::from(std::ffi::OsString::from_vec(
            b"/owned-synthetic/a\0b.eml".to_vec(),
        ));
        assert!(url_from_path(&nul).is_err());
        let oversized = PathBuf::from(std::ffi::OsString::from_vec(
            [b"/".as_slice(), &vec![b'a'; eml_core::MAX_PATH_UNITS]].concat(),
        ));
        assert!(url_from_path(&oversized).is_err());
        for raw in [
            b"/owned-synthetic/\xfe\xff.eml".as_slice(),
            b"/owned-synthetic/a?b#c%\n.eml".as_slice(),
            "/owned-synthetic/\u{fffd}\u{1f600}.eml".as_bytes(),
        ] {
            let path = PathBuf::from(std::ffi::OsString::from_vec(raw.to_vec()));
            assert_eq!(
                path_from_url(&url_from_path(&path).unwrap())
                    .unwrap()
                    .as_os_str()
                    .as_bytes(),
                raw
            );
        }
    }
}
