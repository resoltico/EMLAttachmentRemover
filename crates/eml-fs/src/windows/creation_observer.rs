//! Nonshipping observers for deterministic real-native creation interleavings.
use std::{cell::RefCell, ffi::OsStr, fs::File, io, marker::PhantomData, rc::Rc};
/// Exact operation boundary observed without substituting its native result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreationBoundary {
    /// All validation/path capture finished; the next operation creates a file.
    BeforeFileCreate,
    /// CREATE_NEW returned its exact owned file, before containment or cleanup.
    FileOpened,
    /// The parent guard is held; the next operation is CreateDirectoryW.
    BeforeMirrorCreate,
    /// Boolean creation succeeded; no directory handle has yet been obtained.
    MirrorCreated,
    /// A directory was opened after creation, without ownership authority.
    MirrorOpened,
    /// Before the actual file-type check and message write.
    BeforeWrite,
    /// Actual write completed, before later cleanup/rename can erase effects.
    AfterWrite,
}
/// Borrowed actual native objects; none may escape the callback.
pub struct CreationObservation<'a> {
    /// Operation boundary.
    pub boundary: CreationBoundary,
    /// Exact original parent file object, not a repeated creation-path lookup.
    pub parent: &'a File,
    /// Native single component.
    pub name: &'a OsStr,
    /// Exact returned file object where the operation has reached it.
    pub object: Option<&'a File>,
}
type Observer = Box<dyn for<'a> FnMut(CreationObservation<'a>)>;
thread_local! { static OBSERVER: RefCell<Option<Observer>> = RefCell::new(None); }
/// Thread-bound observer lease; shipping builds never enable this module.
pub struct CreationObserver {
    _same_thread: PhantomData<Rc<()>>,
}
impl Drop for CreationObserver {
    fn drop(&mut self) {
        OBSERVER.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}
/// Install one observer on this test thread; nesting is explicitly refused.
pub fn observe_creations(
    observer: impl for<'a> FnMut(CreationObservation<'a>) + 'static,
) -> io::Result<CreationObserver> {
    OBSERVER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            return Err(io::Error::other("A creation observer is already installed"));
        }
        *slot = Some(Box::new(observer));
        Ok(CreationObserver {
            _same_thread: PhantomData,
        })
    })
}
pub(super) fn notify(
    boundary: CreationBoundary,
    parent: &File,
    name: &OsStr,
    object: Option<&File>,
) {
    OBSERVER.with(|slot| {
        if let Some(observer) = &mut *slot.borrow_mut() {
            observer(CreationObservation {
                boundary,
                parent,
                name,
                object,
            });
        }
    });
}

/// Privilege/group-limited test token retaining the runner's SAME UserSID.
/// This is not a different-user ACL test.
pub struct RestrictedToken {
    _scope: super::test_actor::RestrictedToken,
    _same_thread: PhantomData<Rc<()>>,
}
impl RestrictedToken {
    /// Enter the existing checked ordinary-user capability control.
    pub fn enter() -> io::Result<Self> {
        Ok(Self {
            _scope: super::test_actor::RestrictedToken::enter()?,
            _same_thread: PhantomData,
        })
    }
}
/// Independently query the complete native ID from the observed file object.
pub fn full_identity(file: &File) -> io::Result<crate::Identity> {
    use std::{mem::size_of, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };
    let mut info = FILE_ID_INFO::default();
    // SAFETY: live borrowed handle and aligned generated exact-size output.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(crate::Identity::Windows {
        volume: info.VolumeSerialNumber,
        file: info.FileId.Identifier,
    })
}
/// Convert only an owned fixture directory, using an attribute-only handle.
/// The returned mutation handle stays bound to that same object for ABA restoration.
pub fn make_junction(path: &std::path::Path, target: &std::path::Path) -> io::Result<File> {
    let file = super::test_reparse::writer_with_access(
        path,
        windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES,
    )?;
    super::test_reparse::set_mount_point(&file, target)?;
    Ok(file)
}
/// Restore the exact owned mount-point object without looking up a changed path.
pub fn restore_directory(file: &File) -> io::Result<()> {
    use std::{mem::offset_of, os::windows::io::AsRawHandle};
    use windows_sys::{
        Wdk::Storage::FileSystem::REPARSE_DATA_BUFFER,
        Win32::System::{
            IO::DeviceIoControl, Ioctl::FSCTL_DELETE_REPARSE_POINT,
            SystemServices::IO_REPARSE_TAG_MOUNT_POINT,
        },
    };
    let buffer = REPARSE_DATA_BUFFER {
        ReparseTag: IO_REPARSE_TAG_MOUNT_POINT,
        ..Default::default()
    };
    let mut returned = 0;
    // SAFETY: generated tag-only header, zero data length, exact attribute-only
    // handle retained from the owned conversion, synchronous operation.
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            FSCTL_DELETE_REPARSE_POINT,
            (&buffer as *const REPARSE_DATA_BUFFER).cast(),
            offset_of!(REPARSE_DATA_BUFFER, Anonymous) as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Apply a non-Microsoft tag to an owned empty file through attribute-only access.
/// Success is required before a test may claim that the product rejected a tag.
pub fn tag_empty_file(path: &std::path::Path) -> io::Result<()> {
    use std::{mem::offset_of, os::windows::io::AsRawHandle};
    use windows_sys::Win32::{
        Storage::FileSystem::{FILE_WRITE_ATTRIBUTES, REPARSE_GUID_DATA_BUFFER},
        System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_REPARSE_POINT},
    };
    let file = super::test_reparse::writer_with_access(path, FILE_WRITE_ATTRIBUTES)?;
    if file.metadata()?.len() != 0 {
        return Err(io::Error::other(
            "The fixture tag must precede every message byte",
        ));
    }
    let buffer = REPARSE_GUID_DATA_BUFFER {
        ReparseTag: 0x42,
        ReparseGuid: windows_sys::core::GUID::from_u128(0x624494b4_2d39_45ba_887a_57e089726bf2),
        ..Default::default()
    };
    let mut returned = 0;
    // SAFETY: generated aligned tag/GUID header, zero data length, live
    // attribute-only fixture handle, synchronous operation with no output data.
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            FSCTL_SET_REPARSE_POINT,
            (&buffer as *const REPARSE_GUID_DATA_BUFFER).cast(),
            offset_of!(REPARSE_GUID_DATA_BUFFER, GenericReparseBuffer) as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    super::handle::close(file)
}
