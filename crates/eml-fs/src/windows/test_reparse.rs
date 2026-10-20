//! Real owned-fixture mount-point creation, shared by native reparse controls.
use std::{
    fs::{File, OpenOptions},
    io,
    mem::{offset_of, size_of},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
    ptr,
};
use windows_sys::{
    Wdk::Storage::FileSystem::{REPARSE_DATA_BUFFER, REPARSE_DATA_BUFFER_0_1},
    Win32::{
        Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE,
        },
        System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_REPARSE_POINT},
    },
};

#[cfg(test)]
pub(crate) fn writer(path: &Path) -> io::Result<File> {
    writer_with_access(path, windows_sys::Win32::Foundation::GENERIC_WRITE)
}

pub(crate) fn writer_with_access(path: &Path, access: u32) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(access)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}
pub(crate) fn set_mount_point(file: &File, target: &Path) -> io::Result<()> {
    MountPointBuffer::prepare(target)?.apply(file)
}
struct MountPointBuffer {
    storage: Vec<u64>,
    bytes: u32,
}
impl MountPointBuffer {
    fn prepare(target: &Path) -> io::Result<Self> {
        let target = std::fs::canonicalize(target)?;
        let target: Vec<_> = target.as_os_str().encode_wide().collect();
        if !target.starts_with(&[92, 92, 63, 92]) || target.get(5) != Some(&58) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Owned junction control requires a resolved DOS target",
            ));
        }
        let mut substitute = vec![92, 63, 63, 92];
        substitute.extend_from_slice(&target[4..]);
        let print = &target[4..];
        let path_units = substitute
            .len()
            .checked_add(print.len())
            .and_then(|n| n.checked_add(2))
            .ok_or_else(|| io::Error::other("Reparse length overflow"))?;
        let header = offset_of!(REPARSE_DATA_BUFFER, Anonymous);
        let path_offset = header + offset_of!(REPARSE_DATA_BUFFER_0_1, PathBuffer);
        let total = path_offset
            .checked_add(
                path_units
                    .checked_mul(2)
                    .ok_or_else(|| io::Error::other("Reparse length overflow"))?,
            )
            .ok_or_else(|| io::Error::other("Reparse length overflow"))?;
        if total > 16 * 1024 {
            return Err(io::Error::other(
                "Owned reparse control exceeds native bound",
            ));
        }
        let data_len = u16::try_from(total - header).map_err(io::Error::other)?;
        let sub_len = u16::try_from(substitute.len() * 2).map_err(io::Error::other)?;
        let print_offset = u16::try_from((substitute.len() + 1) * 2).map_err(io::Error::other)?;
        let print_len = u16::try_from(print.len() * 2).map_err(io::Error::other)?;
        let mut storage = vec![
            0u64;
            total
                .max(size_of::<REPARSE_DATA_BUFFER>())
                .div_ceil(size_of::<u64>())
        ];
        let buffer = storage.as_mut_ptr().cast::<REPARSE_DATA_BUFFER>();
        // SAFETY: generated structures supply all offsets; u64 storage meets their
        // alignment and covers full struct plus checked UTF-16 tail. All lengths are
        // bounded above, buffers remain live throughout this synchronous native call.
        // The handle owns the exact freshly created test directory, never owner data.
        unsafe {
            buffer.write(REPARSE_DATA_BUFFER::default());
            ptr::addr_of_mut!((*buffer).ReparseTag)
                .write(windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_MOUNT_POINT);
            ptr::addr_of_mut!((*buffer).ReparseDataLength).write(data_len);
            let point = ptr::addr_of_mut!((*buffer).Anonymous.MountPointReparseBuffer);
            ptr::addr_of_mut!((*point).SubstituteNameOffset).write(0);
            ptr::addr_of_mut!((*point).SubstituteNameLength).write(sub_len);
            ptr::addr_of_mut!((*point).PrintNameOffset).write(print_offset);
            ptr::addr_of_mut!((*point).PrintNameLength).write(print_len);
            let path = storage
                .as_mut_ptr()
                .cast::<u8>()
                .add(path_offset)
                .cast::<u16>();
            ptr::copy_nonoverlapping(substitute.as_ptr(), path, substitute.len());
            ptr::copy_nonoverlapping(print.as_ptr(), path.add(substitute.len() + 1), print.len());
        };
        Ok(Self {
            storage,
            bytes: total as u32,
        })
    }
    fn apply(&self, file: &File) -> io::Result<()> {
        let mut returned = 0;
        // SAFETY: previously checked aligned native input remains immutable/live;
        // exact byte length covers it. File is synchronous and retained through the
        // call; optional output/OVERLAPPED are null, returned-count slot is writable.
        let success = unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                FSCTL_SET_REPARSE_POINT,
                self.storage.as_ptr().cast(),
                self.bytes,
                ptr::null_mut(),
                0,
                &mut returned,
                ptr::null_mut(),
            )
        };
        if success == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PathContext,
        windows::{handle, test_actor::RestrictedToken},
    };
    use windows_sys::Win32::{
        Foundation::{ERROR_DIR_NOT_EMPTY, GENERIC_READ, GENERIC_WRITE},
        Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, FILE_WRITE_ATTRIBUTES},
    };
    #[test]
    fn platform_assumption_nonempty_directory_cannot_become_junction() {
        let actor = RestrictedToken::enter().unwrap();
        let root = tempfile::tempdir().unwrap();
        let context = PathContext::capture(&std::env::current_dir().unwrap()).unwrap();
        let target = root.path().join("target");
        let empty = root.path().join("empty");
        let nonempty = root.path().join("nonempty");
        for path in [&target, &empty, &nonempty] {
            std::fs::create_dir(path).unwrap();
        }
        std::fs::write(target.join("sentinel"), b"owned platform-fact target").unwrap();
        let prepared = MountPointBuffer::prepare(&target).unwrap();
        let flags = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;
        let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE;
        let pin = handle::open(
            &context.resolve(&empty).unwrap(),
            GENERIC_READ | GENERIC_WRITE,
            sharing,
            flags,
        )
        .unwrap();
        let original = handle::identity(&pin).unwrap();
        let attributes = writer_with_access(&empty, FILE_WRITE_ATTRIBUTES).unwrap();
        prepared
            .apply(&attributes)
            .expect("Attribute-only conversion of the empty control must actually succeed");
        assert_eq!(handle::identity(&pin).unwrap(), original);
        assert_ne!(
            handle::attributes(&pin).unwrap().FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT,
            0
        );
        assert_eq!(
            std::fs::read(empty.join("sentinel")).unwrap(),
            b"owned platform-fact target"
        );
        handle::close(attributes).unwrap();
        handle::close(pin).unwrap();
        std::fs::remove_dir(&empty).unwrap();
        let pin = handle::open(
            &context.resolve(&nonempty).unwrap(),
            GENERIC_READ | GENERIC_WRITE,
            sharing,
            flags,
        )
        .unwrap();
        let original = handle::identity(&pin).unwrap();
        let guard = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(nonempty.join("occupant"))
            .unwrap();
        assert_eq!(
            guard.metadata().unwrap().len(),
            0,
            "The relied-on fact must cover a ZERO-byte entry"
        );
        let attributes = writer_with_access(&nonempty, FILE_WRITE_ATTRIBUTES).unwrap();
        let refusal = prepared
            .apply(&attributes)
            .expect_err("The occupied directory must refuse conversion");
        assert_eq!(
            refusal.raw_os_error(),
            Some(ERROR_DIR_NOT_EMPTY as i32),
            "An unrelated setup/permission error is not proof: {refusal}"
        );
        assert_eq!(handle::identity(&pin).unwrap(), original);
        assert_eq!(
            handle::attributes(&pin).unwrap().FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT,
            0
        );
        assert_eq!(
            std::fs::read(target.join("sentinel")).unwrap(),
            b"owned platform-fact target"
        );
        handle::close(attributes).unwrap();
        handle::close(guard).unwrap();
        handle::close(pin).unwrap();
        drop(actor);
        root.close().unwrap();
    }
}
