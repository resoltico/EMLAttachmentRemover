//! Owned parented system dialogs and lossless local-file URL transport only.
use block2::RcBlock;
use objc2::{
    MainThreadMarker, Message,
    rc::{Retained, Weak},
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
};

#[derive(Clone, Copy, Debug)]
pub(crate) enum NativeError {
    MainThreadRequired,
    ParentUnavailable,
    LocalFilesRequired,
    SelectionLimit,
    InvalidNativePath,
    DialogAborted,
}
impl std::fmt::Display for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MainThreadRequired => "A native dialog needs the main application thread",
            Self::ParentUnavailable => {
                "The parent window is unavailable; open the main window and try again"
            }
            Self::LocalFilesRequired => {
                "Choose only local file URLs without host, credentials, query or fragment"
            }
            Self::SelectionLimit => "The native selection exceeds the supported path/count limits",
            Self::InvalidNativePath => "The native file URL cannot be represented losslessly",
            Self::DialogAborted => "The native file dialog could not complete",
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
    if encoded.len() > 3 * eml_core::MAX_PATH_UNITS + 16 {
        return Err(NativeError::SelectionLimit);
    }
    // SAFETY: NSURL supplies retained immutable NSData, not exposed or mutated
    // while these bounded encoded bytes are inspected.
    let encoded = unsafe { encoded.as_bytes_unchecked() };
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
    let mut buffer = vec![0u8; eml_core::MAX_PATH_UNITS + 1];
    // SAFETY: writable owned buffer includes space for the required terminator.
    // Check BOOL and original URL NUL separately; never scan an unbounded pointer.
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
    buffer.truncate(length);
    Ok(std::ffi::OsString::from_vec(buffer).into())
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
fn paths(urls: &NSArray<NSURL>) -> Result<Vec<PathBuf>, NativeError> {
    if urls.is_empty() || urls.len() > eml_core::MAX_INPUT_PATHS {
        return Err(NativeError::SelectionLimit);
    }
    let mut result = Vec::with_capacity(urls.len());
    let mut bytes = 0usize;
    for url in urls {
        let path = path_from_url(&url)?;
        bytes = bytes
            .checked_add(path.as_os_str().as_bytes().len())
            .filter(|bytes| *bytes <= eml_core::MAX_INPUT_PATH_BYTES)
            .ok_or(NativeError::SelectionLimit)?;
        result.push(path);
    }
    Ok(result)
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
pub(crate) fn restore(window: &slint::Window) -> Result<(), NativeError> {
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
        let panel = NSOpenPanel::openPanel(main);
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
        panel.beginSheetModalForWindow_completionHandler(&parent, &block);
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
