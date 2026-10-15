//! Darwin SDK sys/acl.h opaque ABI. No native structure layout is copied.
use std::{
    ffi::{c_int, c_void},
    io,
    os::fd::AsRawFd,
    ptr::{self, NonNull},
};
unsafe extern "C" {
    fn acl_init(count: c_int) -> *mut c_void;
    fn acl_set_fd(fd: c_int, acl: *mut c_void) -> c_int;
    fn acl_get_fd(fd: c_int) -> *mut c_void;
    fn acl_get_entry(acl: *mut c_void, entry_id: c_int, entry: *mut *mut c_void) -> c_int;
    fn acl_get_tag_type(entry: *mut c_void, tag: *mut c_int) -> c_int;
    fn acl_get_permset_mask_np(entry: *mut c_void, mask: *mut u64) -> c_int;
    fn acl_free(acl: *mut c_void) -> c_int;
}
const MAX_ENTRIES: usize = 128; // ACL_MAX_ENTRIES in the selected Darwin SDK.
const READ: u64 = (1 << 1) | (1 << 3) | (1 << 7) | (1 << 9) | (1 << 11) | (1 << 20);
const KNOWN: u64 = 0x3ffe | (1 << 20); // SDK permission bits1..13 and synchronize20.
struct Acl(Option<NonNull<c_void>>);
impl Acl {
    fn finish<T>(mut self, result: io::Result<T>) -> io::Result<T> {
        let pointer = self
            .0
            .take()
            .ok_or_else(|| io::Error::other("ACL ownership was already released"))?;
        // SAFETY: pointer was returned by libacl and has exactly one owner; this
        // consumes it before the call so Drop cannot release it a second time.
        let released = unsafe { acl_free(pointer.as_ptr()) };
        match result {
            Err(primary) => Err(primary),
            Ok(value) => {
                if released == 0 {
                    Ok(value)
                } else {
                    Err(io::Error::last_os_error())
                }
            }
        }
    }
    fn entries(&self) -> io::Result<Vec<(c_int, u64)>> {
        let pointer = self
            .0
            .ok_or_else(|| io::Error::other("ACL no longer owned"))?;
        let mut selector = 0;
        let mut result = Vec::new();
        loop {
            let mut entry = ptr::null_mut();
            // SAFETY: live ACL allocation, live writable pointer slot. Entry is
            // borrowed from that ACL and never outlives or independently frees it.
            let status = unsafe { acl_get_entry(pointer.as_ptr(), selector, &mut entry) };
            if status != 0 {
                let error = io::Error::last_os_error();
                if status == -1 && error.raw_os_error() == Some(22) {
                    return Ok(result);
                }
                return Err(error);
            }
            if entry.is_null() {
                return Err(io::Error::other("ACL entry success supplied no entry"));
            }
            if result.len() >= MAX_ENTRIES {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "ACL exceeds the native entry bound",
                ));
            }
            let mut tag = 0;
            let mut mask = 0;
            // SAFETY: entry belongs to live ACL; tag/mask have SDK-compatible
            // c_int/u64 alignment/size and stay writable for these calls.
            if unsafe { acl_get_tag_type(entry, &mut tag) } != 0
                || unsafe { acl_get_permset_mask_np(entry, &mut mask) } != 0
            {
                return Err(io::Error::last_os_error());
            }
            result.push((tag, mask));
            selector = -1; // ACL_NEXT_ENTRY, SDK constant.
        }
    }
}
impl Drop for Acl {
    fn drop(&mut self) {
        if let Some(pointer) = self.0.take() {
            // SAFETY: panic/error fallback owns and releases this allocation once.
            let _ = unsafe { acl_free(pointer.as_ptr()) };
        }
    }
}
fn get(fd: &impl AsRawFd) -> io::Result<Option<Acl>> {
    // SAFETY: borrowed live descriptor; successful allocation is immediately owned.
    let pointer = unsafe { acl_get_fd(fd.as_raw_fd()) };
    if let Some(pointer) = NonNull::new(pointer) {
        Ok(Some(Acl(Some(pointer))))
    } else {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(2) {
            Ok(None)
        } else {
            Err(error)
        }
    }
}
fn allowed(entries: &[(c_int, u64)]) -> io::Result<()> {
    for (tag, mask) in entries {
        if ![1, 2].contains(tag) || mask & !KNOWN != 0 || (*tag == 1 && mask & !READ != 0) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Choose a destination without unproven ACL write/delete/ownership grants",
            ));
        }
    }
    Ok(())
}
pub(crate) fn admit_namespace(fd: &impl AsRawFd) -> io::Result<()> {
    let Some(acl) = get(fd)? else {
        return Ok(());
    };
    let result = acl.entries().and_then(|entries| allowed(&entries));
    acl.finish(result)
}
/// Clear only a freshly created owned object; caller must never pass an existing source/destination.
pub(crate) fn clear_owned(fd: &impl AsRawFd) -> io::Result<()> {
    // SAFETY: creates an SDK-owned empty ACL, wrapped before any subsequent call.
    let pointer = NonNull::new(unsafe { acl_init(0) }).ok_or_else(io::Error::last_os_error)?;
    let empty = Acl(Some(pointer));
    // SAFETY: live borrowed descriptor and live empty ACL; no pathname lookup.
    let result = if unsafe { acl_set_fd(fd.as_raw_fd(), pointer.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    };
    empty.finish(result)?;
    if let Some(acl) = get(fd)? {
        let result = acl.entries().and_then(|entries| {
            if entries.is_empty() {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Fresh object retains extended ACL entries",
                ))
            }
        });
        acl.finish(result)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_and_namespace_grants_are_not_accepted_as_read_only() {
        for entries in [
            vec![(1, 1 << 6)],
            vec![(1, 1 << 12)],
            vec![(1, 1 << 13)],
            vec![(3, READ)],
            vec![(2, 1 << 63)],
        ] {
            assert!(allowed(&entries).is_err());
        }
        assert!(allowed(&[(1, READ), (2, KNOWN)]).is_ok());
    }
}
