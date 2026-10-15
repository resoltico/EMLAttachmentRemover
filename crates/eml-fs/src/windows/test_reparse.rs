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
        Foundation::GENERIC_WRITE,
        Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE,
        },
        System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_REPARSE_POINT},
    },
};

pub(crate) fn writer(path: &Path) -> io::Result<File> {
    writer_with_access(path, GENERIC_WRITE)
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
        Foundation::{GENERIC_READ, GENERIC_WRITE},
        Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, FILE_WRITE_ATTRIBUTES},
    };
    #[test]
    fn attribute_only_writer_must_not_convert_strictly_pinned_directory() {
        let _actor = RestrictedToken::enter().unwrap();
        let root = tempfile::tempdir().unwrap();
        let context = PathContext::capture().unwrap();
        let target = root.path().join("target");
        let baseline = root.path().join("baseline");
        let pinned = root.path().join("pinned");
        for path in [&target, &baseline, &pinned] {
            std::fs::create_dir(path).unwrap();
        }
        std::fs::write(
            target.join("sentinel"),
            b"owned attribute-only reparse target",
        )
        .unwrap();
        assert_eq!(std::fs::read_dir(&baseline).unwrap().count(), 0);
        let prepared = MountPointBuffer::prepare(&target)
            .expect("Owned reparse buffer preparation failed before either side");
        let base_operation = context.resolve(&baseline).unwrap();
        let share_all = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
        let flags = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;
        let view = handle::open(
            &base_operation,
            windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES,
            share_all,
            flags,
        )
        .unwrap();
        let before = handle::identity(&view).unwrap();
        drop(view);
        let writer = writer_with_access(&baseline, FILE_WRITE_ATTRIBUTES)
            .expect("Same restricted actor's attribute-only baseline open failed");
        prepared.apply(&writer).expect("Same restricted actor's attribute-only baseline FSCTL failed; this proves no exclusion");
        let view = handle::open(
            &base_operation,
            windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES,
            share_all,
            flags,
        )
        .unwrap();
        let observed = handle::attributes(&view).unwrap();
        assert_ne!(observed.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT, 0);
        assert_eq!(
            observed.ReparseTag,
            windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_MOUNT_POINT
        );
        assert_eq!(handle::identity(&view).unwrap(), before);
        assert_eq!(
            std::fs::read(baseline.join("sentinel")).unwrap(),
            b"owned attribute-only reparse target"
        );
        drop(view);
        drop(writer);
        std::fs::remove_dir(&baseline).unwrap();
        println!(
            "Baseline: restricted actor attribute-only FSCTL created real mount point, ID unchanged, target sentinel read"
        );
        assert_eq!(std::fs::read_dir(&pinned).unwrap().count(), 0);
        let operation = context.resolve(&pinned).unwrap();
        let pin = handle::open(
            &operation,
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
        .unwrap();
        let original = handle::identity(&pin).unwrap();
        let mut unexpected = None;
        let converted = match writer_with_access(&pinned, FILE_WRITE_ATTRIBUTES) {
            Ok(writer) => {
                let result = prepared.apply(&writer);
                println!("Pinned attribute-only open succeeded; actual FSCTL result={result:?}");
                match result {
                    Ok(()) => true,
                    Err(error) => {
                        if !matches!(error.raw_os_error(), Some(5 | 32)) {
                            unexpected = Some(("FSCTL", error));
                        }
                        false
                    }
                }
            }
            Err(error) => {
                println!("Pinned attribute-only open refused: {error}");
                if !matches!(error.raw_os_error(), Some(5 | 32)) {
                    unexpected = Some(("open", error));
                }
                false
            }
        };
        let actual = handle::attributes(&pin).unwrap();
        let current = handle::identity(&pin).unwrap();
        let entry = handle::open(
            &operation,
            windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES,
            share_all,
            flags,
        )
        .unwrap();
        assert_eq!(handle::identity(&entry).unwrap(), original);
        drop(entry);
        println!(
            "Pinned native identity before={original:?} after={current:?}; attributes={:#x} tag={:#x}",
            actual.FileAttributes, actual.ReparseTag
        );
        assert_eq!(current, original);
        let redirected = if converted {
            Some(std::fs::read(pinned.join("sentinel")).unwrap())
        } else {
            None
        };
        drop(pin);
        std::fs::remove_dir(&pinned).unwrap();
        assert_eq!(
            std::fs::read(target.join("sentinel")).unwrap(),
            b"owned attribute-only reparse target"
        );
        root.close().unwrap();
        if let Some(bytes) = redirected {
            assert_eq!(bytes, b"owned attribute-only reparse target");
        }
        assert!(
            unexpected.is_none(),
            "Unexpected native result does not prove exclusion: {unexpected:?}"
        );
        assert!(
            !converted && actual.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "SAFETY GATE FAILED: attribute-only actor converted a strict pinned directory; pathname creation is not confined"
        );
    }
}
