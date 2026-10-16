//! Fresh owned ACL fixture shared by native write-capability tests only.
use super::test_actor::{information, require_unimpersonated_thread};
use std::{
    io,
    mem::size_of,
    os::windows::io::{FromRawHandle, OwnedHandle},
    path::Path,
};

/// Prepare only a freshly owned test directory so the verified runner UserSID
/// has real creation/flush/cleanup access without depending on checkout ACLs.
/// This is fixture setup, not product ACL policy or a mutation of an owner folder.
fn grant_current_user_directory(path: &std::path::Path) -> io::Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::*, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
            SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        Storage::FileSystem::FILE_ALL_ACCESS,
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    require_unimpersonated_thread()?;
    let mut raw = ptr::null_mut();
    // SAFETY: live process pseudo-handle and writable output slot; query only.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful call transferred one owned process-token handle.
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let info = information(&token, TokenUser)?;
    if info.bytes < size_of::<TOKEN_USER>() {
        return Err(io::Error::other("Short fixture token-user information"));
    }
    // SAFETY: generated native header is present/aligned; its SID points into
    // the successful native token information kept alive through ACL construction.
    let sid = unsafe { (*(info.words.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    if sid.is_null() {
        return Err(io::Error::other("Missing fixture UserSID"));
    }
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: FILE_ALL_ACCESS,
        grfAccessMode: SET_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: sid.cast(),
            ..Default::default()
        },
    };
    let mut acl = ptr::null_mut();
    // SAFETY: one fully initialized generated entry with live SID; the system
    // returns a separately allocated ACL, and no prior ACL pointer is borrowed.
    let built = unsafe { SetEntriesInAclW(1, &entry, ptr::null(), &mut acl) };
    if built != 0 {
        return Err(io::Error::from_raw_os_error(built as i32));
    }
    if acl.is_null() {
        return Err(io::Error::other("Fixture ACL builder supplied no ACL"));
    }
    let mut native: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: terminated fresh owned fixture path; live constructed ACL. Only
    // this synthetic directory's DACL changes, before any restricted actor enters.
    let applied = unsafe {
        SetNamedSecurityInfoW(
            native.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            acl,
            ptr::null_mut(),
        )
    };
    // SAFETY: the successful ACL builder allocated this exact pointer once.
    let released = unsafe { LocalFree(acl.cast()) };
    if applied != 0 {
        return Err(io::Error::from_raw_os_error(applied as i32));
    }
    if !released.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn directory() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    grant_current_user_directory(root.path()).unwrap();
    root
}
pub(super) fn operation(path: &Path) -> std::path::PathBuf {
    crate::PathContext::capture()
        .unwrap()
        .resolve(path)
        .unwrap()
}
